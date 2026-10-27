# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import io
import itertools
import json
import math
import subprocess
import threading
import time
import unittest
from collections.abc import Callable, Iterator
from contextlib import contextmanager, redirect_stderr
from types import SimpleNamespace
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.core import scheduler as scheduler_module
from eden.integration.correctness.exploratory.core.effects import no_footprint
from eden.integration.correctness.exploratory.core.facts import EMPTY_RESULT
from eden.integration.correctness.exploratory.core.history import (
    FAIL,
    INFO,
    InMemorySink,
    INVOKE,
    OK,
    SKIPPED,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    Environment,
    from_json,
    RunSpec,
    STARTED,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    COMPLETED,
    HARNESS_FAILURE,
    HarnessFailure,
    INVALID_BACKEND_RESPONSE,
    OPERATION_TIMEOUT,
    run_actions,
    RUNNER_DEADLINE,
)
from eden.integration.correctness.exploratory.core.target import (
    ActionDef,
    Binding,
    Grouping,
    ProcessRegistry,
    Target,
)


def _spec(
    actors: list[tuple[str, list[str]]],
    actions: list[Action],
    operation_deadline: float = 60.0,
    *,
    cleanup_deadline: float = 5.0,
    runner_deadline: float = 120.0,
) -> RunSpec:
    return RunSpec(
        seed=1,
        target="test",
        environment=Environment(target={}),
        budgets=Budgets(
            operation_deadline_seconds=operation_deadline,
            cleanup_deadline_seconds=cleanup_deadline,
            runner_deadline_seconds=runner_deadline,
        ),
        actors=tuple(Actor(id=a, actions=tuple(ids)) for a, ids in actors),
        actions=tuple(actions),
    )


def _op(action_id: str, **kwargs: Any) -> Action:
    return Action(
        id=action_id,
        kind="read",
        deadline="operation",
        payload={"path": "r", "channel": "fuse"},
        **kwargs,
    )


def _clock() -> Any:
    return itertools.count().__next__


# Real seconds before a test gives up on an event, a join, or virtual time
# that cannot advance. It guards against hangs; it measures nothing.
_HANG_GUARD = 60.0


def _session_leader(test: unittest.TestCase) -> int:
    """Start a real session leader that ``test`` kills and reaps at cleanup."""
    leader = subprocess.Popen(["sleep", "60"], start_new_session=True)
    test.addCleanup(leader.wait)
    test.addCleanup(leader.kill)
    return leader.pid


class _VirtualTime:
    """Discrete-event time for `run_actions`.

    Under `installed()`, the scheduler's `time.monotonic` reads `now`, and
    its timed thread joins wait in virtual seconds. Virtual time advances
    only when the test thread and every live scheduler thread are blocked
    in `wait`, and then jumps to the earliest pending timeout, so each
    deadline a run reaches is fixed by the spec rather than by how fast the
    host schedules threads.
    """

    def __init__(self) -> None:
        self.now = 0.0
        self._cond = threading.Condition()
        self._live = 1
        self._waits: dict[object, tuple[Callable[[], bool], float]] = {}

    def monotonic(self) -> float:
        """Return the current virtual time."""
        return self.now

    def wait(self, ready: Callable[[], bool], timeout: float | None) -> bool:
        """Block until `ready()` or until `timeout` virtual seconds pass.

        `ready` must be a cheap read of shared state, such as `Event.is_set`.
        Raises AssertionError when nothing can make progress for
        `_HANG_GUARD` real seconds.
        """
        token = object()
        with self._cond:
            wake = math.inf if timeout is None else self.now + timeout
            self._waits[token] = (ready, wake)
            guard = time.monotonic() + _HANG_GUARD
            try:
                while not ready():
                    if self.now >= wake:
                        return False
                    if self._advance():
                        continue
                    if time.monotonic() > guard:
                        raise AssertionError("virtual time cannot advance")
                    self._cond.wait(0.002)
                return True
            finally:
                del self._waits[token]

    def sleep(self, seconds: float) -> None:
        """Spend `seconds` of virtual time."""
        self.wait(lambda: False, seconds)

    def park(self, event: threading.Event) -> None:
        """Block in virtual time until `event` is set."""
        self.wait(event.is_set, None)

    def _advance(self) -> bool:
        """Jump to the earliest timeout once every live thread is blocked."""
        if len(self._waits) < self._live:
            return False
        if any(ready() for ready, _ in self._waits.values()):
            return False
        wake = min(wake for _, wake in self._waits.values())
        if math.isinf(wake) or wake <= self.now:
            return False
        self.now = wake
        self._cond.notify_all()
        return True

    def _thread_class(self) -> type[threading.Thread]:
        clock = self

        class _Thread(threading.Thread):
            """A scheduler thread whose timed joins wait in virtual time."""

            _virtual_done = False

            def start(self) -> None:
                with clock._cond:
                    clock._live += 1
                super().start()

            def run(self) -> None:
                try:
                    super().run()
                finally:
                    with clock._cond:
                        clock._live -= 1
                        self._virtual_done = True
                        clock._cond.notify_all()

            def join(self, timeout: float | None = None) -> None:
                if clock.wait(lambda: self._virtual_done, timeout):
                    super().join()

        return _Thread

    @contextmanager
    def installed(self) -> Iterator[_VirtualTime]:
        """Run the scheduler on this clock for the duration of the block."""
        namespace = {**vars(threading), "Thread": self._thread_class()}
        with (
            mock.patch.object(
                scheduler_module, "time", SimpleNamespace(monotonic=self.monotonic)
            ),
            mock.patch.object(
                scheduler_module, "threading", SimpleNamespace(**namespace)
            ),
        ):
            yield self


def _run_with_held_finished(
    failing: Callable[[Action], BackendResult], unwind: Callable[[], None]
) -> tuple[Any, list[str], InMemorySink]:
    """Run `x1` (via `failing`) and `y1` gated on its finish; hold x1's FINISHED.

    After the gate table publishes FINISHED for `x1`, the publishing thread
    waits up to 0.5 s for `y1` to be invoked. Returns the run status, the
    invoked action ids, and the history.
    """
    invoked: list[str] = []
    dependent_invoked = threading.Event()

    def backend(action: Action) -> BackendResult:
        invoked.append(action.id)
        if action.id == "y1":
            dependent_invoked.set()
            return BackendResult(OK)
        return failing(action)

    mark = scheduler_module._GateTable.mark

    def held_mark(table: Any, action_id: str, event: str) -> None:
        mark(table, action_id, event)
        if (action_id, event) == ("x1", "finished"):
            dependent_invoked.wait(0.5)

    spec = _spec(
        [("a", ["x1"]), ("b", ["y1"])],
        [_op("x1"), _op("y1", requires=(("x1", "finished"),))],
        operation_deadline=0.1,
    )
    sink = InMemorySink()
    with mock.patch.object(scheduler_module._GateTable, "mark", held_mark):
        status = run_actions(spec, backend, sink, _clock(), unwind=unwind)
    return status, invoked, sink


