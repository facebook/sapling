# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Validator tests for the fault catalog: site rules, heal-less specs, and stubbed daemons."""

import os
import tempfile
import time
import unittest
from contextlib import contextmanager
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from typing import Any
from unittest import mock

from eden.fs.service.eden.thrift_types import DIS_NOT_RECURSIVE, SyncBehavior
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP_REMOVED,
    Event,
    FAIL,
    OK,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEADLINE_OPERATION,
    DEADLINE_RUN,
    Environment as SpecEnvironment,
    FINISHED,
    RunSpec,
    SpecError,
    STARTED,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    FAULT_WITNESS_FAILED,
    HarnessFailure,
    INVALID_BACKEND_RESPONSE,
)
from eden.integration.correctness.exploratory.edenfs import faults as faults_module
from eden.integration.correctness.exploratory.edenfs.faults import FaultTracker
from eden.integration.correctness.exploratory.edenfs.sites import (
    CHECKOUT_BEGIN_SITE,
    FETCH_TREE_SITE,
)
from eden.integration.correctness.exploratory.test.fault_fixtures import (
    _await,
    _behavior_spec,
    _checkout,
    _checkpoint_spec,
    _install,
    _install_cache,
    _install_wfpw,
    _owned_sl_children,
    _parked_probe,
    _parked_sync_read,
    _probe,
    _spec,
    _spec_env,
    _sync_read,
    _wfpw_spec,
    REGISTERS,
    validate,
)


def _fetch_tree_spec(channel: str = "thrift", revision: int | None = None) -> RunSpec:
    """Return one current-parent status parked at the root-tree checkpoint."""
    payload: dict[str, Any] = {"channel": channel}
    if revision is not None:
        payload["revision"] = revision
    actions = [
        Action(
            id="f1",
            kind="fault_install",
            deadline=DEADLINE_OPERATION,
            payload={
                "fault_id": "F",
                "target_action": "s1",
                "site": FETCH_TREE_SITE,
                "behavior": "block",
            },
        ),
        Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_RUN,
            requires=(("f1", FINISHED),),
            payload=payload,
        ),
        _await("h1", "F", ("s1", STARTED)),
        Action(
            id="x1",
            kind="fault_remove",
            deadline=DEADLINE_OPERATION,
            requires=(("h1", FINISHED),),
            payload={"fault_id": "F"},
        ),
    ]
    return _spec(
        REGISTERS[0],
        actions,
        [
            ("installer", ["f1"]),
            ("status", ["s1"]),
            ("fault", ["h1", "x1"]),
        ],
    )


def _install_lookup(target: str, *requires: tuple[str, str]) -> Action:
    """Return a fuse-lookup install of fault F for a target checkout/read."""
    return Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        requires=tuple(requires),
        payload={
            "fault_id": "F",
            "target_action": target,
            "site": "fuse-lookup",
            "behavior": "block",
        },
    )


def _parked_lookup_read(register: str) -> Action:
    """Return the run-deadline t1 target a lookup install parks."""
    return Action(
        id="t1",
        kind="read",
        deadline=DEADLINE_RUN,
        requires=(("f1", FINISHED),),
        payload={"path": register, "channel": "fuse"},
    )


class _ListSink:
    """History sink collecting emitted events in a list."""

    def __init__(self, events: list[Event]) -> None:
        self._events = events

    def emit(self, event: Event) -> None:
        self._events.append(event)


class _StubBlocked:
    """Stand-in for a getBlockedFaults response."""

    __slots__ = ("keyValues",)

    def __init__(self, keys: list[str]) -> None:
        self.keyValues = keys


class _StubClient:
    """Stand-in thrift client recording fault injections."""

    def __init__(self) -> None:
        self.injected: list[Any] = []
        self.keys: list[str] = []
        self.blocked_requests: list[Any] = []
        self.removed: list[Any] = []
        self.unblocked: list[Any] = []
        self.clear_on_unblock = False
        self.remove_error: Exception | None = None

    def removeFault(self, arg: Any) -> bool:
        self.removed.append(arg)
        if self.remove_error is not None:
            raise self.remove_error
        return False

    def unblockFault(self, arg: Any) -> None:
        self.unblocked.append(arg)
        if self.clear_on_unblock:
            self.keys.clear()

    def injectFault(self, fault: Any) -> None:
        self.injected.append(fault)

    def getBlockedFaults(self, request: Any) -> _StubBlocked:
        self.blocked_requests.append(request)
        return _StubBlocked(list(self.keys))


