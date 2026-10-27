# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The differential backend: shared actions dispatched to both sides and Eden-only
actions to the primary, bounded dispatch with typed failures before the watchdog,
backend selection per run, each side's facts decoding against the kind's schema, and
when an environment may opt into differential runs.
"""

from __future__ import annotations

import errno
import json
import os
import random
import tempfile
import threading
import time
import unittest
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from typing import Any, cast
from unittest import mock

from eden.integration.correctness.exploratory.core.checker import content_digest
from eden.integration.correctness.exploratory.core.differential import REFERENCE_RESULT
from eden.integration.correctness.exploratory.core.history import (
    FAIL,
    INFO,
    InMemorySink,
    OK,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Environment as SpecEnvironment,
    from_json,
    RunSpec,
    SpecError,
    to_json,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    COMPLETED,
    HarnessFailure,
    INVALID_BACKEND_RESPONSE,
    run_actions,
)
from eden.integration.correctness.exploratory.core.target import (
    ProcessRegistry,
    RUN_DIR_ENV,
)
from eden.integration.correctness.exploratory.edenfs import (
    environment as edenfs_environment,
    target as edenfs_target,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    CASE_INSENSITIVE_MOUNT,
    expects_reference,
    ReferenceNotApplicable,
)
from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend
from eden.integration.correctness.exploratory.edenfs.differential_backend import (
    DifferentialBackend,
)
from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_value,
    initial_content,
)
from eden.integration.correctness.exploratory.edenfs.reference import (
    ReferenceBackend,
    ReferenceCheckout,
    ReferenceFailure,
    ReferenceRuntime,
)
from eden.integration.correctness.exploratory.edenfs.sapling_processes import (
    kill_wait_for,
    LEFT_RUNNING,
    REFERENCE_TIMEOUT,
)
from eden.integration.correctness.exploratory.edenfs.target import (
    RECORDING_TARGET,
    with_reference_clone,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _FakeEnv,
    decode_problems,
    facts_of,
)
from eden.integration.correctness.exploratory.edenfs.test.reference_fixtures import (
    FAKE_SL,
    HANG_GUARD,
    reference_spec,
    REGISTER,
    SEED,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import make_action


def _with_operation_deadline(spec: RunSpec, seconds: float) -> RunSpec:
    return replace(
        spec,
        budgets=replace(spec.budgets, operation_deadline_seconds=seconds),
    )


class _Primary:
    """Primary backend double recording calls and cleanup."""

    def __init__(self, results: dict[str, BackendResult] | None = None) -> None:
        self.results = results or {}
        self.calls: list[str] = []
        self.hooks: dict[str, Any] = {}
        self.closed: list[float | None] = []

    def __call__(self, action: Action) -> BackendResult:
        self.calls.append(action.id)
        hook = self.hooks.get(action.id)
        if hook is not None:
            hook()
        return self.results.get(action.id, BackendResult(OK, {}))

    def unwind_faults(self, _sink: Any, _clock: Any) -> dict[str, Any]:
        return {"unwound": 0, "misses": 0, "leaks": 0, "setup": {"run_dir": "r"}}

    def close_resources(self, deadline: float | None = None) -> int:
        self.closed.append(deadline)
        return 0


class _Reference(_Primary):
    """Reference backend double; a hook may raise to model a reference failure."""


def _wrap(primary: Any, reference: Any, spec: RunSpec) -> DifferentialBackend:
    return DifferentialBackend(primary, cast(Any, reference), spec)


def _terminals(sink: InMemorySink, action_id: str) -> list[Any]:
    return [
        e
        for e in sink.events
        if e.data.get("action") == action_id and e.type in (OK, FAIL, INFO)
    ]


class DifferentialDispatchTest(unittest.TestCase):
    def test_shared_action_starts_both_sides_from_one_invocation(self) -> None:
        """Each side waits for the other, so a serial dispatch would deadlock."""
        spec = reference_spec(
            [
                make_action("w1", "write", path=REGISTER, value_id="v1"),
                make_action("q", "read", path=REGISTER, channel="fuse"),
            ]
        )
        primary, reference = _Primary(), _Reference()
        both: dict[str, threading.Event] = {
            side: threading.Event() for side in ("primary", "reference")
        }

        def meet(mine: str, other: str) -> Any:
            def hook() -> None:
                both[mine].set()
                if not both[other].wait(timeout=HANG_GUARD):
                    raise AssertionError(f"{other} never started beside {mine}")

            return hook

        primary.hooks["q"] = meet("primary", "reference")
        reference.hooks["q"] = meet("reference", "primary")
        sink = InMemorySink()
        status = run_actions(
            spec, _wrap(primary, reference, spec), sink, time.monotonic
        )

        self.assertEqual(COMPLETED, status.status)
        self.assertEqual(["w1", "q"], primary.calls)
        self.assertEqual(["w1", "q"], reference.calls)
        for action_id in ("w1", "q"):
            (terminal,) = _terminals(sink, action_id)
            self.assertIn(REFERENCE_RESULT, terminal.data)
        facts = facts_of(spec, sink.events)
        for action_id in ("w1", "q"):
            recorded = facts.action(action_id)
            side = recorded.reference
            assert side is not None and side.interval is not None
            assert recorded.interval is not None
            self.assertLessEqual(recorded.interval[0], side.interval[0])
            self.assertLessEqual(side.interval[1], recorded.interval[1])

    def test_eden_only_actions_never_reach_the_reference(self) -> None:
        actions = [
            make_action(
                "f1",
                "fault_install",
                fault_id="F",
                target_action="w1",
                site="write",
                behavior="block",
            ),
            make_action("h1", "fault_await", fault_id="F"),
            make_action("x1", "fault_remove", fault_id="F"),
            make_action("k1", "kill_process", target_action="s2", after_ms=0),
            make_action("u1", "unload_inodes", path="tree"),
            make_action("b1", "background_read", root="tree", seconds=0.1),
            make_action(
                "t1", "read", path=REGISTER, channel="thrift", sync_timeout_seconds=0
            ),
            make_action("d1", "readdir", path="tree", channel="thrift"),
            make_action("d0", "readdir", path="", channel="fuse"),
            make_action("s2", "status", channel="thrift"),
            make_action("c1", "checkout", revision=1, via="thrift", mode="normal"),
            make_action("z1", "sleep", seconds=0),
            make_action("j1", "journal_window"),
        ]
        spec = reference_spec(actions)
        primary, reference = _Primary(), _Reference()
        backend = _wrap(primary, reference, spec)
        for action in actions:
            result = backend(action)
            self.assertNotIn(REFERENCE_RESULT, result.facts)
        self.assertEqual([a.id for a in actions], primary.calls)
        self.assertEqual([], reference.calls)

    def test_non_differential_run_uses_the_primary_alone(self) -> None:
        action = make_action("q", "read", path=REGISTER, channel="fuse")
        spec = reference_spec([action], reference=False)
        primary, reference = _Primary(), _Reference()
        self.assertEqual({}, _wrap(primary, reference, spec)(action).facts)
        self.assertEqual([], reference.calls)

    def test_reference_failure_is_a_reference_fact_and_stops_pairing(self) -> None:
        """The primary keeps its result; the reference side is unresolved from then on."""
        spec = reference_spec(
            [
                make_action("q", "read", path=REGISTER, channel="fuse"),
                make_action("q2", "read", path=REGISTER, channel="fuse"),
            ]
        )
        digest = content_digest(b"eden")
        primary = _Primary(
            {"q": BackendResult(OK, digest), "q2": BackendResult(OK, digest)}
        )
        reference = _Reference()

        def straggle() -> None:
            raise ReferenceFailure(LEFT_RUNNING, "reference/q/status left its group")

        reference.hooks["q"] = straggle
        backend = _wrap(primary, reference, spec)
        sink = InMemorySink()
        status = run_actions(spec, backend, sink, time.monotonic)

        self.assertEqual(COMPLETED, status.status)
        self.assertEqual(["q", "q2"], primary.calls)
        self.assertEqual(["q"], reference.calls)
        reasons = {}
        for action_id in ("q", "q2"):
            (terminal,) = _terminals(sink, action_id)
            self.assertEqual(OK, terminal.type)
            side = terminal.data[REFERENCE_RESULT]
            self.assertEqual((INFO, ["reason"]), (side["outcome"], list(side["facts"])))
            reasons[action_id] = side["facts"]["reason"]
        self.assertEqual(
            {
                "q": "left-running: reference/q/status left its group",
                "q2": "reference-diverged: q left-running: "
                "reference/q/status left its group",
            },
            reasons,
        )
        facts = facts_of(spec, sink.events)
        self.assertEqual({}, decode_problems(facts))
        for action_id in ("q", "q2"):
            recorded = facts.action(action_id)
            assert recorded.reference is not None and recorded.result is not None
            self.assertEqual(OK, recorded.outcome)
            self.assertEqual(digest["sha256"], recorded.result.get("sha256"))
            self.assertEqual(
                ("reference", INFO),
                (recorded.reference.side, recorded.reference.outcome),
            )
        failures = [
            {
                "action": "q",
                "kind": LEFT_RUNNING,
                "detail": "reference/q/status left its group",
                "primary": {"outcome": OK, "facts": digest},
            }
        ]
        self.assertEqual({"paired": 0, "failures": failures}, backend.runtime_facts())
        unwound = backend.unwind_faults(sink, time.monotonic)
        self.assertEqual(failures, unwound["setup"]["reference"]["runtime"]["failures"])
        self.assertEqual("r", unwound["setup"]["run_dir"])

    def test_a_raising_primary_keeps_the_reference_failure_beside_it(self) -> None:
        spec = reference_spec([make_action("q", "read", path=REGISTER, channel="fuse")])
        primary, reference = _Primary(), _Reference()

        def primary_breaks() -> None:
            raise OSError(errno.EIO, "primary broke")

        def reference_breaks() -> None:
            raise ReferenceFailure(LEFT_RUNNING, "reference/q left its group")

        primary.hooks["q"] = primary_breaks
        reference.hooks["q"] = reference_breaks
        backend = _wrap(primary, reference, spec)
        with self.assertRaises(OSError):
            backend(spec.action("q"))
        self.assertEqual(
            [
                {
                    "action": "q",
                    "kind": LEFT_RUNNING,
                    "detail": "reference/q left its group",
                    "primary": {"error": "OSError: [Errno 5] primary broke"},
                }
            ],
            backend.runtime_facts()["failures"],
        )

    def test_unexpected_reference_errors_and_malformed_sides_are_unresolved(
        self,
    ) -> None:
        action = make_action("q", "read", path=REGISTER, channel="fuse")
        spec = reference_spec([action])
        cases = (
            ("exception", RuntimeError("boom"), None),
            ("malformed", None, "not a result"),
            ("malformed", None, BackendResult(OK, {"action": "x"})),
        )
        for kind, error, reference_result in cases:
            with self.subTest(kind=kind, reference=reference_result):
                primary = _Primary({"q": BackendResult(OK, {"length": 1})})
                reference = _Reference()
                if error is not None:

                    def fail(error: Exception = error) -> None:
                        raise error

                    reference.hooks["q"] = fail
                else:
                    reference.results["q"] = cast(Any, reference_result)
                backend = _wrap(primary, reference, spec)
                result = backend(action)
                self.assertEqual((OK, 1), (result.outcome, result.facts["length"]))
                side = result.facts[REFERENCE_RESULT]
                self.assertEqual(INFO, side["outcome"])
                self.assertTrue(side["facts"]["reason"].startswith(f"{kind}: "))
                self.assertEqual(kind, backend.runtime_facts()["failures"][0]["kind"])
                self.assertIsNotNone(backend.diverged)

    def test_a_malformed_primary_is_a_harness_failure(self) -> None:
        action = make_action("q", "read", path=REGISTER, channel="fuse")
        spec = reference_spec([action])
        for primary_result in (
            BackendResult(OK, {REFERENCE_RESULT: {}}),
            BackendResult(OK, {"action": "x"}),
            "not a result",
        ):
            with self.subTest(primary=primary_result):
                backend = _wrap(
                    _Primary({"q": cast(Any, primary_result)}), _Reference(), spec
                )
                with self.assertRaises(HarnessFailure) as caught:
                    backend(action)
                self.assertEqual(INVALID_BACKEND_RESPONSE, caught.exception.reason)
                self.assertEqual([], backend.runtime_facts()["failures"])

    def test_wrapper_composes_a_live_backend_subclass_without_changing_it(self) -> None:
        """A lifecycle-style subclass keeps its own unwind and close behavior."""

        class _Subclass(LiveBackend):
            def unwind_faults(self, sink: Any, clock: Any) -> dict[str, Any]:
                result = super().unwind_faults(sink, clock)
                result["subclass"] = True
                return result

            def close_resources(self, deadline: float | None = None) -> int:
                self.closed_at = deadline
                return 1

        spec = reference_spec([make_action("q", "read", path=REGISTER, channel="fuse")])
        env = SimpleNamespace(
            mount_path=Path("/nonexistent"),
            processes=ProcessRegistry(),
            setup_facts={"reference": {"node": "n"}},
        )
        primary = _Subclass(cast(Any, env), spec)
        reference = _Reference()
        backend = _wrap(primary, reference, spec)
        unwound = backend.unwind_faults(InMemorySink(), time.monotonic)
        self.assertTrue(unwound["subclass"])
        self.assertEqual(
            {"node": "n", "runtime": {"paired": 0, "failures": []}},
            unwound["setup"]["reference"],
        )
        self.assertEqual(1, backend.close_resources(12.5))
        self.assertEqual(12.5, primary.closed_at)
        self.assertEqual([12.5], reference.closed)
        self.assertEqual(1, backend.unwind(InMemorySink(), time.monotonic)["leaks"])


class BoundedDispatchTest(unittest.TestCase):
    def test_stuck_reference_side_is_a_typed_failure_before_the_watchdog(self) -> None:
        """A reference that never returns is a reference-timeout, not an operation-timeout.

        The paired call gives up on the reference once its join bound
        passes, and that bound is three quarters of the operation deadline
        the scheduler watchdog enforces, so the primary's result lands first.
        """
        operation = 2.0
        spec = _with_operation_deadline(
            reference_spec([make_action("q", "read", path=REGISTER, channel="fuse")]),
            operation,
        )
        release = threading.Event()
        self.addCleanup(release.set)
        reference = _Reference()
        reference.hooks["q"] = lambda: release.wait(timeout=HANG_GUARD)
        backend = _wrap(_Primary(), reference, spec)
        result = backend(spec.action("q"))
        side = result.facts[REFERENCE_RESULT]
        self.assertEqual(INFO, side["outcome"])
        self.assertEqual(
            "reference-timeout: reference side still running after 1.5s",
            side["facts"]["reason"],
        )
        ((failure_kind, detail),) = [
            (f["kind"], f["detail"]) for f in backend.runtime_facts()["failures"]
        ]
        self.assertEqual(REFERENCE_TIMEOUT, failure_kind)
        # Three quarters of the two-second operation deadline.
        self.assertEqual("reference side still running after 1.5s", detail)

    def test_two_actors_dispatch_shared_actions_concurrently(self) -> None:
        """Both actors' reference sides run at once: each waits for the other to start."""
        spec = reference_spec(
            [
                make_action("qa", "read", path=REGISTER, channel="fuse"),
                make_action("qb", "getattr", path=REGISTER),
            ],
            [("a", ["qa"]), ("b", ["qb"])],
        )
        reference = _Reference()
        running = {name: threading.Event() for name in ("qa", "qb")}

        def overlap(mine: str, other: str) -> Any:
            def hook() -> None:
                running[mine].set()
                if not running[other].wait(timeout=HANG_GUARD):
                    raise AssertionError(f"{other} never ran beside {mine}")

            return hook

        reference.hooks = {"qa": overlap("qa", "qb"), "qb": overlap("qb", "qa")}
        primary = _Primary()
        sink = InMemorySink()
        status = run_actions(
            spec, _wrap(primary, reference, spec), sink, time.monotonic
        )
        self.assertEqual(COMPLETED, status.status)
        self.assertEqual({"qa", "qb"}, set(reference.calls))
        facts = facts_of(spec, sink.events)
        qa, qb = facts.action("qa").reference, facts.action("qb").reference
        assert qa is not None and qa.interval is not None
        assert qb is not None and qb.interval is not None
        self.assertLess(
            max(qa.interval[0], qb.interval[0]), min(qa.interval[1], qb.interval[1])
        )

    def test_failures_after_unwind_reach_the_published_record(self) -> None:
        """The record unwind published is refreshed once workers have joined."""
        spec = reference_spec([make_action("q", "read", path=REGISTER, channel="fuse")])
        runtime = ReferenceRuntime()
        backend = DifferentialBackend(
            _Primary(), cast(Any, _Reference()), spec, runtime=runtime
        )
        unwound = backend.unwind_faults(InMemorySink(), time.monotonic)
        published = unwound["setup"]["reference"]["runtime"]
        self.assertEqual({"paired": 0, "failures": []}, published)
        runtime.failed("q", REFERENCE_TIMEOUT, "late")
        backend.close_resources(time.monotonic() + HANG_GUARD)
        self.assertEqual(
            [{"action": "q", "kind": REFERENCE_TIMEOUT, "detail": "late"}],
            published["failures"],
        )

    def test_real_reference_overrun_is_killed_and_typed_through_the_scheduler(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sl = Path(tmp) / "sl"
            sl.write_text(
                FAKE_SL.replace("status)", "status) exec sleep 3600 ;;\nunused)")
            )
            sl.chmod(0o755)
            root = Path(tmp) / "reference"
            root.mkdir()
            checkout = ReferenceCheckout(root, str(sl), dict(os.environ), ("n0",))
            # The paired join (0.75 of the operation deadline) and the
            # watchdog are hang guards for a 0.3 s process bound.
            spec = reference_spec([make_action("s1", "status", channel="sl")])
            reference_backend = ReferenceBackend(checkout, spec)
            reference_backend._leader_seconds = 0.3
            backend = DifferentialBackend(
                _Primary({"s1": BackendResult(OK, {"status": [], "diff_paths": []})}),
                reference_backend,
                spec,
                runtime=checkout.runtime,
            )
            sink = InMemorySink()
            status = run_actions(spec, backend, sink, time.monotonic)
            self.assertEqual(COMPLETED, status.status)
            (terminal,) = _terminals(sink, "s1")
            self.assertEqual(INFO, terminal.data[REFERENCE_RESULT]["outcome"])
            self.assertEqual({}, decode_problems(facts_of(spec, sink.events)))
            self.assertEqual(
                [REFERENCE_TIMEOUT],
                [f["kind"] for f in checkout.runtime.snapshot()["failures"]],
            )
            self.assertEqual(0, backend.close_resources(time.monotonic() + HANG_GUARD))
            self.assertEqual([], checkout.leftovers())
        # Unpatched, a side's worst case (run, then die) ends before the
        # paired join at three quarters of the operation deadline.
        for operation in (0.5, 2.0, 30.0, 300.0):
            worst = operation / 2 + kill_wait_for(operation)
            self.assertLess(worst, 0.75 * operation, operation)


class TargetBackendSelectionTest(unittest.TestCase):
    def test_non_differential_runs_keep_the_plain_live_backend(self) -> None:
        spec = reference_spec([], reference=False)
        backend = edenfs_target._make_backend(cast(Any, _FakeEnv({})), spec, None)
        self.assertIs(LiveBackend, type(backend))

    def test_differential_runs_wrap_the_live_backend(self) -> None:
        spec = reference_spec([])
        env = _FakeEnv({})
        checkout = ReferenceCheckout(Path("/nonexistent"), "/bin/false", {}, ())
        cast(Any, env).reference = checkout
        backend = edenfs_target._make_backend(
            cast(Any, env), spec, "common-status-loss"
        )
        self.assertIsInstance(backend, DifferentialBackend)
        assert isinstance(backend, DifferentialBackend)
        self.assertIs(LiveBackend, type(backend.primary))
        self.assertIs(checkout, backend.reference.checkout)

    def test_case_insensitive_differential_run_is_refused_before_setup(self) -> None:
        sink = InMemorySink()
        with (
            tempfile.TemporaryDirectory() as run_dir,
            mock.patch.dict(os.environ, {RUN_DIR_ENV: run_dir}),
            mock.patch.object(edenfs_environment, "Environment") as environment,
        ):
            refused = reference_spec([], case_sensitive=False)
            with self.assertRaises(ReferenceNotApplicable) as caught:
                edenfs_target._make_environment(refused, sink)
            self.assertEqual(CASE_INSENSITIVE_MOUNT, caught.exception.reason)
            environment.assert_not_called()

            plain = reference_spec([], case_sensitive=False, reference=False)
            edenfs_target._make_environment(plain, sink)
            environment.assert_called_once()
            self.assertFalse(environment.call_args.kwargs["reference_clone"])
            self.assertFalse(environment.call_args.kwargs["case_sensitive"])


class ReferenceFactsTest(unittest.TestCase):
    """Histories the wrapper writes decode into paired reference-side facts."""

    def _run(
        self, spec: RunSpec, primary: _Primary, reference: _Reference
    ) -> InMemorySink:
        sink = InMemorySink()
        status = run_actions(
            spec, _wrap(primary, reference, spec), sink, time.monotonic
        )
        self.assertEqual(COMPLETED, status.status)
        return sink

    def test_each_side_decodes_against_the_kinds_schema(self) -> None:
        spec = reference_spec(
            [
                make_action("w1", "write", path=REGISTER, value_id="v1"),
                make_action("q", "read", path=REGISTER, channel="fuse"),
            ]
        )
        written = content_digest(encode_value(SEED, "a", "v1"))
        failed = {"errno": errno.EIO, "reason": "read: x"}
        sink = self._run(
            spec,
            _Primary(
                {
                    "w1": BackendResult(OK, {"bytes_written": 64}),
                    "q": BackendResult(OK, written),
                }
            ),
            _Reference(
                {
                    "w1": BackendResult(OK, {"bytes_written": 64}),
                    "q": BackendResult(FAIL, failed),
                }
            ),
        )
        facts = facts_of(spec, sink.events)
        self.assertEqual({}, decode_problems(facts))
        read = facts.action("q")
        reference = read.reference
        assert reference is not None and reference.result is not None
        assert read.interval is not None and reference.interval is not None
        self.assertEqual(("primary", OK), (read.side, read.outcome))
        self.assertEqual(("reference", FAIL), (reference.side, reference.outcome))
        self.assertEqual(errno.EIO, reference.result.errno)
        self.assertLessEqual(read.interval[0], reference.interval[0])
        self.assertLessEqual(reference.interval[1], read.interval[1])
        self.assertNotIn(
            REFERENCE_RESULT, dict(read.result.fields) if read.result else {}
        )

    def test_a_reference_side_off_its_schema_is_a_reference_problem(self) -> None:
        spec = reference_spec([make_action("q", "read", path=REGISTER, channel="fuse")])
        digest = content_digest(initial_content(SEED, REGISTER))
        sink = self._run(
            spec,
            _Primary({"q": BackendResult(OK, digest)}),
            _Reference({"q": BackendResult(OK, {**digest, "extra": 1})}),
        )
        facts = facts_of(spec, sink.events)
        problems = decode_problems(facts)
        self.assertEqual(["q/reference"], sorted(problems))
        self.assertIn("undeclared fields ['extra']", problems["q/reference"][0])
        self.assertEqual((), facts.action("q").problems)


class DifferentialOverrideTest(unittest.TestCase):
    def test_default_sampler_never_opts_in_and_override_is_explicit(self) -> None:
        first = edenfs_target.sample_environment(random.Random(7))
        again = edenfs_target.sample_environment(random.Random(7))
        self.assertEqual(
            json.dumps(first, sort_keys=True), json.dumps(again, sort_keys=True)
        )
        self.assertNotIn("reference_clone", first["setup_profile"])
        sensitive: dict[str, Any] = {**first, "case_sensitive": True}
        opted = with_reference_clone(sensitive)
        self.assertTrue(opted["setup_profile"]["reference_clone"])
        self.assertNotIn("reference_clone", dict(sensitive["setup_profile"]))
        spec = reference_spec([make_action("q", "read", path=REGISTER, channel="fuse")])
        target = {
            **dict(spec.environment.target),
            "setup_profile": {"materialize": False, "scatter_dirty": 0},
        }
        opted_spec = replace(
            spec, environment=SpecEnvironment(target=with_reference_clone(target))
        )
        parsed = from_json(to_json(opted_spec), RECORDING_TARGET)
        self.assertTrue(expects_reference(parsed, parsed.action("q")))

    def test_case_insensitive_environments_cannot_opt_in(self) -> None:
        insensitive = {
            **edenfs_target.sample_environment(random.Random(7)),
            "case_sensitive": False,
        }
        with self.assertRaises(ReferenceNotApplicable) as caught:
            with_reference_clone(insensitive)
        self.assertEqual(CASE_INSENSITIVE_MOUNT, caught.exception.reason)
        spec = reference_spec(
            [make_action("q", "read", path=REGISTER, channel="fuse")],
            case_sensitive=False,
        )
        with self.assertRaisesRegex(SpecError, CASE_INSENSITIVE_MOUNT):
            from_json(to_json(spec), RECORDING_TARGET)