class SchedulerTest(unittest.TestCase):
    def test_target_owned_while_payload_dispatches_unchanged(self) -> None:
        def action_def(kind: str, fields: frozenset[str]) -> ActionDef:
            return ActionDef(
                kind=kind,
                axis="input",
                fields=fields,
                required=fields,
                types={name: str for name in fields},
                validate=lambda _spec, _action: None,
                modeled=False,
                spawns_process=False,
                sample=lambda _rng, _context: None,
                result=EMPTY_RESULT,
                footprint=no_footprint,
                roles=frozenset(),
                binding=Binding.NONE,
                grouping=Grouping.NONE,
            )

        target = Target(
            name="test",
            actions={
                "marker": action_def("marker", frozenset()),
                "target_owned": action_def("target_owned", frozenset({"while"})),
            },
            sites={},
            properties=(),
            install_witnessed=lambda _spec, _facts, _action: False,
            environment_fields={},
            make_environment=lambda _spec, _sink: mock.Mock(),
            make_backend=lambda _env, _spec, _control: mock.Mock(),
            red_controls=frozenset(),
        )
        spec = from_json(
            json.dumps(
                {
                    "schema": "edenfs-exploratory-run",
                    "schema_version": 1,
                    "seed": 1,
                    "target": "test",
                    "environment": {"target": {}},
                    "budgets": {
                        "operation_deadline_seconds": 30,
                        "cleanup_deadline_seconds": 5,
                        "runner_deadline_seconds": 60,
                    },
                    "actors": [
                        {"id": "marker", "actions": ["target"]},
                        {"id": "owner", "actions": ["owned"]},
                    ],
                    "actions": {
                        "target": {"kind": "marker", "deadline": "operation"},
                        "owned": {
                            "kind": "target_owned",
                            "deadline": "operation",
                            "requires": [["target", "finished"]],
                            "while": "target",
                        },
                    },
                }
            ),
            target,
        )
        dispatched: list[Action] = []

        status = run_actions(
            spec,
            lambda action: dispatched.append(action) or BackendResult(OK),
            InMemorySink(),
            _clock(),
        )

        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        self.assertEqual(dispatched, [spec.action("target"), spec.action("owned")])
        self.assertEqual(dispatched[1].payload, {"while": "target"})

    def test_process_registry_tracks_exact_action_lifetime(self) -> None:
        registry = ProcessRegistry()
        process = mock.Mock(pid=_session_leader(self))
        waiting: list[Any] = []
        blocked = threading.Event()

        class _ObservedCondition(threading.Condition):
            def wait(self, timeout: float | None = None) -> bool:
                blocked.set()
                return super().wait(timeout)

        registry._condition = _ObservedCondition()
        thread = threading.Thread(
            target=lambda: waiting.append(registry.wait("a1", _HANG_GUARD))
        )
        thread.start()
        self.assertTrue(blocked.wait(_HANG_GUARD))
        registry.publish("a1", process, 5.5)
        thread.join(timeout=_HANG_GUARD)

        self.assertFalse(thread.is_alive())
        publication = waiting[0]
        self.assertIsNotNone(publication)
        assert publication is not None
        self.assertIs(publication.process, process)
        self.assertEqual(publication.pgid, process.pid)
        self.assertEqual(publication.spawned_at, 5.5)
        self.assertEqual(registry.lookup("a1"), process.pid)
        self.assertIsNone(registry.finished("a1"))
        registry.retire("a1", process)
        self.assertIsNone(registry.lookup("a1"))
        self.assertIsNone(registry.wait("a1", 1.0))
        self.assertEqual(process.pid, registry.finished("a1"))
        self.assertIsNone(registry.finished("a2"))

    def test_process_registry_wait_is_action_specific_and_bounded(self) -> None:
        registry = ProcessRegistry()
        registry.publish(
            "other", mock.Mock(pid=_session_leader(self)), time.monotonic()
        )
        started = time.monotonic()

        self.assertIsNone(registry.wait("wanted", 0.05))
        self.assertGreaterEqual(time.monotonic() - started, 0.05)

    def test_process_registry_refuses_a_process_that_does_not_lead_its_group(
        self,
    ) -> None:
        registry = ProcessRegistry()
        child = subprocess.Popen(["sleep", "60"])
        self.addCleanup(child.wait)
        self.addCleanup(child.kill)
        with self.assertRaisesRegex(RuntimeError, "does not lead its group"):
            registry.publish("a1", child, time.monotonic())
        self.assertIsNone(registry.lookup("a1"))

    def test_process_registry_refuses_republish_after_retire(self) -> None:
        registry = ProcessRegistry()
        process = mock.Mock(pid=_session_leader(self))
        registry.publish("a1", process, 1.0)
        registry.retire("a1", process)
        with self.assertRaisesRegex(RuntimeError, "published twice"):
            registry.publish("a1", process, 2.0)
        self.assertIsNone(registry.lookup("a1"))

    def test_process_registry_retire_requires_the_exact_owner(self) -> None:
        registry = ProcessRegistry()
        owner = mock.Mock(pid=_session_leader(self))
        registry.publish("a1", owner, 1.0)
        with self.assertRaisesRegex(RuntimeError, "is not active"):
            registry.retire("a1", mock.Mock(pid=owner.pid))
        self.assertEqual(registry.lookup("a1"), owner.pid)

    def test_completion_is_emitted_before_finished_is_published(self) -> None:
        order: list[tuple[str, str]] = []

        class _OrderSink(InMemorySink):
            def emit(self, event: Any) -> None:
                super().emit(event)
                if event.type in (OK, INVOKE):
                    order.append((event.type, event.data["action"]))

        mark = scheduler_module._GateTable.mark

        def recording_mark(table: Any, action_id: str, event: str) -> None:
            order.append(("mark " + event, action_id))
            mark(table, action_id, event)

        spec = _spec(
            [("a", ["x1"]), ("b", ["y1"])],
            [_op("x1"), _op("y1", requires=(("x1", "finished"),))],
        )
        with mock.patch.object(scheduler_module._GateTable, "mark", recording_mark):
            status = run_actions(
                spec, lambda action: BackendResult(OK), _OrderSink(), _clock()
            )
        self.assertEqual(status.status, COMPLETED)
        self.assertLess(order.index((OK, "x1")), order.index(("mark finished", "x1")))
        self.assertLess(
            order.index(("mark finished", "x1")), order.index((INVOKE, "y1"))
        )

    def test_cross_edge_actors_really_overlap(self) -> None:
        barrier = threading.Barrier(2, timeout=_HANG_GUARD)
        seen: list[str] = []

        def execute(action: Action) -> BackendResult:
            seen.append(action.id)
            barrier.wait()
            return BackendResult(OK)

        spec = _spec(
            [("a", ["a1"]), ("b", ["b1"])],
            [_op("a1"), _op("b1")],
        )
        status = run_actions(spec, execute, InMemorySink(), _clock())
        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        self.assertEqual(sorted(seen), ["a1", "b1"])

    def test_started_is_admission_not_backend_engagement(self) -> None:
        """`started` releases dependents when the call is admitted, not when it engages.

        `a` cannot return until `b` runs, and `b` requires only `(a, started)`,
        so the run completes only if admission alone publishes `started`.
        The history carries no separate engagement fact for `a`.
        """
        engaged = threading.Event()
        waited: list[bool] = []

        def backend(action: Action) -> BackendResult:
            if action.id == "a":
                waited.append(engaged.wait(timeout=_HANG_GUARD))
            else:
                engaged.set()
            return BackendResult(OK)

        spec = _spec(
            [("p", ["a"]), ("q", ["b"])],
            [_op("a"), _op("b", requires=(("a", STARTED),))],
        )
        sink = InMemorySink()
        status = run_actions(spec, backend, sink, _clock())
        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        self.assertEqual([True], waited)
        types = [(e.type, e.data.get("action")) for e in sink.events]
        self.assertLess(types.index((INVOKE, "b")), types.index((OK, "a")), types)
        self.assertEqual(
            [kind for kind, action in types if action == "a"], [INVOKE, OK], types
        )

    def test_completions_cover_every_action_once(self) -> None:
        spec = _spec(
            [("a", ["w1", "r1"]), ("b", ["t1"])],
            [_op("w1"), _op("r1"), _op("t1")],
        )
        sink = InMemorySink()
        status = run_actions(spec, lambda action: BackendResult(OK), sink, _clock())
        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        invokes = [e for e in sink.events if e.type == INVOKE]
        completions = [e for e in sink.events if e.type in (OK, FAIL, INFO)]
        self.assertEqual(len(invokes), 3)
        self.assertEqual(len(completions), 3)
        self.assertEqual(
            sorted(e.data["action"] for e in completions), ["r1", "t1", "w1"]
        )

    def test_fail_does_not_stop_peers(self) -> None:
        def execute(action: Action) -> BackendResult:
            if action.id == "w1":
                return BackendResult(FAIL, {"errno": "EIO"})
            return BackendResult(OK)

        spec = _spec(
            [("a", ["w1", "r1"]), ("b", ["t1"])],
            [_op("w1"), _op("r1"), _op("t1")],
        )
        sink = InMemorySink()
        status = run_actions(spec, execute, sink, _clock())
        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        by_action = {
            e.data["action"]: e for e in sink.events if e.type in (OK, FAIL, INFO)
        }
        self.assertEqual(by_action["w1"].type, FAIL)
        self.assertEqual(by_action["w1"].data["errno"], "EIO")
        self.assertEqual(by_action["r1"].type, OK)
        self.assertEqual(by_action["t1"].type, OK)
        self.assertEqual(sink.of_type(SKIPPED), [])

    def test_backend_raise_is_invalid_response_and_skips_rest(self) -> None:
        def execute(action: Action) -> BackendResult:
            raise RuntimeError("backend exploded")

        spec = _spec([("a", ["x1", "x2"])], [_op("x1"), _op("x2")])
        sink = InMemorySink()
        cleaned: list[str] = []
        with redirect_stderr(io.StringIO()) as stderr:
            status = run_actions(
                spec, execute, sink, _clock(), cleanup=lambda: cleaned.append("ran")
            )
        self.assertEqual(
            (status.status, status.reason, status.error),
            (
                HARNESS_FAILURE,
                INVALID_BACKEND_RESPONSE,
                "RuntimeError: backend exploded",
            ),
        )
        self.assertIn("Traceback", stderr.getvalue())
        self.assertIn("RuntimeError: backend exploded", stderr.getvalue())
        completions = [e for e in sink.events if e.type in (OK, FAIL, INFO)]
        self.assertEqual(len(completions), 1)
        self.assertEqual(completions[0].type, INFO)
        self.assertEqual(completions[0].data["reason"], INVALID_BACKEND_RESPONSE)
        skipped = sink.of_type(SKIPPED)
        self.assertEqual([e.data["action"] for e in skipped], ["x2"])
        self.assertEqual(cleaned, ["ran"])

    def test_backend_error_text_is_bounded(self) -> None:
        def execute(action: Action) -> BackendResult:
            raise ValueError("é" * 1000)

        spec = _spec([("a", ["x1"])], [_op("x1")])
        with redirect_stderr(io.StringIO()):
            status = run_actions(spec, execute, InMemorySink(), _clock())
        error = status.error
        assert error is not None
        self.assertTrue(error.startswith("ValueError: é"), error)
        self.assertLessEqual(
            len(error.encode()), scheduler_module.MAX_BACKEND_ERROR_BYTES
        )

    def test_sweep_completes_every_action_before_cleanup_when_a_start_fails(
        self,
    ) -> None:
        """An actor thread that cannot start still leaves every action completed before cleanup."""
        spec = _spec([("a", ["x1"]), ("b", ["y1"])], [_op("x1"), _op("y1")])
        sink = InMemorySink()
        completed_at_cleanup: list[set[str]] = []
        real_start = threading.Thread.start

        def start(thread: threading.Thread) -> None:
            if thread.name == "actor-b":
                raise RuntimeError("can't start new thread")
            real_start(thread)

        def cleanup() -> None:
            completed_at_cleanup.append(
                {
                    e.data["action"]
                    for e in sink.events
                    if e.type in (OK, FAIL, INFO, SKIPPED)
                }
            )

        with mock.patch.object(threading.Thread, "start", start):
            with self.assertRaisesRegex(RuntimeError, "can't start new thread"):
                run_actions(
                    spec,
                    lambda action: BackendResult(OK, {}),
                    sink,
                    _clock(),
                    cleanup=cleanup,
                )
        self.assertEqual([{"x1", "y1"}], completed_at_cleanup)

    def test_non_result_response_rejected(self) -> None:
        spec = _spec([("a", ["q1"])], [_op("q1")])
        sink = InMemorySink()
        status = run_actions(spec, lambda action: "ok", sink, _clock())  # type: ignore[return-value]
        self.assertEqual(
            (status.status, status.reason),
            (HARNESS_FAILURE, INVALID_BACKEND_RESPONSE),
        )

    def test_timeout_emits_info_and_stops_admission(self) -> None:
        release = threading.Event()

        def parked(action: Action) -> BackendResult:
            release.wait(_HANG_GUARD)
            return BackendResult(OK)

        spec = _spec(
            [("a", ["s1", "s2"]), ("b", ["t1"])],
            [_op("s1"), _op("s2"), _op("t1")],
            operation_deadline=0.1,
        )
        sink = InMemorySink()
        try:
            status = run_actions(spec, parked, sink, _clock(), unwind=release.set)
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, OPERATION_TIMEOUT)
        )
        # Both actors' watchdogs fire; s2 never starts and is skipped.
        by_action = {
            e.data["action"]: e for e in sink.events if e.type in (OK, FAIL, INFO)
        }
        self.assertEqual(set(by_action), {"s1", "t1"})
        for action_id in ("s1", "t1"):
            self.assertEqual(by_action[action_id].type, INFO)
            self.assertEqual(by_action[action_id].data["reason"], OPERATION_TIMEOUT)
        self.assertEqual([e.data["action"] for e in sink.of_type(SKIPPED)], ["s2"])

    def test_admission_stops_before_a_failed_action_publishes_finished(
        self,
    ) -> None:
        """A dependent gated on a timed-out or raising action never starts.

        The gate table holds FINISHED for `x1` until `y1` is invoked or a
        bound passes, so a scheduler that publishes FINISHED before it stops
        admission invokes `y1` inside that window.
        """
        release = threading.Event()

        def timed_out(action: Action) -> BackendResult:
            release.wait(_HANG_GUARD)
            return BackendResult(OK)

        def raising(action: Action) -> BackendResult:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)

        try:
            for name, failing, reason in (
                ("timeout", timed_out, OPERATION_TIMEOUT),
                ("harness-error", raising, INVALID_BACKEND_RESPONSE),
            ):
                with self.subTest(name):
                    status, invoked, sink = _run_with_held_finished(
                        failing, release.set
                    )
                    self.assertEqual(
                        (HARNESS_FAILURE, reason), (status.status, status.reason)
                    )
                    self.assertEqual(["x1"], invoked)
                    self.assertEqual(
                        ["y1"], [e.data["action"] for e in sink.of_type(SKIPPED)]
                    )
        finally:
            release.set()

    def test_deadline_run_action_is_not_watched(self) -> None:
        clock = _VirtualTime()

        def slow(action: Action) -> BackendResult:
            clock.sleep(0.3)
            return BackendResult(OK)

        action = Action(
            id="c0",
            kind="checkout",
            deadline="run",
            payload={"revision": 0, "via": "goto", "mode": "normal"},
        )
        spec = _spec([("a", ["c0"])], [action], operation_deadline=0.1)
        sink = InMemorySink()
        with clock.installed():
            status = run_actions(spec, slow, sink, _clock())
        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        self.assertEqual(sink.of_type(OK)[0].data["action"], "c0")

    def test_final_operation_outlives_cleanup_budget(self) -> None:
        """The outer operation watchdog is independent of cleanup policy."""
        cleanup_deadline = 0.2
        spec = _spec(
            [("a", ["last"])],
            [_op("last")],
            operation_deadline=_HANG_GUARD,
            cleanup_deadline=cleanup_deadline,
        )
        sink = InMemorySink()
        clock = _VirtualTime()

        def backend(_action: Action) -> BackendResult:
            clock.sleep(2 * cleanup_deadline)
            return BackendResult(OK)

        with clock.installed():
            status = run_actions(spec, backend, sink, _clock())
        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        self.assertEqual([e.data["action"] for e in sink.of_type(OK)], ["last"])

    def test_run_deadline_before_last_action_cannot_block_join(self) -> None:
        """Run watchdog policy bounds a hung action even before the last one.

        The returning checkout in test_deadline_run_action_is_not_watched
        is the accept twin: the operation watchdog does not judge checkout.
        """
        clock = _VirtualTime()
        release = threading.Event()
        cleaned: list[tuple[float, bool]] = []
        action = Action(id="c0", kind="checkout", deadline="run")
        spec = _spec(
            [("a", ["c0", "last"])],
            [action, _op("last")],
            cleanup_deadline=0.2,
            runner_deadline=0.5,
        )
        sink = InMemorySink()

        def backend(action: Action) -> BackendResult:
            if action.id == "c0":
                clock.park(release)
            return BackendResult(OK)

        def cleanup() -> None:
            cleaned.append((clock.now, release.is_set()))
            release.set()

        try:
            with clock.installed():
                status = run_actions(spec, backend, sink, _clock(), cleanup=cleanup)
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, "runner-deadline")
        )
        self.assertEqual([e.data["action"] for e in sink.of_type(SKIPPED)], ["last"])
        # Cleanup starts at the join bound D - C/2 while c0 is still parked.
        ((cleaned_at, released),) = cleaned
        self.assertAlmostEqual(0.5 - 0.2 / 2, cleaned_at)
        self.assertFalse(released, "run-deadline action blocked actor join")

    def test_hung_run_deadline_op_past_join_bound_is_harness_failure(self) -> None:
        """A hung checkout is harness-red at the reserved runner deadline.

        test_deadline_run_action_is_not_watched is the returning accept twin.
        """
        clock = _VirtualTime()
        release = threading.Event()

        def hung(action: Action) -> BackendResult:
            clock.park(release)
            return BackendResult(OK)

        action = Action(
            id="c0",
            kind="checkout",
            deadline="run",
            payload={"revision": 0, "via": "goto", "mode": "normal"},
        )
        spec = _spec(
            [("a", ["c0"])], [action], cleanup_deadline=0.2, runner_deadline=0.8
        )
        sink = InMemorySink()
        cleaned: list[float] = []

        def cleanup() -> None:
            cleaned.append(clock.now)
            release.set()

        try:
            with clock.installed():
                status = run_actions(spec, hung, sink, _clock(), cleanup=cleanup)
        finally:
            release.set()
        # Actors stop at D - C; the hung worker's join ends at D - C/2.
        self.assertEqual(1, len(cleaned))
        self.assertAlmostEqual(0.8 - 0.2 / 2, cleaned[0])
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, OPERATION_TIMEOUT)
        )
        # The invoked action completed as info; an invoked action is never
        # swept as skipped.
        by_action = {
            e.data["action"]: e for e in sink.events if e.type in (OK, FAIL, INFO)
        }
        self.assertEqual(by_action["c0"].type, INFO)
        self.assertEqual(by_action["c0"].data["reason"], OPERATION_TIMEOUT)
        invoked = {e.data["action"] for e in sink.events if e.type == INVOKE}
        self.assertEqual(invoked, {"c0"})
        self.assertEqual(sink.of_type(SKIPPED), [])

    def test_operation_op_in_flight_at_the_runner_deadline_is_runner_deadline(
        self,
    ) -> None:
        """An operation-deadline op still inside its own budget is not a timeout.

        test_hung_run_deadline_op_past_join_bound_is_harness_failure is the
        run-deadline twin, which has no watchdog and stays a timeout.
        """
        clock = _VirtualTime()
        release = threading.Event()

        def slow(action: Action) -> BackendResult:
            clock.park(release)
            return BackendResult(OK)

        spec = _spec(
            [("a", ["r0"])],
            [_op("r0")],
            operation_deadline=60.0,
            cleanup_deadline=0.2,
            runner_deadline=0.8,
        )
        sink = InMemorySink()
        try:
            with clock.installed():
                status = run_actions(spec, slow, sink, _clock(), cleanup=release.set)
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, RUNNER_DEADLINE)
        )
        (swept,) = sink.of_type(INFO)
        self.assertEqual(
            ("r0", RUNNER_DEADLINE), (swept.data["action"], swept.data["reason"])
        )
        self.assertEqual(sink.of_type(SKIPPED), [])

    def test_skips_are_deterministic(self) -> None:
        def execute(action: Action) -> BackendResult:
            if action.id == "f1":
                raise HarnessFailure("fault-witness-failed")
            return BackendResult(OK)

        spec = _spec(
            [("a", ["f1", "a2", "a3"]), ("b", ["b1", "b2"])],
            [_op("f1"), _op("a2"), _op("a3"), _op("b1"), _op("b2")],
        )
        # Thread interleavings vary run to run, so assert the per-run
        # invariant (skips are exactly the never-invoked set, in spec
        # order) rather than one exact skip set across runs.
        for _ in range(5):
            with self.subTest():
                sink = InMemorySink()
                status = run_actions(spec, execute, sink, _clock())
                self.assertEqual(
                    (status.status, status.reason),
                    (HARNESS_FAILURE, "fault-witness-failed"),
                )
                skipped = [e.data["action"] for e in sink.of_type(SKIPPED)]
                invoked = {e.data["action"] for e in sink.events if e.type == INVOKE}
                self.assertEqual(set(skipped) | invoked, {"f1", "a2", "a3", "b1", "b2"})
                self.assertTrue(set(skipped).isdisjoint(invoked))
                self.assertEqual(skipped, sorted(skipped))

    def test_cleanup_runs_on_every_exit_path(self) -> None:
        def _boom(action: Action) -> BackendResult:
            raise RuntimeError("boom")

        for mode in ("ok", "fail", "raise", "timeout"):
            with self.subTest(mode=mode):
                release = threading.Event()

                def parked(action: Action, gate: threading.Event = release) -> Any:
                    gate.wait(_HANG_GUARD)
                    return BackendResult(OK)

                backend: Any = {
                    "ok": lambda action: BackendResult(OK),
                    "fail": lambda action: BackendResult(FAIL),
                    "raise": _boom,
                    "timeout": parked,
                }[mode]
                deadline = 0.1 if mode == "timeout" else 30.0
                spec = _spec([("a", ["m1"])], [_op("m1")], operation_deadline=deadline)
                cleaned: list[str] = []
                try:
                    status = run_actions(
                        spec,
                        backend,
                        InMemorySink(),
                        _clock(),
                        unwind=release.set,
                        cleanup=lambda c=cleaned: c.append("ran"),
                    )
                finally:
                    release.set()
                self.assertEqual(cleaned, ["ran"])
                self.assertEqual(
                    {
                        "ok": (COMPLETED, None),
                        "fail": (COMPLETED, None),
                        "raise": (HARNESS_FAILURE, INVALID_BACKEND_RESPONSE),
                        "timeout": (HARNESS_FAILURE, OPERATION_TIMEOUT),
                    }[mode],
                    (status.status, status.reason),
                )

    def test_timed_out_worker_is_joined_before_cleanup(self) -> None:
        """A worker that returns after its watchdog fired is joined before cleanup."""
        release = threading.Event()
        returned = threading.Event()

        def outlives_watchdog(action: Action) -> BackendResult:
            release.wait(_HANG_GUARD)
            returned.set()
            return BackendResult(OK)

        class Sink(InMemorySink):
            def emit(self, event: Any) -> None:
                """Let the worker return once its watchdog has recorded the timeout."""
                super().emit(event)
                if event.type == INFO:
                    release.set()

        # The join bound C/2 is a hang guard for the released worker.
        spec = _spec(
            [("a", ["m1"])],
            [_op("m1")],
            operation_deadline=0.1,
            cleanup_deadline=2 * _HANG_GUARD,
            runner_deadline=4 * _HANG_GUARD,
        )
        seen_at_cleanup: list[bool] = []
        try:
            status = run_actions(
                spec,
                outlives_watchdog,
                Sink(),
                _clock(),
                cleanup=lambda: seen_at_cleanup.append(returned.is_set()),
            )
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, OPERATION_TIMEOUT)
        )
        self.assertEqual(seen_at_cleanup, [True])

    def test_timed_out_worker_join_is_bounded(self) -> None:
        clock = _VirtualTime()
        release = threading.Event()

        def hung(action: Action) -> BackendResult:
            clock.park(release)
            return BackendResult(OK)

        deadline = 0.1
        spec = _spec([("a", ["m1"])], [_op("m1")], operation_deadline=deadline)
        seen_at_cleanup: list[tuple[float, bool]] = []
        try:
            with clock.installed():
                status = run_actions(
                    spec,
                    hung,
                    InMemorySink(),
                    _clock(),
                    cleanup=lambda: seen_at_cleanup.append(
                        (clock.now, release.is_set())
                    ),
                )
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, OPERATION_TIMEOUT)
        )
        # The watchdog fires at `deadline`; the hung worker is joined for
        # exactly C/2 more, then cleanup runs with the worker still parked.
        ((cleaned_at, released),) = seen_at_cleanup
        self.assertFalse(released)
        self.assertAlmostEqual(
            deadline + spec.budgets.cleanup_deadline_seconds / 2, cleaned_at
        )

    def test_join_bound_is_one_deadline_for_all_actors(self) -> None:
        """Hung actors share one join deadline instead of one bound each."""
        clock = _VirtualTime()
        release = threading.Event()

        def hung(action: Action) -> BackendResult:
            clock.park(release)
            return BackendResult(OK)

        actions = [
            Action(
                id=f"c{index}",
                kind="checkout",
                deadline="run",
                payload={"revision": 0, "via": "goto", "mode": "normal"},
            )
            for index in range(3)
        ]
        spec = _spec(
            [(f"a{i}", [f"c{i}"]) for i in range(3)],
            actions,
            cleanup_deadline=0.2,
            runner_deadline=1.2,
        )
        try:
            with clock.installed():
                status = run_actions(spec, hung, InMemorySink(), _clock())
                returned_at = clock.now
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, OPERATION_TIMEOUT)
        )
        # Three hung actors and three hung workers all stop at D - C/2.
        self.assertAlmostEqual(1.2 - 0.2 / 2, returned_at)

    def test_actors_working_past_the_join_deadline_complete(self) -> None:
        """Actors may work past the cleanup budget within the runner budget."""

        clock = _VirtualTime()

        def slow(action: Action) -> BackendResult:
            clock.sleep(0.4)
            return BackendResult(OK)

        actors = [(f"a{i}", [f"a{i}s{j}" for j in range(4)]) for i in range(3)]
        spec = _spec(actors, [_op(i) for _, ids in actors for i in ids])
        sink = InMemorySink()
        with clock.installed():
            status = run_actions(
                spec, slow, sink, _clock(), cleanup_deadline_seconds=1.0
            )
        self.assertEqual((status.status, status.reason), (COMPLETED, None))
        self.assertEqual(len(sink.of_type(OK)), 12)

    def test_hung_actors_after_admission_stops_share_one_deadline(self) -> None:
        """Working then hung actors share the deadline measured from run start.

        test_actors_working_past_the_join_deadline_complete is the accept twin.
        """
        clock = _VirtualTime()
        release = threading.Event()
        work = 0.6
        bound = 0.5

        def backend(action: Action) -> BackendResult:
            if action.kind == "checkout":
                clock.park(release)
            else:
                clock.sleep(work)
            return BackendResult(OK)

        run_deadline = {"revision": 0, "via": "goto", "mode": "normal"}
        hung = [
            Action(id=f"c{i}", kind="checkout", deadline="run", payload=run_deadline)
            for i in range(3)
        ]
        spec = _spec(
            [(f"a{i}", [f"w{i}", f"c{i}"]) for i in range(3)],
            [*(_op(f"w{i}") for i in range(3)), *hung],
            cleanup_deadline=0.2,
            runner_deadline=work + bound + 0.2,
        )
        sink = InMemorySink()
        try:
            with clock.installed():
                status = run_actions(spec, backend, sink, _clock())
                returned_at = clock.now
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, OPERATION_TIMEOUT)
        )
        invoked = {e.data["action"] for e in sink.events if e.type == INVOKE}
        self.assertEqual(invoked, {"w0", "w1", "w2", "c0", "c1", "c2"})
        self.assertEqual(sink.of_type(SKIPPED), [])
        self.assertEqual(
            {e.data["action"] for e in sink.of_type(INFO)}, {"c0", "c1", "c2"}
        )
        # The work spans [0, work]; the shared deadline measured from run start,
        # not from when admission stopped, ends the joins at D - C/2.
        self.assertAlmostEqual(work + bound + 0.2 - 0.2 / 2, returned_at)

    def test_nothing_is_invoked_after_cleanup_starts(self) -> None:
        """A swept action is never invoked, and a run whose action ran after
        cleanup started never reports completed.

        ``t`` times out and stops admission. ``x`` hangs past the runner
        deadline minus the cleanup reservation. The sweep reaches ``x2`` first and
        skips it; only then does ``x`` return. Its actor must not go on to
        invoke the swept ``x2``, and the run is a harness failure.
        """
        virtual = _VirtualTime()
        release = threading.Event()
        actor_resumed = threading.Event()
        actor_done = threading.Event()
        actor = "p"

        def backend(action: Action) -> BackendResult:
            if action.id in ("x", "t"):
                virtual.park(release)
            return BackendResult(OK)

        ticks = itertools.count()

        def clock() -> float:
            if release.is_set() and threading.current_thread().name == f"actor-{actor}":
                actor_resumed.set()
            return float(next(ticks))

        class Sink(InMemorySink):
            def emit(self, event: Any) -> None:
                super().emit(event)
                if event.type == SKIPPED and event.data["action"] == "x2":
                    # The sweep holds the sink: let x return, and wait until
                    # its actor either reaches its next clock read or exits.
                    release.set()
                    while not actor_resumed.wait(0.01) and not actor_done.is_set():
                        pass

        run_deadline = {"revision": 0, "via": "goto", "mode": "normal"}
        actions = [
            Action(id="x2", kind="checkout", deadline="run", payload=run_deadline),
            Action(id="x", kind="checkout", deadline="run", payload=run_deadline),
            _op("t", requires=(("x", STARTED),)),
        ]
        spec = _spec(
            [(actor, ["x", "x2"]), ("q", ["t"])],
            actions,
            0.2,
            cleanup_deadline=0.2,
            runner_deadline=1.2,
        )
        sink = Sink()

        def join_actor() -> None:
            for thread in threading.enumerate():
                if thread.name == f"actor-{actor}":
                    thread.join(timeout=_HANG_GUARD)
            actor_done.set()

        try:
            with virtual.installed():
                status = run_actions(spec, backend, sink, clock, cleanup=join_actor)
        finally:
            release.set()
        types = [(e.type, e.data["action"]) for e in sink.events]
        skipped_at = types.index((SKIPPED, "x2"))
        self.assertNotIn((INVOKE, "x2"), types[skipped_at:], types)
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, OPERATION_TIMEOUT), types
        )


