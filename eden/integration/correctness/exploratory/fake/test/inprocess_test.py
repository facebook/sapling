# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import hashlib
import io
import json
import os
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from collections.abc import Callable
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.core import (
    containment,
    scheduler as scheduler_module,
)
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    BLOCKED,
    check,
    checker_sources,
    CheckerSourceUnavailable,
    CORE_RESULT_SCHEMA,
    encode_verdict,
    HARNESS_SUSPECT,
    INVALID_HISTORY,
    NO_EVALUATION,
    NO_PROPERTY_REASON,
    NOT_EVALUATED,
    read_verdict_document,
    REJECT,
    semantic_digest,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    _runner_env,
    Execution,
    EXECUTION_NAME,
    HARNESS_FAILURE,
    load_history,
    load_replay,
    REPLAY_NAME,
    run_exploration,
    RUN_SPEC_NAME,
    runner_argv_for,
    runner_main,
    VERDICT_NAME,
)
from eden.integration.correctness.exploratory.core.history import (
    Event,
    HistorySink,
    InMemorySink,
)
from eden.integration.correctness.exploratory.core.jsonio import loads
from eden.integration.correctness.exploratory.core.processes import kill_group
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    control_rules,
    identity_witnesses,
    ObligationStatus,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    from_json,
    SpecError,
    to_json,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    Clock,
    COMPLETED,
    HarnessFailure,
    run_actions,
)
from eden.integration.correctness.exploratory.core.sources import (
    ModuleSource,
    read_module_source,
    SourceReader,
)
from eden.integration.correctness.exploratory.core.target import RUN_DIR_ENV
from eden.integration.correctness.exploratory.core.test.containment_testing import (
    deadline_after,
    private_temp_root,
)
from eden.integration.correctness.exploratory.core.test.testing import HANG_GUARD
from eden.integration.correctness.exploratory.fake.target import (
    BYTES_PROPERTY,
    encode_value,
    FAKE,
    FakeEnvironment,
)


def _wait_dead(pid: int) -> None:
    """Wait until ``pid`` is a zombie or gone, failing after `_HANG_GUARD`."""
    deadline = time.monotonic() + HANG_GUARD
    while time.monotonic() < deadline:
        try:
            with open(f"/proc/{pid}/stat") as handle:
                if handle.read().rsplit(")", 1)[1].split()[0] == "Z":
                    return
        except FileNotFoundError:
            return
        time.sleep(0.01)
    raise AssertionError(f"process {pid} survived containment")


def _capture_into(
    runners: list[subprocess.Popen[Any]],
) -> Callable[..., subprocess.Popen[Any]]:
    """Return a ``Popen`` stand-in that spawns for real and records each runner."""
    spawn = subprocess.Popen

    def capture(*args: Any, **kwargs: Any) -> subprocess.Popen[Any]:
        runners.append(spawn(*args, **kwargs))
        return runners[-1]

    return capture


_SPEC = json.dumps(
    {
        "schema": "edenfs-exploratory-run",
        "schema_version": 1,
        "seed": 7,
        "target": "fake",
        "environment": {"target": {}},
        "budgets": {
            "operation_deadline_seconds": 60,
            "cleanup_deadline_seconds": 10,
            "runner_deadline_seconds": 120,
        },
        "actors": [{"id": "a", "actions": ["w", "r"]}],
        "actions": {
            "w": {
                "kind": "write",
                "deadline": "operation",
                "path": "register",
                "value_id": "written",
            },
            "r": {"kind": "read", "deadline": "operation", "path": "register"},
        },
    }
)


