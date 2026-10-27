# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Live runs: capture, fixture provenance, and differential pairs, judged by `EDENFS_TARGET`.

Each run executes under the recording target (whose verdict is always
not-evaluated) and its recorded artifacts are then re-judged by the
property-bearing `EDENFS_TARGET`, which must accept, or leave exactly the
named not-evaluated reasons.
"""

from __future__ import annotations

import errno
import json
import os
import unittest
from dataclasses import replace
from typing import Any

from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    check,
    NO_EVALUATION,
    NO_PROPERTY_REASON,
    NOT_EVALUATED,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    load_history,
)
from eden.integration.correctness.exploratory.core.differential import REFERENCE_RESULT
from eden.integration.correctness.exploratory.core.history import (
    FAIL,
    INFO,
    INVOKE,
    OK,
    RUN_CLOSED,
)
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEADLINE_OPERATION,
    Environment as SpecEnvironment,
    from_json,
    RunSpec,
    to_json,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    CASE_INSENSITIVE_MOUNT,
    expects_reference,
)
from eden.integration.correctness.exploratory.edenfs.contracts.files import (
    failure_errno_name,
)
from eden.integration.correctness.exploratory.edenfs.encoding import (
    content_token,
    encode_committed,
)
from eden.integration.correctness.exploratory.edenfs.environment import TARGET_SETUP
from eden.integration.correctness.exploratory.edenfs.fixture import layout_for
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    RECORDING_TARGET,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _artifact_directory,
    _fail_on_setup_failure,
    _kill_status_spec,
    _require_live,
    _spec,
    decode_problems,
    facts_of,
    REGISTERS,
)
from eden.integration.correctness.exploratory.edenfs.test.live import run_contained

_REGISTER = REGISTERS[0]
_EXECUTION_FIELDS = {
    "status",
    "reason",
    "events",
    "returncode",
    "runner_phase",
    "cleanup",
    "setup",
    "daemon_exit",
    "setup_error",
    "backend_error",
}


def _action(action_id: str, kind: str, **payload: Any) -> Action:
    return Action(action_id, kind, DEADLINE_OPERATION, payload=payload)


def _with_profile(
    spec: RunSpec, case_sensitive: bool = True, **profile: Any
) -> RunSpec:
    target = dict(spec.environment.target)
    target["case_sensitive"] = case_sensitive
    target["setup_profile"] = {**target["setup_profile"], **profile}
    return replace(spec, environment=SpecEnvironment(target=target))


def _differential_spec(actions: list[Action], scatter_dirty: int) -> RunSpec:
    spec = _spec(_REGISTER, actions, [("a", [a.id for a in actions])])
    return _with_profile(spec, reference_clone=True, scatter_dirty=scatter_dirty)


def _assert_recorded(
    case: unittest.TestCase, spec: RunSpec, result: Any, events: Any
) -> Any:
    """Require a not-evaluated verdict and a history whose every side decodes."""
    verdict = result.verdict
    case.assertEqual(
        (NOT_EVALUATED, NO_PROPERTY_REASON, NO_EVALUATION),
        (verdict.kind, verdict.reason, verdict.not_evaluated_cause),
    )
    facts = facts_of(spec, events, result.execution.status)
    case.assertEqual({}, decode_problems(facts))
    return facts


def _paired(facts: Any) -> set[str]:
    """Return the actions whose terminal carried a reference side."""
    return {
        action_id
        for action_id, recorded in facts.actions.items()
        if recorded.reference is not None
    }


def _fields(facts: Any, action_id: str) -> tuple[Any, Any]:
    """Return one action's decoded primary and reference result fields."""
    recorded = facts.action(action_id)
    reference = recorded.reference
    assert recorded.result is not None
    assert reference is not None and reference.result is not None
    return recorded.result.fields, reference.result.fields