class CleanupReservationTest(unittest.TestCase):
    def test_deadline_skip_precedes_sweep_timeout(self) -> None:
        """Deadline skips win over timeouts created by sweeping.

        Earlier actual failures retain their reason; timely actions complete.
        """
        for case, expected in (
            ("blocked", "runner-deadline"),
            ("failure", "fault-witness-failed"),
            ("timely", None),
        ):
            with self.subTest(case=case):
                clock = _VirtualTime()
                release = threading.Event()
                spec = _spec(
                    [("a", ["first", "last"])],
                    [Action(id="first", kind="checkout", deadline="run"), _op("last")],
                    cleanup_deadline=0.4,
                    runner_deadline=1,
                )
                sink = InMemorySink()

                def backend(
                    _action: Action,
                    mode: str = case,
                    gate: threading.Event = release,
                    clock: _VirtualTime = clock,
                ) -> BackendResult:
                    if mode == "blocked":
                        clock.park(gate)
                    elif mode == "failure":
                        raise HarnessFailure("fault-witness-failed")
                    return BackendResult(OK)

                try:
                    with clock.installed():
                        status = run_actions(
                            spec,
                            backend,
                            sink,
                            _clock(),
                            runner_deadline=clock.now + 0.5,
                            unwind=release.set,
                        )
                finally:
                    release.set()
                self.assertEqual(status.reason, expected)
                if expected is not None:
                    self.assertEqual(status.status, HARNESS_FAILURE)
                    self.assertEqual(sink.of_type(INFO)[0].data["reason"], expected)
                    self.assertEqual(
                        [e.data["action"] for e in sink.of_type(SKIPPED)], ["last"]
                    )
                else:
                    self.assertEqual(status.status, COMPLETED)

    def test_wedged_worker_preserves_containment_slice(self) -> None:
        """Worker joins cannot consume containment's half.

        test_fault_unwind_precedes_join_and_close is the returning accept twin.
        """
        clock = _VirtualTime()
        release = threading.Event()
        closed: list[float] = []
        contained: list[tuple[list[str], float]] = []
        cleanup_deadline = 0.4
        spec = _spec(
            [("a", ["x"])],
            [_op("x")],
            operation_deadline=0.05,
            cleanup_deadline=cleanup_deadline,
            runner_deadline=1.0,
        )
        deadline = 0.5

        def backend(_action: Action) -> BackendResult:
            clock.park(release)
            return BackendResult(OK)

        try:
            with clock.installed():
                status = run_actions(
                    spec,
                    backend,
                    InMemorySink(),
                    _clock(),
                    runner_deadline=deadline,
                    contain=lambda active: contained.append(
                        (active, deadline - clock.now)
                    ),
                    cleanup=lambda: closed.append(deadline - clock.now),
                )
        finally:
            release.set()
        self.assertEqual(status.reason, OPERATION_TIMEOUT)
        self.assertEqual(contained[0][0], ["x"])
        # The timeout at 0.05 starts a C/2 join, which leaves D - 0.25 behind.
        self.assertAlmostEqual(deadline - 0.05 - cleanup_deadline / 2, contained[0][1])
        self.assertGreaterEqual(contained[0][1], cleanup_deadline / 2)
        self.assertGreaterEqual(closed[0], cleanup_deadline / 2)

    def test_wedged_unwind_preserves_containment_slice(self) -> None:
        """An unwind callback cannot spend containment's half.

        test_fault_unwind_precedes_join_and_close is the returning accept twin.
        """
        clock = _VirtualTime()
        release = threading.Event()
        closed: list[float] = []
        cleanup_deadline = 0.4
        spec = _spec(
            [("a", ["x"])],
            [_op("x")],
            cleanup_deadline=cleanup_deadline,
            runner_deadline=1.0,
        )
        deadline = 0.5

        def unwind() -> None:
            clock.park(release)

        try:
            with clock.installed():
                status = run_actions(
                    spec,
                    lambda _action: BackendResult(OK),
                    InMemorySink(),
                    _clock(),
                    runner_deadline=deadline,
                    unwind=unwind,
                    cleanup=lambda: closed.append(deadline - clock.now),
                )
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, "runner-deadline")
        )
        # The wedged unwind is abandoned after C/2, leaving the rest for close.
        self.assertAlmostEqual(deadline - cleanup_deadline / 2, closed[0])
        self.assertGreaterEqual(closed[0], cleanup_deadline / 2)

    def test_action_admission_obeys_actor_deadline(self) -> None:
        """A started actor cannot admit work after its budget.

        test_completions_cover_every_action_once is the timely accept twin.
        """
        spec = _spec([("a", ["first", "last"])], [_op("first"), _op("last")])
        now = [0.0]

        class Sink(InMemorySink):
            def emit(self, event: Any) -> None:
                """Advance past the actor deadline when its first action completes."""
                super().emit(event)
                if event.type == OK:
                    now[0] = 1.0

        sink = Sink()
        with mock.patch(
            "eden.integration.correctness.exploratory.core.scheduler.time.monotonic",
            side_effect=lambda: now[0],
        ):
            status = run_actions(
                spec,
                lambda _action: BackendResult(OK),
                sink,
                _clock(),
                runner_deadline=5.5,
            )
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, "runner-deadline")
        )
        self.assertEqual([e.data["action"] for e in sink.of_type(INVOKE)], ["first"])

    def test_setup_uses_the_supervisor_absolute_deadline(self) -> None:
        """Setup cannot reset the actor deadline or spend cleanup's reservation."""
        clock = _VirtualTime()
        cleanup_deadline = 0.2
        spec = _spec(
            [("a", ["x"])],
            [_op("x")],
            cleanup_deadline=cleanup_deadline,
            runner_deadline=1.0,
        )
        sink = InMemorySink()
        invoked: list[str] = []
        cleaned: list[float] = []
        # Setup already spent all but C of the supervisor's runner budget.
        deadline = clock.now + cleanup_deadline
        with clock.installed():
            status = run_actions(
                spec,
                lambda action: invoked.append(action.id) or BackendResult(OK),
                sink,
                _clock(),
                runner_deadline=deadline,
                cleanup=lambda: cleaned.append(deadline - clock.now),
            )
        self.assertEqual(invoked, [])
        self.assertEqual([e.data["action"] for e in sink.of_type(SKIPPED)], ["x"])
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, "runner-deadline")
        )
        # Nothing waited: cleanup keeps the whole reservation.
        self.assertEqual([cleanup_deadline], cleaned)

    def test_a_run_past_its_cleanup_reservation_keeps_containment_half(self) -> None:
        """Unwind and joins stop at D - C/2 even when the run reaches cleanup late.

        Setup left only C/4 of the runner budget, so D - C/2 is already past:
        a wedged unwind is abandoned at once and close keeps the remaining C/4
        instead of waiting a further C/2 past the deadline.
        """
        clock = _VirtualTime()
        release = threading.Event()
        cleanup_deadline = 0.4
        spec = _spec(
            [("a", ["x"])],
            [_op("x")],
            cleanup_deadline=cleanup_deadline,
            runner_deadline=1.0,
        )
        deadline = clock.now + cleanup_deadline / 4
        closed: list[float] = []
        try:
            with clock.installed():
                status = run_actions(
                    spec,
                    lambda _action: BackendResult(OK),
                    InMemorySink(),
                    _clock(),
                    runner_deadline=deadline,
                    unwind=lambda: clock.park(release),
                    cleanup=lambda: closed.append(deadline - clock.now),
                )
        finally:
            release.set()
        self.assertEqual(
            (status.status, status.reason), (HARNESS_FAILURE, "runner-deadline")
        )
        self.assertEqual([cleanup_deadline / 4], closed)

    def test_fault_unwind_precedes_join_and_close(self) -> None:
        """Parked calls return after unwind and before environment teardown."""
        release = threading.Event()
        returned = threading.Event()
        order: list[str] = []
        # The C/2 join bound is a hang guard for the released call.
        spec = _spec(
            [("a", ["x"])],
            [_op("x")],
            operation_deadline=0.05,
            cleanup_deadline=2 * _HANG_GUARD,
            runner_deadline=4 * _HANG_GUARD,
        )

        def backend(_action: Action) -> BackendResult:
            release.wait(_HANG_GUARD)
            order.append("returned")
            returned.set()
            return BackendResult(OK)

        def unwind() -> None:
            order.append("unwind")
            release.set()

        def cleanup() -> None:
            order.append("close")
            self.assertTrue(returned.is_set())

        try:
            status = run_actions(
                spec, backend, InMemorySink(), _clock(), unwind=unwind, cleanup=cleanup
            )
        finally:
            release.set()
        self.assertEqual(status.reason, OPERATION_TIMEOUT)
        self.assertEqual(order, ["unwind", "returned", "close"])

    def test_worker_join_uses_remaining_cleanup_budget(self) -> None:
        """A wedged call spends cleanup's shared budget, without an extra join."""
        clock = _VirtualTime()
        release = threading.Event()
        operation_deadline = 0.05
        cleanup_deadline = 0.1
        spec = _spec(
            [("a", ["x"])],
            [_op("x")],
            operation_deadline=operation_deadline,
            cleanup_deadline=cleanup_deadline,
            runner_deadline=0.8,
        )

        def backend(_action: Action) -> BackendResult:
            clock.park(release)
            return BackendResult(OK)

        cleaned: list[float] = []

        def cleanup() -> None:
            cleaned.append(clock.now)
            release.set()

        try:
            with clock.installed():
                status = run_actions(
                    spec,
                    backend,
                    InMemorySink(),
                    _clock(),
                    runner_deadline=clock.now + 0.8,
                    unwind=lambda: None,
                    cleanup=cleanup,
                )
        finally:
            release.set()
        self.assertEqual(status.reason, OPERATION_TIMEOUT)
        # The timeout plus one C/2 join: the unwind adds no join of its own.
        self.assertEqual(1, len(cleaned))
        self.assertAlmostEqual(operation_deadline + cleanup_deadline / 2, cleaned[0])