class _StubEnv:
    """Stand-in Environment exposing a stub thrift client."""

    def __init__(self, client: _StubClient, rpc_parent: str | None = None) -> None:
        self.mount_path = "/fake/mount"
        parent = rpc_parent if rpc_parent is not None else f"{4:040x}"
        self.mount_repo = type("MountRepo", (), {"hg": lambda _self, *_args: parent})()
        self.commits = tuple(
            type("Commit", (), {"node": f"{index + 1:040x}"})() for index in range(4)
        )
        self._client = client

    @contextmanager
    def get_thrift_client(self, timeout: Any = None) -> Any:
        del timeout
        yield self._client


class FaultValidatorTest(unittest.TestCase):
    def test_inode_site_resolves_without_loading_target(self) -> None:
        """Fault binding observes an unloaded entry without issuing a stat."""
        spec = _checkpoint_spec("inodemap-load", "block")
        client = _StubClient()
        entry = SimpleNamespace(name=b"r0.reg", inodeNumber=42, loaded=False)
        info = SimpleNamespace(entries=[entry])
        with (
            mock.patch.object(
                client, "debugInodeStatus", create=True, return_value=[info]
            ) as query,
            mock.patch.object(
                Path, "stat", side_effect=AssertionError("binding loaded target")
            ),
        ):
            tracker = FaultTracker(_StubEnv(client), spec)
            tracker.install(spec.action("f1"))
        query.assert_called_once_with(
            b"/fake/mount",
            b"tree/registers",
            DIS_NOT_RECURSIVE,
            SyncBehavior(syncTimeoutSeconds=0),
        )
        self.assertEqual(client.injected[0].keyClass, "inodemap-load")
        self.assertEqual(client.injected[0].keyValueRegex, "42")
        client.keys = ["42"]
        self.assertEqual(tracker.wait_for_hit(spec.action("h1")).facts["witness"], "42")

    def test_inode_site_rejects_unassigned_key_before_injection(self) -> None:
        """A missing inode identity cannot arm a wildcard or zero-key fault."""
        spec = _checkpoint_spec("inodemap-load", "block")
        for entries in ([], [SimpleNamespace(name=b"r0.reg", inodeNumber=0)]):
            with self.subTest(entries=entries):
                client = _StubClient()
                with mock.patch.object(
                    client,
                    "debugInodeStatus",
                    create=True,
                    return_value=[SimpleNamespace(entries=entries)],
                ):
                    tracker = FaultTracker(_StubEnv(client), spec)
                    with self.assertRaises(HarnessFailure):
                        tracker.install(spec.action("f1"))
                self.assertEqual(client.injected, [])
                self.assertEqual(client.removed, [])

    def test_fetch_tree_install_and_named_witness_are_exact(self) -> None:
        spec = _fetch_tree_spec(revision=3)
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)

        result = tracker.install(spec.action("f1"))

        self.assertEqual(result.outcome, OK)
        (fault,) = client.injected
        self.assertEqual(fault.keyClass, "SaplingBackingStore::getRootTree")
        self.assertEqual(fault.keyValueRegex, ".*")
        self.assertEqual(fault.count, 1)
        self.assertTrue(fault.block)
        expected = f"{4:040x}"
        client.keys = ["f" * 40, expected]
        hit = tracker.wait_for_hit(spec.action("h1"))
        self.assertEqual(
            hit.facts,
            {"fault_id": "F", "hit": True, "witness": expected},
        )
        self.assertEqual(
            client.blocked_requests[-1].keyclass,
            "SaplingBackingStore::getRootTree",
        )

    def test_fetch_tree_native_comparison_revision_uses_rpc_parent(self) -> None:
        spec = _fetch_tree_spec(channel="sl", revision=2)
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        tracker.install(spec.action("f1"))
        rpc_parent = f"{4:040x}"
        comparison_revision = f"{3:040x}"
        client.keys = [comparison_revision, rpc_parent]

        result = tracker.wait_for_hit(spec.action("h1"))

        self.assertEqual(result.facts.get("witness"), rpc_parent)

    def test_fetch_tree_wrong_witness_times_out(self) -> None:
        spec = _fetch_tree_spec()
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        tracker.install(spec.action("f1"))
        client.keys = ["f" * 40]
        with mock.patch.object(faults_module, "HIT_TIMEOUT_SECONDS", 0):
            with self.assertRaises(HarnessFailure) as context:
                tracker.wait_for_hit(spec.action("h1"))
        self.assertEqual(context.exception.reason, FAULT_WITNESS_FAILED)

    def test_fetch_tree_missing_expected_witness_rejects_before_mutation(self) -> None:
        spec = _fetch_tree_spec(channel="sl", revision=2)
        client = _StubClient()
        env = _StubEnv(client, rpc_parent="")
        with self.assertRaises(HarnessFailure) as context:
            FaultTracker(env, spec).install(spec.action("f1"))
        self.assertEqual(context.exception.reason, INVALID_BACKEND_RESPONSE)
        self.assertEqual(
            (client.removed, client.unblocked, client.injected), ([], [], [])
        )

    def test_fetch_tree_remove_unblocks_and_drains_exact_class(self) -> None:
        spec = _fetch_tree_spec()
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        tracker.install(spec.action("f1"))
        client.keys = [f"{4:040x}"]
        tracker.wait_for_hit(spec.action("h1"))
        client.clear_on_unblock = True

        result = tracker.remove(spec.action("x1"))

        self.assertEqual(result.outcome, OK)
        self.assertEqual(
            client.removed[-1].keyClass, "SaplingBackingStore::getRootTree"
        )
        self.assertEqual(client.removed[-1].keyValueRegex, ".*")
        self.assertEqual(client.unblocked[-1].keyClass, client.removed[-1].keyClass)
        self.assertEqual(client.keys, [])

    def test_fetch_tree_rejects_bad_binding_before_mutation(self) -> None:
        bad_behavior = _fetch_tree_spec()
        bad_behavior = replace(
            bad_behavior,
            actions=tuple(
                replace(action, payload={**action.payload, "behavior": "error"})
                if action.id == "f1"
                else action
                for action in bad_behavior.actions
            ),
        )
        bad_specs = [
            _fetch_tree_spec(channel="thrift", revision=2),
            replace(
                _fetch_tree_spec(),
                environment=SpecEnvironment(
                    target={**_spec_env().target, "fixture_source": "overlay"}
                ),
            ),
            bad_behavior,
            replace(
                _fetch_tree_spec(),
                actions=tuple(
                    replace(action, payload={**action.payload, "ordinal": 2})
                    if action.id == "f1"
                    else action
                    for action in _fetch_tree_spec().actions
                ),
            ),
        ]
        for spec in bad_specs:
            with self.subTest(spec=spec):
                client = _StubClient()
                tracker = FaultTracker(_StubEnv(client), spec)
                with self.assertRaises(HarnessFailure) as context:
                    tracker.install(spec.action("f1"))
                self.assertEqual(context.exception.reason, INVALID_BACKEND_RESPONSE)
                self.assertEqual(client.removed, [])
                self.assertEqual(client.unblocked, [])
                self.assertEqual(client.injected, [])

    def test_fetch_tree_failure_unwind_is_recorded(self) -> None:
        spec = _fetch_tree_spec()
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        tracker.install(spec.action("f1"))
        client.remove_error = RuntimeError("clear failed")
        events: list[Event] = []

        summary = tracker.unwind(_ListSink(events), time.monotonic)

        self.assertEqual(summary, {"unwound": 1, "misses": 1, "leaks": 1})
        (cleanup,) = events
        self.assertEqual(cleanup.type, CLEANUP_REMOVED)
        self.assertIn("RuntimeError: clear failed", cleanup.data.get("error", ""))

    def test_sl_runner_selection_is_scoped_to_contained_run(self) -> None:
        """Only descendants of the exact run-spec runner belong to a run."""
        node = "a" * 40
        with tempfile.TemporaryDirectory() as proc_root:
            own_spec = "/artifacts/own/run-spec.json"
            other_spec = "/artifacts/other/run-spec.json"

            def write_process(pid: int, ppid: int, argv: list[str]) -> None:
                process_dir = os.path.join(proc_root, str(pid))
                os.mkdir(process_dir)
                with open(os.path.join(process_dir, "cmdline"), "wb") as handle:
                    handle.write(b"\0".join(arg.encode() for arg in argv) + b"\0")
                with open(os.path.join(process_dir, "status"), "w") as handle:
                    handle.write(f"Name:\ttest\nPPid:\t{ppid}\n")

            write_process(100, 1, ["python", "cli.py", "--spec", own_spec])
            write_process(101, 100, ["sl", "goto", node])
            write_process(200, 1, ["python", "cli.py", "--spec", other_spec])
            write_process(201, 200, ["sl", "goto", node])
            write_process(301, 300, ["sl", "goto", node])
            write_process(400, 1, ["python", "cli.py", "--spec", own_spec + ".old"])
            write_process(401, 400, ["sl", "goto", node])

            self.assertEqual(
                _owned_sl_children(node, own_spec, proc_root=proc_root),
                [101],
            )

    def test_nonblock_await_uses_recorded_target_completion(self) -> None:
        """A non-block await reports the recorded target outcome in its fixed fields."""
        spec = _behavior_spec(REGISTERS[0], CHECKOUT_BEGIN_SITE, "error")
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        tracker.install(spec.action("f1"))
        (fault,) = client.injected
        self.assertEqual(fault.errorType, "runtime_error")
        self.assertIn("F", fault.errorMessage)
        tracker.record_completion(
            spec.action("t1"), BackendResult(FAIL, {"reason": fault.errorMessage})
        )
        result = tracker.wait_for_hit(_await("h1", "F"))
        self.assertEqual(
            dict(result.facts),
            {"fault_id": "F", "target_completed": True, "target_outcome": FAIL},
        )

    def test_nonblock_completion_preserves_unrelated_failure(self) -> None:
        """Completion witnessing never decorates an unrelated target failure."""
        spec = _behavior_spec(REGISTERS[0], CHECKOUT_BEGIN_SITE, "error")
        tracker = FaultTracker(_StubEnv(_StubClient()), spec)
        tracker.install(spec.action("f1"))
        failure = BackendResult(FAIL, {"reason": "unrelated failure"})
        self.assertEqual(tracker.record_completion(spec.action("t1"), failure), failure)

    def test_remove_without_hit_is_witness_failure(self) -> None:
        spec = _spec(
            REGISTERS[0],
            [_install("c0"), _checkout("c0", 2, ("f1", FINISHED))],
            [("nemesis", ["f1"]), ("target", ["c0"])],
        )
        tracker = FaultTracker(_StubEnv(_StubClient()), spec)
        tracker.install(spec.action("f1"))
        with self.assertRaises(HarnessFailure) as context:
            tracker.remove(_await("x1", "F"))
        self.assertEqual(FAULT_WITNESS_FAILED, context.exception.reason)

    def test_remove_with_leftover_registration_is_witness_failure(self) -> None:
        spec = _spec(
            REGISTERS[0],
            [_install("c0"), _checkout("c0", 2, ("f1", FINISHED))],
            [("nemesis", ["f1"]), ("target", ["c0"])],
        )
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        tracker.install(spec.action("f1"))
        client.keys = ["/fake/mount"]
        tracker.wait_for_hit(_await("h1", "F"))
        with self.assertRaises(HarnessFailure) as context:
            tracker.remove(_await("x1", "F"))
        self.assertEqual(FAULT_WITNESS_FAILED, context.exception.reason)

    def test_install_without_remove_rejected(self) -> None:
        """A fault install with no heal (remove) fails validation."""
        actions = [_install("c0"), _checkout("c0", 2, ("f1", FINISHED))]
        actors = [("nemesis", ["f1"]), ("a", ["c0"])]
        with self.assertRaisesRegex(SpecError, "no remove"):
            validate(_spec(REGISTERS[0], actions, actors))

    def test_cache_install_without_remove_rejected(self) -> None:
        """A cache-fill install with no heal (remove) fails validation."""
        actions = [_install_cache("s1"), _parked_probe()]
        actors = [("nemesis", ["f1"]), ("b", ["s1"])]
        with self.assertRaisesRegex(SpecError, "no remove"):
            validate(_spec(REGISTERS[0], actions, actors))

    def test_wfpw_install_without_remove_rejected(self) -> None:
        """A pending-writes install with no heal (remove) fails validation."""
        register = REGISTERS[0]
        actions = [_install_wfpw("t1"), _parked_sync_read(register)]
        actors = [("nemesis", ["f1"]), ("b", ["t1"])]
        with self.assertRaisesRegex(SpecError, "no remove"):
            validate(_spec(register, actions, actors))

    def test_wfpw_install_resolves_empty_key(self) -> None:
        """The catalog arms waitForPendingWrites with the empty key."""
        register = REGISTERS[0]
        spec = _wfpw_spec(register)
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        result = tracker.install(spec.action("f1"))
        self.assertEqual(result.outcome, OK)
        (fault,) = client.injected
        self.assertEqual(fault.keyClass, "waitForPendingWrites")
        self.assertEqual(fault.keyValueRegex, "")
        self.assertTrue(fault.block)
        client.keys = [""]
        hit = tracker.wait_for_hit(spec.action("h1"))
        self.assertEqual(hit.facts.get("hit"), True)

    def test_wfpw_sync_zero_target_rejected(self) -> None:
        """A sync-0 read never reaches the checkpoint, so it cannot arm it."""
        register = REGISTERS[0]
        w1 = Action(
            id="w1",
            kind="write",
            deadline=DEADLINE_OPERATION,
            payload={"path": register, "value_id": "fault-w1"},
        )
        t1 = _sync_read("t1", register, 0, ("f1", FINISHED))
        actions = [_install_wfpw("t1"), w1, t1]
        actors = [("nemesis", ["f1"]), ("b", ["w1", "t1"])]
        spec = _spec(register, actions, actors)
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        with self.assertRaises(HarnessFailure) as ctx:
            tracker.install(spec.action("f1"))
        self.assertEqual(ctx.exception.reason, INVALID_BACKEND_RESPONSE)
        self.assertEqual(client.injected, [])

    def test_lookup_install_without_remove_rejected(self) -> None:
        """A lookup install with no heal (remove) fails validation."""
        register = REGISTERS[1]
        actions = [_install_lookup("t1"), _parked_lookup_read(register)]
        actors = [("nemesis", ["f1"]), ("b", ["t1"])]
        with self.assertRaisesRegex(SpecError, "no remove"):
            validate(_spec(register, actions, actors))

    def test_lookup_install_resolves_name_key(self) -> None:
        """The catalog arms FuseDispatcherImpl::lookup with the basename."""
        register = REGISTERS[1]
        spec = _spec(
            register,
            [_install_lookup("t1"), _parked_lookup_read(register)],
            [("nemesis", ["f1"]), ("b", ["t1"])],
        )
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        result = tracker.install(spec.action("f1"))
        self.assertEqual(result.outcome, OK)
        (fault,) = client.injected
        self.assertEqual(fault.keyClass, "FuseDispatcherImpl::lookup")
        self.assertEqual(fault.keyValueRegex, "r1\\.reg")
        self.assertTrue(fault.block)
        client.keys = ["r1.reg"]
        hit = tracker.wait_for_hit(_await("h1", "F"))
        self.assertEqual(hit.facts.get("hit"), True)

    def test_lookup_remove_after_hit_releases(self) -> None:
        """A hit lookup removes cleanly once the daemon consumed the fault."""
        register = REGISTERS[1]
        spec = _spec(
            register,
            [_install_lookup("t1"), _parked_lookup_read(register)],
            [("nemesis", ["f1"]), ("b", ["t1"])],
        )
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        tracker.install(spec.action("f1"))
        client.keys = ["r1.reg"]
        tracker.wait_for_hit(_await("h1", "F"))
        # The one-shot fault is consumed on the hit and the unblock
        # releases the parked lookup, so the drain finds nothing left.
        client.keys = []
        remove = Action(
            id="x1",
            kind="fault_remove",
            deadline=DEADLINE_OPERATION,
            payload={"fault_id": "F"},
        )
        result = tracker.remove(remove)
        self.assertEqual(result.outcome, OK)
        self.assertEqual(result.facts.get("removed"), 0)

    def test_lookup_unwind_counts_miss_without_leak(self) -> None:
        """Unwinding an unhit lookup install records a miss and no leak."""
        register = REGISTERS[1]
        spec = _spec(
            register,
            [_install_lookup("t1"), _parked_lookup_read(register)],
            [("nemesis", ["f1"]), ("b", ["t1"])],
        )
        tracker = FaultTracker(_StubEnv(_StubClient()), spec)
        tracker.install(spec.action("f1"))
        events: list[Event] = []
        summary = tracker.unwind(_ListSink(events), time.monotonic)
        self.assertEqual(summary, {"unwound": 1, "misses": 1, "leaks": 0})
        (cleanup,) = events
        self.assertEqual(cleanup.type, CLEANUP_REMOVED)
        self.assertEqual(
            (cleanup.data.get("miss"), cleanup.data.get("leak")), (True, False)
        )

    def test_lookup_accepts_read_targets_only(self) -> None:
        """A fuse-lookup install arms a FUSE read by basename; a checkout's walk
        never reaches `FuseDispatcherImpl::lookup`."""
        register = REGISTERS[1]
        spec = _spec(
            register,
            [_install_lookup("t1"), _parked_lookup_read(register)],
            [("nemesis", ["f1"]), ("b", ["t1"])],
        )
        tracker = FaultTracker(_StubEnv(_StubClient()), spec)
        result = tracker.install(spec.action("f1"))
        self.assertEqual(result.outcome, OK)
        self.assertEqual(result.facts.get("selector"), "r1\\.reg")
        checkout_spec = _spec(
            REGISTERS[0],
            [_install_lookup("c0"), _checkout("c0", 2, ("f1", FINISHED))],
            [("nemesis", ["f1"]), ("a", ["c0"])],
        )
        checkout_tracker = FaultTracker(_StubEnv(_StubClient()), checkout_spec)
        with self.assertRaises(HarnessFailure):
            checkout_tracker.install(checkout_spec.action("f1"))

    def test_lookup_rejects_bad_targets(self) -> None:
        """Targets that never reach the lookup checkpoint cannot arm it."""
        register = REGISTERS[0]
        targets = [
            _probe("s1", ("f1", FINISHED)),
            _sync_read("t9", register, 60, ("f1", FINISHED)),
            Action(
                id="g9",
                kind="getattr",
                deadline=DEADLINE_OPERATION,
                requires=(("f1", FINISHED),),
                payload={"path": register},
            ),
            Action(
                id="w9",
                kind="write",
                deadline=DEADLINE_OPERATION,
                requires=(("f1", FINISHED),),
                payload={"path": register, "value_id": "bad"},
            ),
            Action(
                id="t1",
                kind="read",
                deadline=DEADLINE_RUN,
                requires=(("f1", FINISHED),),
                payload={"channel": "fuse"},
            ),
            Action(
                id="c0",
                kind="checkout",
                deadline=DEADLINE_RUN,
                requires=(("f1", FINISHED),),
                payload={"revision": True, "via": "goto", "mode": "normal"},
            ),
        ]
        for target in targets:
            with self.subTest(target=target.id):
                spec = _spec(
                    register,
                    [_install_lookup(target.id), target],
                    [("nemesis", ["f1"]), ("b", [target.id])],
                )
                client = _StubClient()
                tracker = FaultTracker(_StubEnv(client), spec)
                with self.assertRaises(HarnessFailure) as ctx:
                    tracker.install(spec.action("f1"))
                self.assertEqual(ctx.exception.reason, INVALID_BACKEND_RESPONSE)
                self.assertEqual(client.injected, [])

    def test_lookup_unknown_target_rejected(self) -> None:
        """An install naming no action cannot arm the lookup site."""
        register = REGISTERS[0]
        spec = _spec(
            register,
            [_install_lookup("ghost"), _parked_lookup_read(register)],
            [("nemesis", ["f1"]), ("b", ["t1"])],
        )
        client = _StubClient()
        tracker = FaultTracker(_StubEnv(client), spec)
        with self.assertRaises(HarnessFailure) as ctx:
            tracker.install(spec.action("f1"))
        self.assertEqual(ctx.exception.reason, INVALID_BACKEND_RESPONSE)
        self.assertEqual(client.injected, [])