def _assert_judged(
    case: unittest.TestCase,
    spec: RunSpec,
    events: Any,
    execution: Any,
    not_evaluated: frozenset[str] = frozenset(),
) -> Verdict:
    """Re-judge a recorded live run with `EDENFS_TARGET`.

    Nothing may be violated; the not-evaluated reasons must be exactly
    `not_evaluated` (accept when it is empty).
    """
    verdict = check(
        EDENFS_TARGET,
        from_json(to_json(spec), EDENFS_TARGET),
        events,
        execution=execution,
    )
    report = verdict.report
    assert report is not None, verdict.reason
    obligations = [o for result in report.results for o in result.obligations]
    case.assertEqual(
        [],
        [
            (o.rule_id, o.scope, o.reason)
            for o in obligations
            if o.status is ObligationStatus.VIOLATED
        ],
    )
    case.assertEqual(
        not_evaluated,
        frozenset(
            o.not_evaluated_reason or ""
            for o in obligations
            if o.status is ObligationStatus.NOT_EVALUATED
        ),
    )
    if not not_evaluated:
        case.assertEqual(ACCEPT, verdict.kind, verdict.reason)
    satisfied = sum(o.status is ObligationStatus.SATISFIED for o in obligations)
    print(f"EDENFS_TARGET verdict: {verdict.kind} ({satisfied} satisfied)")
    return verdict