class _BreakingSink(InMemorySink):
    """Accept `good` events, then fail every later emit as a dead pipe would."""

    def __init__(self, good: int) -> None:
        super().__init__()
        self._good = good

    def emit(self, event: Any) -> None:
        if len(self.events) >= self._good:
            raise BrokenPipeError("supervisor gone")
        super().emit(event)


class SchedulerFailureContainmentTest(unittest.TestCase):
    def _teardown_calls(
        self,
        backend: Callable[[Action], BackendResult],
        sink: InMemorySink,
        unwind: Callable[[], None] = lambda: None,
    ) -> tuple[Any, list[str]]:
        calls: list[str] = []

        def recording_unwind() -> None:
            calls.append("unwind")
            unwind()

        spec = _spec([("a", ["x1", "x2"])], [_op("x1"), _op("x2")])
        with redirect_stderr(io.StringIO()):
            status = run_actions(
                spec,
                backend,
                sink,
                _clock(),
                unwind=recording_unwind,
                cleanup=lambda: calls.append("cleanup"),
            )
        return status, calls

    def test_broken_sink_stops_the_run_and_still_tears_down(self) -> None:
        status, calls = self._teardown_calls(
            lambda _action: BackendResult(OK), _BreakingSink(good=1)
        )
        self.assertEqual(
            (HARNESS_FAILURE, scheduler_module.BROKEN_HISTORY_TRANSPORT),
            (status.status, status.reason),
        )
        self.assertEqual(["unwind", "cleanup"], calls)

    def test_facts_cannot_forge_scheduler_keys_or_break_the_sink(self) -> None:
        for facts in ({"action": "x2"}, {"actor": "b"}, {"unencodable": object()}):
            with self.subTest(facts=sorted(facts)):
                sink = InMemorySink()
                status, calls = self._teardown_calls(
                    lambda _action, facts=facts: BackendResult(OK, facts), sink
                )
                self.assertEqual(
                    (HARNESS_FAILURE, INVALID_BACKEND_RESPONSE),
                    (status.status, status.reason),
                )
                self.assertEqual(
                    [(INVOKE, "x1"), (INFO, "x1"), (SKIPPED, "x2")],
                    [(e.type, e.data["action"]) for e in sink.events],
                )
                self.assertEqual(["unwind", "cleanup"], calls)

    def test_raising_unwind_is_a_harness_failure_not_an_exception(self) -> None:
        def unwind() -> None:
            raise OSError("fault table unreadable")

        status, calls = self._teardown_calls(
            lambda _action: BackendResult(OK), InMemorySink(), unwind
        )
        self.assertEqual(
            (
                HARNESS_FAILURE,
                INVALID_BACKEND_RESPONSE,
                "OSError: fault table unreadable",
            ),
            (status.status, status.reason, status.error),
        )
        self.assertEqual(["unwind", "cleanup"], calls)