_EXECUTION_FIELDS = frozenset(
    {
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
)
_UNPROPERTIED = replace(FAKE, properties=(), red_controls=frozenset())


def _sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def _canonical(value: Any) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


class _FailingSetupEnvironment(FakeEnvironment):
    """Fake environment whose setup raises, standing in for a broken target setup."""

    def setup(self) -> None:
        raise RuntimeError("injected setup failure")


def _setup_error_runner_main(spec_path: str) -> int:
    """Run the ordinary runner with an environment whose setup raises."""
    target = replace(
        FAKE, make_environment=lambda _spec, _sink: _FailingSetupEnvironment()
    )
    return runner_main(spec_path, target)


def _deadline_runner_main(spec_path: str, mode: str) -> int:
    """Exercise real runner deadline classification with target-owned hooks."""
    with open(spec_path) as handle:
        cleanup = json.load(handle)["budgets"]["cleanup_deadline_seconds"]
    actors_stop = float(os.environ[containment.RUNNER_DEADLINE_ENV]) - cleanup

    class Env(FakeEnvironment):
        def setup(self) -> None:
            """Outlast the actor reservation only for the setup deadline case."""
            if mode == "slow-setup":
                time.sleep(max(0.0, actors_stop - time.monotonic()) + 0.01)

    class Backend:
        def __call__(self, action: Action) -> BackendResult:
            """Return or fail."""
            if mode == "failure":
                raise HarnessFailure("fault-witness-failed")
            return BackendResult("ok")

        def unwind(self, sink: HistorySink, clock: Clock) -> dict[str, Any]:
            """Hold no faults."""
            return {}

    target = replace(
        FAKE, make_environment=lambda *_: Env(), make_backend=lambda *_: Backend()
    )
    return runner_main(spec_path, target)


class FakeInProcessTest(unittest.TestCase):
    """Run the fake target through the scheduler and checker without a runner."""

    def _run(self, red_control: str | None, raw: str = _SPEC) -> Verdict:
        spec = from_json(raw, FAKE)
        environment = FAKE.make_environment(spec, None)
        environment.setup()
        backend = FAKE.make_backend(environment, spec, red_control)
        sink = InMemorySink()
        status = run_actions(spec, backend, sink, time.monotonic)
        self.assertEqual(COMPLETED, status.status, status.reason)
        return check(FAKE, spec, sink.events)

    def _bytes(self, verdict: Verdict) -> Any:
        assert verdict.report is not None
        (result,) = [
            r for r in verdict.report.results if r.property_id == BYTES_PROPERTY.id
        ]
        return result

    def test_write_then_read_accepts(self) -> None:
        verdict = self._run(None)
        self.assertEqual(ACCEPT, verdict.kind)
        self.assertEqual({"bytes": ACCEPT}, verdict.lanes)
        result = self._bytes(verdict)
        self.assertEqual(
            [
                ("bytes.fabricated-value", "r", "satisfied"),
                ("bytes.stale-read", "r", "satisfied"),
                ("bytes.torn-read", "r", "satisfied"),
            ],
            [(o.rule_id, o.scope, str(o.status)) for o in result.obligations],
        )
        self.assertEqual((), result.findings)

    def test_torn_write_red_control_rejects(self) -> None:
        verdict = self._run("torn-write")
        self.assertEqual(REJECT, verdict.kind)
        (finding,) = self._bytes(verdict).findings
        self.assertEqual("bytes.torn-read", finding.rule_id)
        self.assertEqual(Attribution.UNDETERMINED, finding.attribution)
        self.assertEqual(finding.reason, verdict.reason)
        self.assertEqual(Subject(SubjectKind.PATH, "path:register"), finding.subject)
        self.assertEqual(
            (Witness("channel", "fake"), Witness("read-action", "r")),
            finding.witnesses,
        )
        rule = BYTES_PROPERTY.rules[finding.rule_id]
        self.assertEqual((("channel", "fake"),), identity_witnesses(rule, finding))

    def test_torn_write_control_maps_only_the_torn_read_rule(self) -> None:
        self.assertEqual(
            {"torn-write": frozenset({"bytes.torn-read"})},
            control_rules(FAKE.properties),
        )
        self.assertEqual(frozenset({"torn-write"}), FAKE.red_controls)
        self.assertEqual("fake-memory-v1", FAKE.semantics_profile)
        self.assertEqual(
            {SubjectKind.PATH},
            {rule.subject_kind for rule in BYTES_PROPERTY.rules.values()},
        )

    def test_malformed_read_result_is_a_harness_violation_not_a_byte_judgment(
        self,
    ) -> None:
        spec = from_json(_SPEC, FAKE)
        environment = FAKE.make_environment(spec, None)
        backend = FAKE.make_backend(environment, spec, None)
        sink = InMemorySink()
        run_actions(spec, backend, sink, time.monotonic)
        for bad in ({"bytes": "zz"}, {}, {"bytes": "00", "extra": 1}):
            with self.subTest(result=bad):
                events = [
                    (
                        replace(event, data={"action": "r", "actor": "a", **bad})
                        if event.type == "ok" and event.data.get("action") == "r"
                        else event
                    )
                    for event in sink.events
                ]
                verdict = check(FAKE, spec, events)
                self.assertEqual(HARNESS_SUSPECT, verdict.kind)
                assert verdict.report is not None
                schema = next(
                    r
                    for r in verdict.report.results
                    if r.property_id == CORE_RESULT_SCHEMA
                )
                violated = [
                    o.scope
                    for o in schema.obligations
                    if o.status == ObligationStatus.VIOLATED
                ]
                self.assertEqual(["r"], violated)
                self.assertEqual((), self._bytes(verdict).obligations)

    def test_unencodable_write_is_rejected_at_parse_time(self) -> None:
        raw = json.loads(_SPEC)
        raw["actions"]["w"] = {**raw["actions"]["w"], "value_id": "v" * 64}
        with self.assertRaisesRegex(SpecError, "encoded register value too long"):
            from_json(json.dumps(raw), FAKE)

    def test_stale_read_names_the_superseding_write(self) -> None:
        raw = json.loads(_SPEC)
        raw["actors"] = [{"id": "a", "actions": ["w0", "w", "r"]}]
        raw["actions"]["w0"] = {**raw["actions"]["w"], "value_id": "first"}
        spec = from_json(json.dumps(raw), FAKE)
        environment = FAKE.make_environment(spec, None)
        backend = FAKE.make_backend(environment, spec, None)
        sink = InMemorySink()
        self.assertEqual(
            COMPLETED, run_actions(spec, backend, sink, time.monotonic).status
        )
        superseded = encode_value(spec.seed, "a", "first").hex()
        events = [
            (
                replace(event, data={**event.data, "bytes": superseded})
                if event.type == "ok" and event.data.get("action") == "r"
                else event
            )
            for event in sink.events
        ]
        verdict = check(FAKE, spec, events)
        self.assertEqual(REJECT, verdict.kind)
        (finding,) = self._bytes(verdict).findings
        self.assertEqual("bytes.stale-read", finding.rule_id)
        self.assertEqual(Attribution.PRODUCT, finding.attribution)
        self.assertEqual((Witness("channel", "fake"),), finding.witnesses)
        assert verdict.report is not None
        (proof,) = verdict.report.proofs
        self.assertEqual(("o:r",), proof.observations)
        self.assertIn("g:w", proof.groups)

    def test_repeated_producers_are_a_finite_choice(self) -> None:
        """Six writes of one value and five overlapping reads: the engine decides them."""
        writes = [f"w{index}" for index in range(6)]
        reads = [f"r{index}" for index in range(5)]
        raw = json.loads(_SPEC)
        raw["actors"] = [{"id": "a", "actions": writes}] + [
            {"id": f"b{index}", "actions": [action]}
            for index, action in enumerate(reads)
        ]
        raw["actions"] = {
            **{
                action: {
                    "kind": "write",
                    "deadline": "operation",
                    "path": "register",
                    "value_id": "same",
                }
                for action in writes
            },
            **{
                action: {"kind": "read", "deadline": "operation", "path": "register"}
                for action in reads
            },
        }
        spec = from_json(json.dumps(raw), FAKE)
        value = encode_value(spec.seed, "a", "same").hex()
        events = [
            event
            for action in writes
            for event in (
                Event("invoke", 0.0, {"action": action}),
                Event("ok", 10.0, {"action": action}),
            )
        ] + [
            event
            for action in reads
            for event in (
                Event("invoke", 1.0, {"action": action}),
                Event("ok", 9.0, {"action": action, "bytes": value}),
            )
        ]
        verdict = check(FAKE, spec, events)
        result = self._bytes(verdict)
        statuses = {(o.rule_id, o.scope): o for o in result.obligations}
        self.assertEqual(15, len(statuses))
        self.assertEqual(
            {ObligationStatus.SATISFIED}, {o.status for o in statuses.values()}
        )
        self.assertEqual({"bytes": ACCEPT}, verdict.lanes)

    def test_reads_are_linearized_beside_a_fabricated_read(self) -> None:
        """A fabricated read keeps its direct finding and the other reads are still
        decided: A, then B, then A again from two overlapping writes."""
        raw = json.loads(_SPEC)
        reads = ["r1", "r2", "r3", "r4"]
        raw["actors"] = [{"id": "a", "actions": ["w1", "w2"]}] + [
            {"id": f"b{index}", "actions": [action]}
            for index, action in enumerate(reads)
        ]
        raw["actions"] = {
            "w1": {**raw["actions"]["w"], "value_id": "x"},
            "w2": {**raw["actions"]["w"], "value_id": "y"},
            **{action: dict(raw["actions"]["r"]) for action in reads},
        }
        spec = from_json(json.dumps(raw), FAKE)
        first = encode_value(spec.seed, "a", "x").hex()
        second = encode_value(spec.seed, "a", "y").hex()
        observed = {"r1": first, "r2": second, "r3": first, "r4": "00" * 64}
        events = [
            event
            for action in ("w1", "w2")
            for event in (
                Event("invoke", 0.0, {"action": action}),
                Event("ok", 10.0, {"action": action}),
            )
        ] + [
            event
            for index, action in enumerate(reads)
            for event in (
                Event("invoke", 1.0 + 2 * index, {"action": action}),
                Event(
                    "ok", 2.0 + 2 * index, {"action": action, "bytes": observed[action]}
                ),
            )
        ]
        verdict = check(FAKE, spec, events)
        result = self._bytes(verdict)
        violated = {
            (o.rule_id, o.scope)
            for o in result.obligations
            if o.status == ObligationStatus.VIOLATED
        }
        self.assertEqual(
            {
                ("bytes.fabricated-value", "r4"),
                ("bytes.stale-read", "r1"),
                ("bytes.stale-read", "r2"),
                ("bytes.stale-read", "r3"),
            },
            violated,
        )
        self.assertEqual(REJECT, verdict.kind)
        self.assertEqual(3 * len(reads) - 1, len(result.obligations))

    def test_writes_without_reads_are_not_evaluated(self) -> None:
        raw = json.loads(_SPEC)
        raw["actors"] = [{"id": "a", "actions": ["w"]}]
        del raw["actions"]["r"]
        verdict = self._run(None, json.dumps(raw))
        self.assertEqual(
            (NOT_EVALUATED, NO_EVALUATION),
            (verdict.kind, verdict.not_evaluated_cause),
        )
        self.assertEqual({"bytes": NOT_EVALUATED}, verdict.lanes)


class FakeSetupErrorTest(unittest.TestCase):
    """Spawn one contained fake runner whose environment setup raises."""

    def setUp(self) -> None:
        private_temp_root(self)

    def test_setup_exception_is_recorded_in_execution_json(self) -> None:
        spec = from_json(_SPEC, FAKE)
        with tempfile.TemporaryDirectory() as tmp:
            argv = runner_argv_for(os.path.join(tmp, RUN_SPEC_NAME), FAKE.name)
            argv[1] = os.path.abspath(__file__)
            argv.append("--setup-error-runner")
            result = run_exploration(spec, tmp, target=FAKE, runner_argv=argv)
            with open(os.path.join(tmp, EXECUTION_NAME)) as handle:
                execution = json.load(handle)
        self.assertEqual(HARNESS_FAILURE, result.execution.status)
        self.assertEqual(0, execution["events"])
        self.assertEqual("setup-begin", execution["runner_phase"])
        record = execution["setup_error"]
        self.assertEqual("RuntimeError", record["type"])
        self.assertEqual("injected setup failure", record["message"])
        self.assertLessEqual(len(record["log_tail"]), 40)
        self.assertIn("RuntimeError: injected setup failure", record["log_tail"])


class FakeContainedVerdictTest(unittest.TestCase):
    """Run contained fake runners and read back the manifests they leave."""

    def setUp(self) -> None:
        private_temp_root(self)

    def _contained(
        self, red_control: str | None, target: Any = FAKE
    ) -> tuple[Any, dict[str, Any], bytes, str]:
        spec = from_json(_SPEC, FAKE)
        with tempfile.TemporaryDirectory() as tmp:
            argv = runner_argv_for(
                os.path.join(tmp, RUN_SPEC_NAME), FAKE.name, red_control
            )
            argv[1] = os.path.abspath(__file__)
            argv.append("--fake-runner")
            result = run_exploration(
                spec, tmp, target=target, red_control=red_control, runner_argv=argv
            )
            execution = json.loads((Path(tmp) / EXECUTION_NAME).read_text())
            verdict = (Path(tmp) / VERDICT_NAME).read_bytes()
            runspec = _sha256((Path(tmp) / RUN_SPEC_NAME).read_bytes())
        return result, execution, verdict, runspec

    def test_verdict_document_round_trips_and_binds_execution(self) -> None:
        for red_control, kind in ((None, ACCEPT), ("torn-write", REJECT)):
            with self.subTest(red_control=red_control):
                result, execution, written, runspec = self._contained(red_control)
                self.assertEqual(kind, result.verdict.kind)
                self.assertEqual(encode_verdict(result.verdict), written)
                self.assertEqual(
                    result.verdict, read_verdict_document(json.loads(written))
                )
                report = result.verdict.report
                assert report is not None
                self.assertEqual(
                    _sha256(_canonical(execution)), report.bindings.execution
                )
                self.assertEqual(runspec, report.bindings.runspec)
                self.assertEqual(_EXECUTION_FIELDS, frozenset(execution))
                self.assertEqual(
                    _EXECUTION_FIELDS, frozenset(Execution.__dataclass_fields__)
                )
                self.assertEqual(COMPLETED, execution["status"])
                self.assertEqual("ok", execution["reason"])

    def test_zero_property_target_is_not_evaluated(self) -> None:
        for red_control in (None, "torn-write"):
            with self.subTest(red_control=red_control):
                result, execution, written, _ = self._contained(
                    red_control, _UNPROPERTIED
                )
                verdict = read_verdict_document(json.loads(written))
                self.assertEqual(COMPLETED, execution["status"])
                self.assertEqual(
                    (NOT_EVALUATED, NO_EVALUATION, NO_PROPERTY_REASON, {}),
                    (
                        verdict.kind,
                        verdict.not_evaluated_cause,
                        verdict.reason,
                        verdict.lanes,
                    ),
                )
                assert verdict.report is not None
                self.assertEqual((), verdict.report.results)
                self.assertEqual(result.verdict, verdict)

    def test_pre_launch_failures_are_not_evaluated(self) -> None:
        """A run that never reached the target is not evaluated, with or without properties."""

        def broken() -> None:
            raise RuntimeError("injected pre-launch failure")

        for target, cause in ((FAKE, BLOCKED), (_UNPROPERTIED, NO_EVALUATION)):
            with (
                self.subTest(target=bool(target.properties)),
                tempfile.TemporaryDirectory() as tmp,
            ):
                result = run_exploration(
                    from_json(_SPEC, FAKE), tmp, target=target, setup=broken
                )
                verdict = read_verdict_document(
                    json.loads((Path(tmp) / VERDICT_NAME).read_text())
                )
                self.assertEqual(HARNESS_FAILURE, result.execution.status)
                self.assertEqual(
                    (NOT_EVALUATED, cause), (verdict.kind, verdict.not_evaluated_cause)
                )
                self.assertEqual(result.verdict, verdict)

    def test_semantic_digest_binds_the_fake_target_sources(self) -> None:
        package = "eden.integration.correctness.exploratory"
        names = set(checker_sources(FAKE))
        for module in (
            "fake.target",
            "core.scheduler",
            "core.actions",
            "core.checker",
            "core.facts",
            "core.effects",
        ):
            self.assertIn(f"{package}.{module}", names)
        self.assertNotIn(f"{package}.fake.test.inprocess_test", names)

        def changing(changed: str) -> SourceReader:
            def read(name: str) -> ModuleSource | None:
                source = read_module_source(name)
                if name != changed or source is None:
                    return source
                return ModuleSource(source.data + b"\nCHANGED = 1\n", source.is_package)

            return read

        digest = semantic_digest(FAKE)
        for module in ("fake.target", "core.scheduler", "core.actions"):
            with self.subTest(dependency=module):
                changed = semantic_digest(FAKE, reader=changing(f"{package}.{module}"))
                self.assertNotEqual(digest, changed)
        unrelated = semantic_digest(
            FAKE, reader=changing(f"{package}.core.containment")
        )
        self.assertEqual(digest, unrelated)

    def test_unbindable_target_fails_before_anything_runs(self) -> None:
        def collect_bytes(*_args: Any) -> None:
            raise AssertionError("never evaluated")

        collect_bytes.__module__ = "eden_exploratory_module_without_source"
        target = replace(
            FAKE, properties=(replace(BYTES_PROPERTY, collect=collect_bytes),)
        )
        setup = mock.Mock()
        with tempfile.TemporaryDirectory() as tmp:
            artifacts = os.path.join(tmp, "run")
            with (
                mock.patch.object(
                    containment.subprocess,
                    "Popen",
                    side_effect=AssertionError("must not spawn"),
                ) as popen,
                self.assertRaises(CheckerSourceUnavailable),
            ):
                run_exploration(
                    from_json(_SPEC, FAKE), artifacts, target=target, setup=setup
                )
            popen.assert_not_called()
            setup.assert_not_called()
            self.assertFalse(os.path.exists(artifacts))

    def test_containment_never_signals_process_group_one(self) -> None:
        with mock.patch.object(containment.os, "killpg") as killpg:
            for pgid in (0, 1, mock.MagicMock().pid):
                with self.subTest(pgid=pgid), self.assertRaises(ValueError):
                    kill_group(pgid)
            killpg.assert_not_called()
            environment = mock.Mock()
            environment.processes.lookup.return_value = mock.MagicMock().pid
            with self.assertRaises(ValueError):
                containment._contain_workers(environment, ["a1"])
            killpg.assert_not_called()

    def test_judge_failure_keeps_execution_and_replay(self) -> None:
        def broken() -> None:
            raise RuntimeError("injected pre-launch failure")

        with (
            tempfile.TemporaryDirectory() as tmp,
            mock.patch.object(
                containment, "judge", side_effect=RuntimeError("judge failed")
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, "judge failed"):
                run_exploration(from_json(_SPEC, FAKE), tmp, target=FAKE, setup=broken)
            execution = json.loads((Path(tmp) / EXECUTION_NAME).read_text())
            self.assertEqual(HARNESS_FAILURE, execution["status"])
            _, events = load_replay(os.path.join(tmp, REPLAY_NAME), FAKE)
            self.assertEqual([], events)
            self.assertFalse((Path(tmp) / VERDICT_NAME).exists())


class StrictJsonTest(unittest.TestCase):
    def setUp(self) -> None:
        private_temp_root(self)

    def test_repeated_keys_and_non_finite_numbers_are_refused(self) -> None:
        good = '{"type":"cleanup","time":1,"text":"x"}'
        self.assertEqual({"type": "cleanup", "time": 1, "text": "x"}, loads(good))
        bad = (
            '{"type":"cleanup","time":1,"time":2}',
            '{"type":"cleanup","time":NaN}',
            '{"type":"cleanup","time":Infinity}',
            '{"type":"cleanup","time":-Infinity}',
            '{"type":"cleanup","time":1e999}',
            '{"type":"cleanup","time":1,"data":{"a":1,"a":2}}',
        )
        for text in bad:
            with self.subTest(text=text):
                with self.assertRaises(ValueError):
                    loads(text)
                with tempfile.TemporaryDirectory() as tmp:
                    history = Path(tmp) / "history.jsonl"
                    history.write_text(good + "\n" + text + "\n")
                    with self.assertRaises(SpecError):
                        load_history(str(history))
                    spec = json.loads(_SPEC)
                    replay = Path(tmp) / REPLAY_NAME
                    replay.write_text(
                        '{"history":['
                        + text
                        + '],"replay_command":"x","spec":'
                        + json.dumps(spec)
                        + "}"
                    )
                    with self.assertRaises(SpecError):
                        load_replay(str(replay), FAKE)

    def test_contained_history_stops_at_a_non_finite_or_repeated_line(self) -> None:
        prefix = b'{"type":"cleanup","time":1,"text":"prefix"}\n'
        for bad in (
            b'{"type":"cleanup","time":NaN}\n',
            b'{"type":"cleanup","time":2,"time":3}\n',
        ):
            with self.subTest(line=bad), tempfile.TemporaryDirectory() as tmp:
                script = f"import os; os.write(1, {prefix + bad!r})"
                result = run_exploration(
                    from_json(_SPEC, FAKE),
                    tmp,
                    target=FAKE,
                    runner_argv=[sys.executable, "-c", script],
                )
                self.assertEqual(HARNESS_FAILURE, result.execution.status)
                self.assertTrue(
                    result.execution.reason.startswith("broken-history-transport:")
                )
                _, replay = load_replay(os.path.join(tmp, REPLAY_NAME), FAKE)
                self.assertEqual(["prefix"], [e.data["text"] for e in replay])
                self.assertEqual(
                    replay,
                    containment.durable_history(os.path.join(tmp, "history.jsonl")),
                )


if __name__ == "__main__" and "--fake-runner" in sys.argv:
    raise SystemExit(
        runner_main(
            sys.argv[sys.argv.index("--spec") + 1],
            FAKE,
            (
                sys.argv[sys.argv.index("--red-control") + 1]
                if "--red-control" in sys.argv
                else None
            ),
        )
    )

if __name__ == "__main__" and "--setup-error-runner" in sys.argv:
    raise SystemExit(_setup_error_runner_main(sys.argv[sys.argv.index("--spec") + 1]))

if __name__ == "__main__" and "--deadline-runner" in sys.argv:
    raise SystemExit(
        _deadline_runner_main(
            sys.argv[sys.argv.index("--spec") + 1],
            sys.argv[sys.argv.index("--deadline-runner") + 1],
        )
    )


class RunnerCleanupBudgetTest(unittest.TestCase):
    def setUp(self) -> None:
        private_temp_root(self)

    def test_a_record_cut_off_at_the_deadline_stays_out_of_the_durable_prefix(
        self,
    ) -> None:
        """A large line torn by the runner deadline is untrusted tail only.

        The deadline expires once the supervisor has read part of the torn line.
        """
        spec = from_json(_SPEC, FAKE)
        prefix = b'{"type":"cleanup","time":1,"text":"prefix"}\n'
        torn = b'{"type":"cleanup","time":2,"text":"' + b"x" * 100_000
        script = (
            f"import os, signal; os.write(1, {prefix!r}); os.write(1, {torn!r}); "
            "signal.pause()"
        )
        with (
            tempfile.TemporaryDirectory() as tmp,
            deadline_after(lambda output: len(output) > len(prefix)),
        ):
            result = run_exploration(
                spec, tmp, target=FAKE, runner_argv=[sys.executable, "-c", script]
            )
            self.assertEqual(containment.CONTAINMENT_TIMEOUT, result.execution.status)
            history = Path(tmp) / "history.jsonl"
            with self.assertRaises(SpecError):
                containment.load_history(str(history))
            durable = containment.durable_history(history)
            _, replay = load_replay(os.path.join(tmp, REPLAY_NAME), FAKE)
        self.assertEqual(["prefix"], [e.data["text"] for e in durable])
        self.assertEqual(replay, durable)
        self.assertEqual(1, result.execution.events)

    def test_deadline_reason_survives_supervisor_containment(self) -> None:
        """Execution artifacts retain the winning runner reason.

        Setup that outlasts the actor reservation is runner-deadline; timely
        setup completes; a true earlier failure keeps its own reason. Each
        runner's cleanup has `_HANG_GUARD` before the supervisor's SIGKILL.
        """
        base = from_json(_SPEC, FAKE)
        # A one-second actor reservation, which slow setup sleeps past.
        short = replace(
            base,
            budgets=replace(
                base.budgets,
                operation_deadline_seconds=1,
                cleanup_deadline_seconds=HANG_GUARD,
                runner_deadline_seconds=HANG_GUARD + 1,
            ),
        )
        # An actor reservation that only a hang could exhaust.
        long = replace(
            base,
            budgets=replace(
                base.budgets,
                operation_deadline_seconds=HANG_GUARD,
                cleanup_deadline_seconds=HANG_GUARD,
                runner_deadline_seconds=2 * HANG_GUARD,
            ),
        )
        for mode, spec, expected in (
            ("slow-setup", short, "runner-deadline"),
            ("failure", long, "fault-witness-failed"),
            ("timely", long, "ok"),
        ):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                argv = runner_argv_for(os.path.join(tmp, RUN_SPEC_NAME), FAKE.name)
                argv[1] = os.path.abspath(__file__)
                argv += ["--deadline-runner", mode]
                result = run_exploration(spec, tmp, target=FAKE, runner_argv=argv)
                execution = json.loads((Path(tmp) / EXECUTION_NAME).read_text())
                self.assertEqual(result.execution.reason, expected)
                self.assertEqual(execution["reason"], expected)
                self.assertEqual(
                    result.execution.status,
                    "completed" if mode == "timely" else HARNESS_FAILURE,
                )

    def test_history_transport_rejects_invalid_utf8_without_changing_bytes(
        self,
    ) -> None:
        """Invalid transport fails closed and preserves raw evidence/manifests.

        Split valid UTF-8 and complete JSON without a newline are accept twins.
        Only valid records preceding the first bad record enter replay facts.
        """
        prefix = b'{"type":"cleanup","time":1,"text":"prefix"}\n'
        valid = b'{"type":"cleanup","time":2,"text":"\xc3\xa9"}'
        invalid = b'{"type":"cleanup","time":2,"text":"\xff"}'
        suffix = b'{"type":"cleanup","time":3,"text":"suffix"}\n'
        cases = (
            ("invalid-line", (prefix + invalid + b"\n" + suffix,), False),
            ("invalid-tail", (prefix + invalid,), False),
            ("truncated-utf8", (prefix + valid[:-3],), False),
            ("invalid-json", (prefix + b'{"type":bad}\n' + suffix,), False),
            ("split-valid", (prefix + valid[:-3], valid[-3:] + b"\n"), True),
            ("complete-tail", (prefix + valid,), True),
        )
        for name, chunks, accepted in cases:
            with self.subTest(case=name), tempfile.TemporaryDirectory() as tmp:
                script = (
                    f"import os,time; chunks={chunks!r}; "
                    "[(os.write(1, chunk), time.sleep(.02)) for chunk in chunks]"
                )
                result = run_exploration(
                    from_json(_SPEC, FAKE),
                    tmp,
                    target=FAKE,
                    runner_argv=[sys.executable, "-c", script],
                )
                self.assertEqual(
                    COMPLETED if accepted else HARNESS_FAILURE, result.execution.status
                )
                if not accepted:
                    self.assertTrue(
                        result.execution.reason.startswith("broken-history-transport:")
                    )
                    with self.assertRaises(SpecError):
                        containment.load_history(os.path.join(tmp, "history.jsonl"))
                raw = (Path(tmp) / "history.jsonl").read_bytes()
                emitted = b"".join(chunks)
                self.assertEqual(
                    emitted if emitted.endswith(b"\n") else emitted + b"\n", raw
                )
                _, replay = containment.load_replay(
                    os.path.join(tmp, "replay.json"), FAKE
                )
                self.assertEqual(
                    ["prefix", "é"] if accepted else ["prefix"],
                    [e.data["text"] for e in replay],
                )
                # The tolerant reader yields exactly the bound durable prefix.
                self.assertEqual(
                    replay,
                    containment.durable_history(Path(tmp) / "history.jsonl"),
                )
                execution = json.loads((Path(tmp) / "execution.json").read_text())
                self.assertEqual(result.execution.status, execution["status"])
                self.assertEqual(result.execution.reason, execution["reason"])
                verdict = read_verdict_document(
                    json.loads((Path(tmp) / VERDICT_NAME).read_text())
                )
                self.assertEqual(result.verdict, verdict)
                if accepted:
                    # A completed execution must carry the scheduler's record.
                    self.assertEqual(INVALID_HISTORY, verdict.not_evaluated_cause)

    def test_killed_runner_records_exactly_sigkill(self) -> None:
        """A runner killed at its deadline records exactly -SIGKILL.

        The runner never exits on its own, so only the kill ends it. The
        supervisor waits for that exit under a hang guard of at least
        `_HANG_GUARD`, not a short drain window that a loaded host can miss.
        """
        spec = from_json(_SPEC, FAKE)
        spec = replace(
            spec,
            budgets=replace(
                spec.budgets,
                operation_deadline_seconds=0.1,
                cleanup_deadline_seconds=0.1,
                runner_deadline_seconds=0.5,
            ),
        )
        waits: list[float | None] = []

        class RecordingPopen(subprocess.Popen[Any]):
            def wait(self, timeout: float | None = None) -> int:
                """Record each reap bound the supervisor uses."""
                waits.append(timeout)
                return super().wait(timeout=timeout)

        with (
            tempfile.TemporaryDirectory() as tmp,
            mock.patch.object(containment.subprocess, "Popen", RecordingPopen),
        ):
            result = run_exploration(
                spec,
                tmp,
                target=FAKE,
                runner_argv=[sys.executable, "-c", "import signal; signal.pause()"],
            )
            execution = json.loads((Path(tmp) / EXECUTION_NAME).read_text())
        self.assertEqual(containment.CONTAINMENT_TIMEOUT, result.execution.status)
        self.assertEqual(-signal.SIGKILL, result.execution.returncode)
        self.assertEqual(-signal.SIGKILL, execution["returncode"])
        (reap,) = waits
        assert reap is not None
        self.assertGreaterEqual(reap, HANG_GUARD)

    def test_descendant_stdout_cannot_extend_supervisor_deadline(self) -> None:
        """Inherited stdout cannot extend D; exited descendants finish normally.

        A timeout runner leaves a descendant that holds its stdout forever,
        so a supervisor that waited for that stdout would never return. The
        deadline expires once the runner's line is read; the runner is
        SIGKILLed and the supervisor kills the descendant by run dir. The real
        timeout runner retains one event; its timely twin exits normally.
        """
        spec = from_json(_SPEC, FAKE)
        holding = "import signal; signal.pause()"
        for timeout, detached in ((True, False), (True, True), (False, False)):
            with (
                self.subTest(timeout=timeout, detached=detached),
                tempfile.TemporaryDirectory() as tmp,
            ):
                holder = holding if timeout else "pass"
                script = f"""
import json, os, signal, subprocess, sys, time
p = subprocess.Popen([sys.executable, '-c', {holder!r}, os.environ[{RUN_DIR_ENV!r}]], start_new_session={detached})
print(json.dumps({{'type': 'cleanup', 'time': time.monotonic(), 'pid': p.pid}}), flush=True)
{"signal.pause()" if timeout else "p.wait()"}
"""
                runners: list[subprocess.Popen[Any]] = []
                started = time.monotonic()
                with (
                    mock.patch.object(
                        containment.subprocess,
                        "Popen",
                        side_effect=_capture_into(runners),
                    ),
                    deadline_after(
                        (lambda output: b"\n" in output) if timeout else lambda _: False
                    ),
                ):
                    result = containment.run_exploration(
                        spec,
                        tmp,
                        target=FAKE,
                        runner_argv=[sys.executable, "-c", script],
                    )
                # A hang guard: a supervisor waiting on the holder's stdout would
                # never return.
                self.assertLess(time.monotonic() - started, 2 * HANG_GUARD)
                expected = (
                    containment.CONTAINMENT_TIMEOUT
                    if timeout
                    else containment.COMPLETED
                )
                self.assertEqual(expected, result.execution.status)
                exit_status = -signal.SIGKILL if timeout else 0
                self.assertEqual(exit_status, runners[0].wait(timeout=HANG_GUARD))
                self.assertEqual(exit_status, result.execution.returncode)
                events = containment.load_history(os.path.join(tmp, "history.jsonl"))
                self.assertEqual(1, len(events))
                _wait_dead(events[0].data["pid"])
                self.assertEqual(
                    expected,
                    json.loads((Path(tmp) / "execution.json").read_text())["status"],
                )
                _, replay = containment.load_replay(
                    os.path.join(tmp, "replay.json"), FAKE
                )
                self.assertEqual(events, replay)

    def test_resource_joins_preserve_environment_close(self) -> None:
        """Resource joins cannot consume environment close time.

        A frozen clock places cleanup inside the final quarter of the runner
        budget; resource joins then consume their whole slice, and close
        still receives exactly C/4.

        test_returning_runner_has_one_cleanup_record is the accept twin.
        """
        now = [1000.0]
        cleanup = 0.4
        deadline = now[0] + HANG_GUARD
        frozen = SimpleNamespace(monotonic=lambda: now[0], sleep=time.sleep)
        closed: list[float] = []

        class Env(FakeEnvironment):
            def close(self) -> str | None:
                """Record how much runner time remains for environment close."""
                closed.append(deadline - now[0])
                return None

        env = Env()
        original = FAKE.make_backend(env, from_json(_SPEC, FAKE), None)
        real_unwind = original.unwind
        joins: list[tuple[float, float]] = []

        def unwind(sink: HistorySink, clock: Clock) -> dict[str, Any]:
            result = real_unwind(sink, clock)
            now[0] = deadline - 0.15
            return result

        def close_resources(join_deadline: float) -> int:
            joins.append((now[0], join_deadline))
            now[0] = join_deadline
            return 0

        target = replace(
            FAKE, make_environment=lambda *_: env, make_backend=lambda *_: original
        )
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "run-spec.json"
            raw = json.loads(_SPEC)
            raw["budgets"] = {
                "operation_deadline_seconds": HANG_GUARD,
                "cleanup_deadline_seconds": cleanup,
                "runner_deadline_seconds": 2 * HANG_GUARD,
            }
            path.write_text(json.dumps(raw))
            with (
                mock.patch.dict(
                    os.environ, {"EDEN_EXPLORATORY_RUNNER_DEADLINE": str(deadline)}
                ),
                mock.patch.object(containment, "time", frozen),
                mock.patch.object(scheduler_module, "time", frozen),
                mock.patch.object(original, "unwind", side_effect=unwind),
                mock.patch.object(
                    original, "close_resources", close_resources, create=True
                ),
                mock.patch("sys.stdout", io.StringIO()),
            ):
                self.assertEqual(runner_main(str(path), target), 0)
        ((called, join_deadline),) = joins
        self.assertEqual(deadline - 0.15, called)
        self.assertEqual(deadline - cleanup / 4, join_deadline)
        self.assertEqual([deadline - join_deadline], closed)

    def _cleanup_runner(self, backend: Any) -> tuple[int, dict[str, Any]]:
        """Run the real runner with a substitute backend; return its exit and cleanup record."""
        target = replace(FAKE, make_backend=lambda *_: backend)
        output = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "run-spec.json"
            raw = json.loads(_SPEC)
            raw["budgets"] = {
                "operation_deadline_seconds": HANG_GUARD,
                "cleanup_deadline_seconds": 0.4,
                "runner_deadline_seconds": 2 * HANG_GUARD,
            }
            path.write_text(json.dumps(raw))
            with mock.patch("sys.stdout", output):
                code = runner_main(str(path), target)
        events = [json.loads(line) for line in output.getvalue().splitlines()]
        (record,) = [e for e in events if "scheduler" in e]
        return code, record

    def test_resource_leaks_survive_a_wedged_unwind(self) -> None:
        release = threading.Event()

        class Backend:
            def __call__(self, action: Action) -> BackendResult:
                """Complete every action."""
                return BackendResult("ok", {"bytes": b"x".hex()})

            def unwind_faults(self, sink: HistorySink, clock: Clock) -> dict[str, Any]:
                """Hang past the unwind slice without recording leaks."""
                release.wait(HANG_GUARD)
                return {"leaks": 0}

            def close_resources(self, deadline: float) -> int:
                """Report one leaked resource."""
                return 1

        try:
            code, record = self._cleanup_runner(Backend())
        finally:
            release.set()
        self.assertEqual(1, code)
        self.assertNotIn("unwind", record)
        self.assertEqual(1, record["resource_leaks"])

    def test_resource_leaks_fail_an_otherwise_clean_runner(self) -> None:
        class Backend:
            def __call__(self, action: Action) -> BackendResult:
                """Complete every action."""
                return BackendResult("ok", {"bytes": b"x".hex()})

            def unwind_faults(self, sink: HistorySink, clock: Clock) -> dict[str, Any]:
                """Unwind cleanly; this backend has no `unwind` fallback at all."""
                return {"leaks": 0}

            def close_resources(self, deadline: float) -> int:
                """Report one leaked resource."""
                return 1

        code, record = self._cleanup_runner(Backend())
        self.assertEqual(1, code)
        self.assertEqual(
            ({"leaks": 0}, 1), (record["unwind"], record["resource_leaks"])
        )

    def _run_runner(
        self,
        *,
        wedged: bool,
        cleanup_seconds: float,
        runner_seconds: float,
        slow_setup: bool = False,
    ) -> tuple[list[dict[str, Any]], list[str], float, list[float]]:
        """Exercise the real runner lifecycle with synchronized fake target hooks.

        Returns the stdout records, the hook order, the supervisor deadline, and
        the runner deadline each scheduler run received.
        """
        order: list[str] = []
        release = threading.Event()
        returned = threading.Event()
        process: list[subprocess.Popen[Any]] = []
        deadline = time.monotonic() + runner_seconds

        class Env(FakeEnvironment):
            def setup(self) -> None:
                """Spend time in setup before the scheduler starts."""
                if slow_setup:
                    time.sleep(0.5)

            def close(self) -> str | None:
                """Assert calls are released or killed before environment teardown."""
                order.append("close")
                if wedged:
                    self_test.assertEqual(
                        process[0].wait(timeout=HANG_GUARD), -signal.SIGKILL
                    )
                else:
                    self_test.assertTrue(returned.is_set())
                return None

        env = Env()
        self_test = self

        class Backend:
            def __call__(self, action: Action) -> BackendResult:
                """Park a call on a fault or a contained process."""
                if wedged:
                    proc = subprocess.Popen(
                        [sys.executable, "-c", "import signal; signal.pause()"],
                        start_new_session=True,
                    )
                    process.append(proc)
                    env.processes.publish(action.id, proc, time.monotonic())
                    proc.wait()
                    env.processes.retire(action.id, proc)
                else:
                    release.wait(HANG_GUARD)
                    order.append("returned")
                    returned.set()
                return BackendResult("ok")

            def unwind(self, sink: HistorySink, clock: Clock) -> dict[str, Any]:
                """Release the fault without waiting for the parked call."""
                order.append("unwind")
                if not wedged:
                    release.set()
                return {}

        spec = from_json(_SPEC, FAKE)
        spec = replace(
            spec,
            budgets=replace(
                spec.budgets,
                operation_deadline_seconds=0.05,
                cleanup_deadline_seconds=cleanup_seconds,
                runner_deadline_seconds=runner_seconds,
            ),
            actors=(replace(spec.actors[0], actions=("w",)),),
            actions=(next(a for a in spec.actions if a.id == "w"),),
        )
        target = replace(
            FAKE, make_environment=lambda *_: env, make_backend=lambda *_: Backend()
        )
        runner_deadlines: list[float] = []
        real_run_actions = containment.run_actions

        def run_actions(*args: Any, **kwargs: Any) -> Any:
            runner_deadlines.append(kwargs["runner_deadline"])
            return real_run_actions(*args, **kwargs)

        output = io.StringIO()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                path = os.path.join(tmp, RUN_SPEC_NAME)
                with open(path, "w") as handle:
                    handle.write(to_json(spec))
                with (
                    mock.patch.dict(
                        os.environ, {"EDEN_EXPLORATORY_RUNNER_DEADLINE": str(deadline)}
                    ),
                    mock.patch.object(containment, "run_actions", run_actions),
                    mock.patch("sys.stdout", output),
                ):
                    self.assertEqual(runner_main(path, target), 1)
        finally:
            release.set()
            for proc in process:
                if proc.poll() is None:
                    proc.kill()
                proc.wait(timeout=HANG_GUARD)
        return (
            [json.loads(line) for line in output.getvalue().splitlines()],
            order,
            deadline,
            runner_deadlines,
        )

    def test_returning_runner_has_one_cleanup_record(self) -> None:
        """A returning runner retains one final cleanup record with no active workers."""
        output = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "run-spec.json"
            path.write_text(_SPEC)
            with mock.patch("sys.stdout", output):
                self.assertEqual(runner_main(str(path), FAKE), 0)
        events = [json.loads(line) for line in output.getvalue().splitlines()]
        cleanups = [event for event in events if event["type"] == "cleanup"]
        self.assertEqual(len(cleanups), 1)
        self.assertEqual(cleanups[0]["active_at_teardown"], [])

    def test_slow_setup_preserves_cleanup_and_releases_parked_call(self) -> None:
        """Setup shares the supervisor deadline; fault unwind precedes worker join.

        The scheduler receives the supervisor's absolute deadline after setup,
        not a fresh one. Its C/2 join bound is a hang guard for the released
        call.
        """
        events, order, deadline, runner_deadlines = self._run_runner(
            wedged=False,
            slow_setup=True,
            cleanup_seconds=2 * HANG_GUARD,
            runner_seconds=4 * HANG_GUARD,
        )
        self.assertEqual([deadline], runner_deadlines)
        self.assertEqual(order, ["unwind", "returned", "close"])
        self.assertEqual(events[-1]["type"], "cleanup")

    def test_wedged_process_group_killed_and_recorded_before_close(self) -> None:
        """A wedged owned process is killed and recorded before teardown."""
        events, order, _deadline, _runner_deadlines = self._run_runner(
            wedged=True, cleanup_seconds=1.0, runner_seconds=HANG_GUARD
        )
        self.assertEqual(order, ["unwind", "close"])
        self.assertEqual(events[-1]["active_at_teardown"], ["w"])


class RunnerDeadlineTransportTest(unittest.TestCase):
    def test_deadline_does_not_leak_to_backend_processes(self) -> None:
        """The supervisor deadline belongs to the runner only."""
        with mock.patch.dict(os.environ, {"EDEN_EXPLORATORY_RUNNER_DEADLINE": "123.0"}):
            self.assertNotIn("EDEN_EXPLORATORY_RUNNER_DEADLINE", _runner_env())

    def test_invalid_deadline_fails_before_setup(self) -> None:
        """Malformed or nonfinite deadlines cannot disable supervisor containment."""
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "run-spec.json"
            path.write_text(_SPEC)
            for value in ("bad", "nan", "inf"):
                with (
                    self.subTest(value=value),
                    mock.patch.dict(
                        os.environ, {"EDEN_EXPLORATORY_RUNNER_DEADLINE": value}
                    ),
                    mock.patch.object(FakeEnvironment, "setup") as setup,
                ):
                    self.assertEqual(runner_main(str(path), FAKE), 2)
                    setup.assert_not_called()


def _gone(pid: int) -> bool:
    """Return whether `pid` exited, polling until `HANG_GUARD`."""
    deadline = time.monotonic() + HANG_GUARD
    while time.monotonic() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return True
        time.sleep(0.05)
    return False


def _child_python(script: str) -> subprocess.CompletedProcess[str]:
    """Run `script` in a python child that can import this package."""
    return subprocess.run(
        [sys.executable, "-c", script],
        env=_runner_env(),
        capture_output=True,
        text=True,
        timeout=HANG_GUARD,
    )


class RunnerProcessIsolationTest(unittest.TestCase):
    def setUp(self) -> None:
        private_temp_root(self)

    def test_close_run_dir_kills_survivors_after_the_runner_removed_it(self) -> None:
        """The kill scan runs even when the runner's close already removed run_dir."""
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = os.path.join(tmp, "removed")
        survivor = subprocess.Popen(
            ["sleep", "300"], env={**os.environ, RUN_DIR_ENV: run_dir}
        )
        self.addCleanup(survivor.kill)
        self.assertEqual("runner", containment.close_run_dir(run_dir))
        self.assertEqual(-signal.SIGKILL, survivor.wait(timeout=HANG_GUARD))

    def test_close_run_dir_keeps_the_dir_when_a_kill_is_refused(self) -> None:
        """A pid that refuses SIGKILL keeps the dir and its mounts in place."""

        def refuse(pid: int, sig: int) -> None:
            raise PermissionError(pid, sig)

        with tempfile.TemporaryDirectory() as run_dir:
            with (
                mock.patch.object(containment, "_pids_owning", return_value=[4242]),
                mock.patch.object(containment.os, "kill", refuse),
                mock.patch.object(containment.subprocess, "run") as unmount,
            ):
                cleanup = containment.close_run_dir(run_dir)
            self.assertTrue(os.path.isdir(run_dir))
        self.assertEqual("supervisor: cannot kill [4242]; kept", cleanup)
        unmount.assert_not_called()

    def test_close_run_dir_aborts_only_its_own_wedged_fuse_connection(self) -> None:
        """A run blocked on its own FUSE mount is unwedged by aborting that connection.

        Processes blocked in the mount survive SIGKILL until the connection
        is aborted; a busy unmount gets the same abort before its retry. The
        cleanup text records the connection's waiting count and the D-state
        threads. Another FUSE mount's connection is never touched.
        """
        with tempfile.TemporaryDirectory() as root:
            run_dir = os.path.join(root, "run dir")
            os.mkdir(run_dir)
            escaped = run_dir.replace(" ", "\\040")
            mountinfo = os.path.join(root, "mountinfo")
            with open(mountinfo, "w") as handle:
                handle.write(
                    f"30 1 0:92 / /data/repos/fbsource rw - fuse.edenfs edenfs rw\n"
                    f"31 1 0:123 / {escaped}/mounts/repo rw - fuse.edenfs edenfs rw\n"
                    f"32 1 8:3 / {escaped}/scratch rw - ext4 /dev/sda3 rw\n"
                )
            sysfs = os.path.join(root, "connections")
            for connection, waiting in (("92", "5"), ("123", "2"), ("3", "0")):
                os.makedirs(os.path.join(sysfs, connection))
                Path(sysfs, connection, "waiting").write_text(waiting + "\n")
                Path(sysfs, connection, "abort").write_text("")
            proc = os.path.join(root, "proc")
            for tid, state, wchan in (
                ("4242", "Z", "0"),
                ("4243", "D", "fuse_simple_request"),
            ):
                os.makedirs(os.path.join(proc, "4242", "task", tid))
                Path(proc, "4242", "task", tid, "stat").write_text(
                    f"{tid} (a b) {state} 1"
                )
                Path(proc, "4242", "task", tid, "wchan").write_text(wchan)

            def aborted(connection: str) -> bool:
                return Path(sysfs, connection, "abort").read_text() == "1"

            with (
                mock.patch.object(containment, "_MOUNTINFO", mountinfo),
                mock.patch.object(containment, "_FUSE_CONNECTIONS", sysfs),
                mock.patch.object(containment, "_PROC", proc),
                mock.patch.object(
                    containment,
                    "_pids_owning",
                    side_effect=lambda _run_dir: [] if aborted("123") else [4242],
                ),
                mock.patch.object(containment.os, "kill"),
                mock.patch.object(
                    containment,
                    "poll_until",
                    side_effect=lambda predicate, _deadline, _interval: predicate(),
                ),
            ):
                cleanup = containment.close_run_dir(run_dir)
            self.assertFalse(os.path.lexists(run_dir))
            self.assertEqual(
                f"{containment.MOUNT_WEDGED_PREFIX}"
                "blocked threads ['4242/4243:fuse_simple_request']; "
                f"{run_dir}/mounts/repo connection 123 waiting=2 aborted; "
                "unmounted 0, removed",
                cleanup,
            )
            self.assertEqual(
                [False, True, False], [aborted(c) for c in ("92", "123", "3")]
            )

            Path(sysfs, "123", "abort").write_text("")
            os.mkdir(run_dir)
            busy = subprocess.CompletedProcess([], 1, "", "Device or resource busy")
            with (
                mock.patch.object(containment, "_MOUNTINFO", mountinfo),
                mock.patch.object(containment, "_FUSE_CONNECTIONS", sysfs),
                mock.patch.object(containment, "_pids_owning", return_value=[]),
                mock.patch.object(
                    containment,
                    "_mounts_under",
                    return_value=[f"{run_dir}/mounts/repo"],
                ),
                mock.patch.object(
                    containment,
                    "_fusermount",
                    side_effect=[busy, subprocess.CompletedProcess([], 0, "", "")],
                ),
            ):
                cleanup = containment.close_run_dir(run_dir)
            self.assertTrue(cleanup.startswith(containment.MOUNT_WEDGED_PREFIX))
            self.assertTrue(cleanup.endswith("unmounted 1, removed"))
            self.assertEqual([False, True], [aborted(c) for c in ("92", "123")])

    def test_children_inheriting_stdout_cannot_reach_the_history(self) -> None:
        """After the transport moves, a child's inherited fd 1 is stderr."""
        result = _child_python(
            "import subprocess\n"
            "from eden.integration.correctness.exploratory.core import containment\n"
            "transport = containment._private_transport()\n"
            "subprocess.run(['echo', 'from a child'], check=True)\n"
            "transport.write('history\\n')\n"
            "transport.flush()\n"
        )
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual("history\n", result.stdout)
        self.assertIn("from a child", result.stderr)

    def test_runner_dies_with_its_supervisor(self) -> None:
        """A child spawned with the pre-exec hook dies when its parent is SIGKILLed."""
        parent = subprocess.Popen(
            [
                sys.executable,
                "-c",
                "import subprocess, signal\n"
                "from eden.integration.correctness.exploratory.core import containment\n"
                "child = subprocess.Popen(['sleep', '300'],"
                " preexec_fn=containment._die_with_spawning_thread())\n"
                "print(child.pid, flush=True)\n"
                "signal.pause()\n",
            ],
            env=_runner_env(),
            stdout=subprocess.PIPE,
            text=True,
        )
        self.addCleanup(parent.kill)
        assert parent.stdout is not None
        child = int(parent.stdout.readline())
        self.addCleanup(lambda: _gone(child) or os.kill(child, signal.SIGKILL))
        parent.kill()
        parent.wait(timeout=HANG_GUARD)
        self.assertTrue(_gone(child))

    def test_a_run_first_sweeps_run_dirs_whose_lock_is_free(self) -> None:
        """A dead run's dir is closed before the next run; a dir without a lock stays."""
        with tempfile.TemporaryDirectory() as root:
            orphan = os.path.join(root, "exploratory.dead")
            unlocked = os.path.join(root, "exploratory.unlocked")
            os.mkdir(orphan)
            os.mkdir(unlocked)
            Path(orphan, "run.lock").touch()
            daemon = subprocess.Popen(
                ["sleep", "300"], env={**os.environ, RUN_DIR_ENV: orphan}
            )
            self.addCleanup(daemon.kill)
            with (
                tempfile.TemporaryDirectory() as tmp,
                mock.patch.object(tempfile, "tempdir", root),
            ):
                result = run_exploration(
                    from_json(_SPEC, FAKE),
                    tmp,
                    target=FAKE,
                    runner_argv=[sys.executable, "-c", "pass"],
                )
            self.assertFalse(os.path.lexists(orphan))
            self.assertTrue(os.path.isdir(unlocked))
            self.assertEqual(-signal.SIGKILL, daemon.wait(timeout=HANG_GUARD))
        self.assertEqual(
            {orphan: "supervisor: unmounted 0, removed"},
            (result.execution.setup or {}).get("swept"),
        )

    def test_a_run_never_sweeps_a_run_dir_whose_lock_is_held(self) -> None:
        """A live run's dir is left alone, and a run's own lock is held while it runs."""
        with tempfile.TemporaryDirectory() as root:
            live = os.path.join(root, "exploratory.live")
            os.mkdir(live)
            holder = subprocess.Popen(
                [
                    sys.executable,
                    "-c",
                    "import fcntl, os, signal, sys\n"
                    "fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT)\n"
                    "fcntl.flock(fd, fcntl.LOCK_SH)\n"
                    "print(flush=True)\n"
                    "signal.pause()\n",
                    os.path.join(live, "run.lock"),
                ],
                env={**os.environ, RUN_DIR_ENV: live},
                stdout=subprocess.PIPE,
                text=True,
            )
            self.addCleanup(holder.kill)
            assert holder.stdout is not None
            holder.stdout.readline()
            probe = (
                "import fcntl, os, sys\n"
                f"fd = os.open(os.environ[{RUN_DIR_ENV!r}] + '/run.lock', os.O_RDWR)\n"
                "try:\n"
                "    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)\n"
                "except BlockingIOError:\n"
                "    sys.exit(0)\n"
                "sys.exit(3)\n"
            )
            with (
                tempfile.TemporaryDirectory() as tmp,
                mock.patch.object(tempfile, "tempdir", root),
            ):
                result = run_exploration(
                    from_json(_SPEC, FAKE),
                    tmp,
                    target=FAKE,
                    runner_argv=[sys.executable, "-c", probe],
                )
            self.assertIsNone(holder.poll())
            self.assertTrue(os.path.isdir(live))
        self.assertEqual(0, result.execution.returncode)
        self.assertNotIn("swept", result.execution.setup or {})

    def test_a_line_past_the_event_bound_breaks_the_transport(self) -> None:
        """The supervisor stops buffering a line once it passes `MAX_EVENT_BYTES`."""
        script = (
            "import os, signal; "
            f"os.write(1, b'{{' + b'x' * {containment.MAX_EVENT_BYTES + 2}); "
            "signal.pause()"
        )
        with tempfile.TemporaryDirectory() as tmp:
            result = run_exploration(
                from_json(_SPEC, FAKE),
                tmp,
                target=FAKE,
                runner_argv=[sys.executable, "-c", script],
            )
        self.assertEqual(HARNESS_FAILURE, result.execution.status)
        self.assertEqual(
            "broken-history-transport: line exceeds "
            f"{containment.MAX_EVENT_BYTES} bytes",
            result.execution.reason,
        )

    def test_a_blank_history_line_is_a_bad_line(self) -> None:
        """`load_history` reads exactly what containment ingested: no skipped lines."""
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "history.jsonl")
            Path(path).write_text('{"type":"cleanup","time":1}\n\n')
            with self.assertRaisesRegex(SpecError, ":2: bad line"):
                load_history(path)