class ReferenceLiveTest(unittest.TestCase):
    def _run(self, spec: RunSpec, prefix: str, **kwargs: Any) -> Any:
        _require_live(self)
        with _artifact_directory(prefix) as tmp:
            result = run_contained(spec, tmp, target=RECORDING_TARGET, **kwargs)
            with open(os.path.join(tmp, "stderr.log")) as handle:
                self.assertNotIn("ReferenceFailure", handle.read())
            _fail_on_setup_failure(result, tmp)
            with open(os.path.join(tmp, "execution.json")) as handle:
                execution = json.load(handle)
            with open(os.path.join(tmp, "stderr.log")) as handle:
                stderr = handle.read()
            events = load_history(os.path.join(tmp, "history.jsonl"))
        return result, execution, events, stderr

    def test_dirty_start_matches_and_every_shared_action_pairs(self) -> None:
        dirty = layout_for(_differential_spec([], 2)).file_at(0, 3).path
        actions = [
            _action("s0", "status", channel="sl"),
            _action("w1", "write", path=_REGISTER, value_id="v1"),
            _action("r1", "read", path=_REGISTER, channel="fuse"),
            _action("g1", "getattr", path=_REGISTER),
            _action("o1", "open", path=_REGISTER),
            _action("f1", "read_fd", fd="o1"),
            _action("c1", "close", fd="o1"),
            _action("m1", "mkdir", path="tree/registers/made"),
            _action("d1", "readdir", path="tree/registers", channel="fuse"),
            _action("s1", "status", channel="sl"),
        ]
        spec = _differential_spec(actions, 2)
        result, execution, events, stderr = self._run(spec, "reference-live.")
        self.assertEqual(COMPLETED, result.execution.status, stderr[-4000:])

        facts = _assert_recorded(self, spec, result, events)
        shared = {a.id for a in actions if expects_reference(spec, a)}
        self.assertEqual({a.id for a in actions}, shared)
        self.assertEqual(shared, _paired(facts))
        primary, reference = _fields(facts, "s0")
        self.assertEqual(primary["status"], reference["status"])
        self.assertIn({"path": dirty, "code": "M"}, reference["status"])
        self.assertEqual(primary["diff_paths"], reference["diff_paths"])
        terminals = [e for e in events if REFERENCE_RESULT in e.data]
        self.assertEqual(len(shared), len(terminals))

        _assert_judged(self, spec, events, execution)
        for action_id in shared:
            recorded = facts.action(action_id)
            reference = recorded.reference
            assert reference is not None and recorded.interval is not None
            self.assertEqual("reference", reference.side)
            self.assertEqual(recorded.outcome, reference.outcome, action_id)
            assert reference.interval is not None
            self.assertLessEqual(recorded.interval[0], reference.interval[0])
            self.assertLessEqual(reference.interval[1], recorded.interval[1])

        setup = execution["setup"]
        reference = setup["reference"]
        (early,) = [e for e in events if e.type == TARGET_SETUP]
        self.assertEqual(
            reference["initial_state"],
            early.data["setup"]["reference"]["initial_state"],
        )
        self.assertEqual(2, reference["dirty_paths"])
        self.assertTrue(reference["initial_state"].startswith("sha256:"))
        self.assertEqual({"paired": len(shared), "failures": []}, reference["runtime"])
        self.assertEqual(_EXECUTION_FIELDS, set(execution))

    def test_sapling_processes_pair_and_leave_nothing_running(self) -> None:
        actions = [
            _action("k1", "checkout", revision=1, via="goto", mode="normal"),
            _action("r1", "read", path=_REGISTER, channel="fuse"),
            _action(
                "v1",
                "sl_mv",
                source=_REGISTER,
                destination="tree/registers/moved.reg",
            ),
            _action("s1", "status", channel="sl"),
            _action("k2", "checkout", revision=3, via="goto", mode="normal"),
        ]
        spec = _differential_spec(actions, 0)
        result, execution, events, stderr = self._run(spec, "reference-sapling.")
        self.assertEqual(COMPLETED, result.execution.status, stderr[-4000:])
        facts = _assert_recorded(self, spec, result, events)
        self.assertEqual({a.id for a in actions}, _paired(facts))
        for action_id in ("k1", "v1", "s1"):
            recorded = facts.action(action_id)
            assert recorded.reference is not None
            self.assertEqual(
                (OK, OK), (recorded.outcome, recorded.reference.outcome), action_id
            )
        primary, reference = _fields(facts, "s1")
        self.assertEqual(primary["status"], reference["status"])
        self.assertEqual([], execution["setup"]["reference"]["runtime"]["failures"])
        _assert_judged(self, spec, events, execution)

    def test_a_dirty_normal_checkout_fails_alike_by_return_code(self) -> None:
        """Both sides refuse a normal checkout over dirty files as a process failure.

        Neither side reports an errno, so the pair compares by return code.
        Register bytes are revision-specific, so the dirty register conflicts.
        """
        actions = [
            _action("w1", "write", path=_REGISTER, value_id="v1"),
            _action("k1", "checkout", revision=1, via="goto", mode="normal"),
            _action("k2", "checkout", revision=3, via="goto", mode="normal"),
        ]
        spec = _differential_spec(actions, 2)
        result, execution, events, stderr = self._run(spec, "reference-dirty.")
        self.assertEqual(COMPLETED, result.execution.status, stderr[-4000:])
        facts = _assert_recorded(self, spec, result, events)
        failed = [
            action_id
            for action_id in ("k1", "k2")
            if facts.action(action_id).outcome == FAIL
        ]
        self.assertTrue(failed, [(i, facts.action(i).outcome) for i in ("k1", "k2")])
        for action_id in failed:
            side = facts.action(action_id).reference
            assert side is not None
            self.assertEqual(FAIL, side.outcome)
            primary, reference = _fields(facts, action_id)
            self.assertNotIn("errno", primary)
            self.assertNotIn("errno", reference)
            self.assertEqual(primary["returncode"], reference["returncode"])
        _assert_judged(self, spec, events, execution)

    def test_setup_is_durable_before_actions_and_run_closed_is_last(self) -> None:
        spec = _with_profile(
            _spec(
                _REGISTER,
                [_action("r1", "read", path=_REGISTER, channel="fuse")],
                [("a", ["r1"])],
            )
        )
        result, execution, events, stderr = self._run(spec, "setup.")
        self.assertEqual(COMPLETED, result.execution.status, stderr[-4000:])
        self.assertEqual(_EXECUTION_FIELDS, set(execution))
        types = [event.type for event in events]
        self.assertLess(types.index(TARGET_SETUP), types.index(INVOKE))
        self.assertEqual(RUN_CLOSED, types[-1])
        self.assertGreater(events[-1].time, max(e.time for e in events[:-1]))
        self.assertNotIn("reference", execution["setup"])
        self.assertTrue(all(REFERENCE_RESULT not in e.data for e in events))
        _assert_judged(self, spec, events, execution)

    def test_differential_runs_with_eden_only_actions_are_refused_before_setup(
        self,
    ) -> None:
        """Faults and Thrift checkouts are refused by validation, never run diverged."""
        cases = {
            "eden-only-nemesis": _action(
                "f1",
                "fault_install",
                fault_id="F",
                target_action="q1",
                site="lookup",
                behavior="block",
            ),
            "eden-only-channel": _action(
                "c2", "checkout", revision=3, via="thrift", mode="normal"
            ),
        }
        for reason, action in cases.items():
            with self.subTest(reason=reason):
                spec = _differential_spec(
                    [action, _action("q1", "read", path=_REGISTER, channel="fuse")], 0
                )
                with _artifact_directory("reference-refused.") as tmp:
                    result = run_contained(spec, tmp, target=RECORDING_TARGET)
                    with open(os.path.join(tmp, "stderr.log")) as handle:
                        stderr = handle.read()
                self.assertNotEqual(COMPLETED, result.execution.status)
                self.assertEqual(2, result.execution.returncode)
                self.assertIn(reason, stderr)
                self.assertNotIn("setup-begin", stderr)

    def test_case_insensitive_differential_run_is_refused_without_a_daemon(
        self,
    ) -> None:
        spec = _with_profile(
            _differential_spec(
                [_action("r1", "read", path=_REGISTER, channel="fuse")], 0
            ),
            case_sensitive=False,
        )
        with _artifact_directory("reference-refused.") as tmp:
            result = run_contained(spec, tmp, target=RECORDING_TARGET)
            with open(os.path.join(tmp, "stderr.log")) as handle:
                stderr = handle.read()
        self.assertNotEqual(COMPLETED, result.execution.status)
        self.assertEqual(2, result.execution.returncode)
        self.assertIn(CASE_INSENSITIVE_MOUNT, stderr)
        self.assertNotIn("setup-begin", stderr)


_WIDE = {"name": "tree/wide", "entries": 2000, "churn_all": True}


class LiveCaptureTest(unittest.TestCase):
    """One live run covering every kind a recording-only run can complete, judged by `EDENFS_TARGET`."""

    def test_every_kind_records_a_decodable_result_and_registers_change_per_revision(
        self,
    ) -> None:
        r0, r1 = REGISTERS
        moved = "tree/registers/moved.reg"
        thrift = {"channel": "thrift", "sync_timeout_seconds": 60}
        actions = [
            _action("q0", "read", path=r0, channel="fuse"),
            _action("k1", "checkout", revision=1, via="goto", mode="normal"),
            _action("q1", "read", path=r0, channel="fuse"),
            # Thrift status of the current parent asks Sapling for it, so it
            # runs while Sapling's and EdenFS's parents agree.
            _action("st", "status", channel="thrift"),
            _action("k2", "checkout", revision=3, via="thrift", mode="normal"),
            # A Thrift checkout leaves Sapling's `.` behind EdenFS's parent.
            _action("su", "status", channel="thrift"),
            _action("k3", "checkout", revision=2, via="thrift", mode="force"),
            _action("p1", "manifest_presence", from_revision=0, to_revision=3),
            _action("w1", "write", path=r0, value_id="w1"),
            _action("t1", "read", path=r0, **thrift),
            _action("g1", "getattr", path=r0),
            _action("o1", "open", path=r0),
            _action("f1", "read_fd", fd="o1"),
            _action("c1", "close", fd="o1"),
            _action("h1", "chmod", path=r0, mode=0o755),
            _action("m1", "mkdir", path="tree/registers/made"),
            _action("m2", "mkdir", path="tree/registers/made"),
            _action("d1", "readdir", path="tree/registers", channel="fuse"),
            _action("d2", "readdir", path="tree/registers", channel="thrift"),
            _action("n1", "rename", source=r1, destination=moved),
            _action("q2", "read", path=r1, channel="fuse"),
            _action("t2", "read", path=r1, **thrift),
            _action("s1", "status", channel="sl"),
            _action("s2", "status", channel="sl", revision=1),
            _action("j1", "journal_window", since_sequence=0),
            _action("j2", "journal_window"),
            _action("sc", "status_counts"),
            _action("v1", "sl_mv", source=r0, destination="tree/registers/sl.reg"),
            _action("x1", "sl_rm", path=r1),
            _action("u1", "unlink", path=r1),
            _action("ui", "unload_inodes", path="tree/registers"),
            _action("b1", "background_read", root="tree", seconds=0.5),
            _action("y1", "readdir_stream", path="tree/wide", pause_after=1000),
            _action("y2", "readdir_resume", stream="y1"),
            _action("z1", "sleep", seconds=0.0),
        ]
        spec = _with_profile(
            _spec(r0, actions, [("a", [a.id for a in actions])]), wide_dir=_WIDE
        )
        target = dict(spec.environment.target)
        target["repository"] = {**target["repository"], "registers": [r0, r1]}
        spec = replace(spec, environment=SpecEnvironment(target=target))
        _require_live(self)
        with _artifact_directory("live-capture.") as tmp:
            result = run_contained(spec, tmp, target=RECORDING_TARGET)
            _fail_on_setup_failure(result, tmp)
            with open(os.path.join(tmp, "stderr.log")) as handle:
                stderr = handle.read()
            with open(os.path.join(tmp, "execution.json")) as handle:
                execution = json.load(handle)
            events = load_history(os.path.join(tmp, "history.jsonl"))
        terminals = [
            (event.type, event.data.get("action"), event.data.get("reason"))
            for event in events
            if event.type in ("ok", "fail", "info")
        ]
        self.assertEqual(
            COMPLETED, result.execution.status, f"{terminals[-3:]} {stderr[-2000:]}"
        )
        facts = _assert_recorded(self, spec, result, events)
        outcomes = {
            action.id: str(facts.action(action.id).outcome) for action in actions
        }
        print(f"live outcomes: {json.dumps(outcomes, sort_keys=True)}")
        self.assertNotIn(INFO, outcomes.values())
        kinds = {facts.action(a.id).kind for a in actions}
        self.assertEqual(
            frozenset(RECORDING_TARGET.actions)
            - {
                "fault_install",
                "fault_await",
                "fault_remove",
                "kill_process",
                "blocked_faults",
            },
            kinds,
        )
        for action_id in ("q2", "u1", "m2", "su"):
            self.assertEqual("fail", outcomes[action_id], action_id)
        self.assertEqual(errno.ENOENT, facts.action("q2").result.errno)
        self.assertEqual(errno.EEXIST, facts.action("m2").result.errno)
        # A Thrift read's EdenError keeps its POSIX code in `error_code`,
        # which the contracts read as the errno it names.
        thrift_read = facts.action("t2")
        self.assertEqual(
            (None, errno.ENOENT, "ENOENT"),
            (
                thrift_read.result.errno,
                thrift_read.result.get("error_code"),
                failure_errno_name(thrift_read),
            ),
        )
        # The status after the Thrift checkout is a typed failure, not a
        # harness failure that ends the run.
        stale = facts.action("su").result
        self.assertEqual(
            ("thrift", "OUT_OF_DATE_PARENT"),
            (stale.get("source"), stale.get("error_type")),
            dict(stale.fields),
        )
        # Committed register bytes are revision-specific.
        for action_id, revision in (("q0", 3), ("q1", 1)):
            read = facts.action(action_id).result
            assert read is not None
            self.assertEqual(
                content_token(encode_committed(spec.seed, r0, revision)),
                read.get("sha256"),
                action_id,
            )
        # `p1` probes the manifests from revision 0, which no checkout here
        # visits: its deleted residue-0 paths (`tree/a0/000/00000.dat`) are
        # absent at every revision the run can be at, and they are judged
        # through the class `fixture.representatives` forms for them.
        _assert_judged(self, spec, events, execution)

    def test_fault_and_kill_kinds_decode_from_a_live_run(self) -> None:
        """The fault kinds and `kill_process`, which the capture run leaves out.

        A block fault parks an sl status at the status-cache fill; the
        nemesis awaits the hit, kills the status's process group, and
        removes the fault. Every result decodes.
        """
        spec = _kill_status_spec(_REGISTER)
        _require_live(self)
        with _artifact_directory("live-faults.") as tmp:
            result = run_contained(spec, tmp, target=RECORDING_TARGET)
            _fail_on_setup_failure(result, tmp)
            with open(os.path.join(tmp, "stderr.log")) as handle:
                stderr = handle.read()
            with open(os.path.join(tmp, "execution.json")) as handle:
                execution = json.load(handle)
            events = load_history(os.path.join(tmp, "history.jsonl"))
        self.assertEqual(COMPLETED, result.execution.status, stderr[-2000:])
        facts = _assert_recorded(self, spec, result, events)
        outcomes = {
            action.id: str(facts.action(action.id).outcome) for action in spec.actions
        }
        print(f"live fault outcomes: {json.dumps(outcomes, sort_keys=True)}")
        self.assertEqual(
            {"fault_install", "fault_await", "fault_remove", "kill_process"},
            {facts.action(a.id).kind for a in spec.actions} - {"status"},
        )
        # The kill the spec plans ends the parked status `fail`.
        self.assertEqual("fail", outcomes["s1"])
        for action_id in ("f1", "h1", "k1", "x1"):
            self.assertEqual("ok", outcomes[action_id], action_id)
            self.assertIsNotNone(facts.action(action_id).result, action_id)
        install = spec.action("f1")
        self.assertTrue(RECORDING_TARGET.install_witnessed(spec, facts, install))
        _assert_judged(self, spec, events, execution)
