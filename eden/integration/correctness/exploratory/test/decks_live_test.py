# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Live deck lanes, judged by `EDENFS_TARGET`: `expected: accept` decks fail on
any other verdict, and `expected: not-evaluated` decks fail unless exactly their
declared reasons leave the verdict not evaluated. An `expected: reject` deck
that completes must reject with its `expected_task_key` among its canonical
error-set keys; one that does not complete must leave every execution finding
its `expected_execution` names. Any other outcome, an accept included, fails
the deck; a keyless reject deck that completes rejecting is a skip notice.

Every completed run's setup must record the `fatal_eden_bug` fact its deck declares.
Each red control on the red-control deck must complete and reject with exactly
the rules `expected_controls` names."""

from __future__ import annotations

import dataclasses
import json
import os
import tempfile
import unittest
from collections.abc import Callable
from contextlib import redirect_stdout
from dataclasses import asdict, dataclass
from io import StringIO
from pathlib import Path
from unittest import mock

from eden.integration.correctness.exploratory import advisory
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    NOT_EVALUATED,
    REJECT,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    Execution,
    ExplorationResult,
    HARNESS_FAILURE,
)
from eden.integration.correctness.exploratory.core.generate import admitted_strata
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.sites import PUBLIC_MASTER
from eden.integration.correctness.exploratory.edenfs.actions import reference_clone
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    run_live,
)
from eden.integration.correctness.exploratory.edenfs.test import backends_fixtures
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _artifact_directory,
    live_environment_problem,
)
from eden.integration.correctness.exploratory.findings import execution_findings, keyed
from eden.integration.correctness.exploratory.schemas import (
    load_verdict_file,
    SchemaError,
    VerdictFile,
)
from eden.integration.correctness.exploratory.test.waiting import Waiting, WAITING_ON

DAEMON_LABEL = PUBLIC_MASTER
DECKS = Path(advisory.__file__).with_name("decks")


def load_specs() -> dict[str, RunSpec]:
    """Every deck, parsed under `EDENFS_TARGET`."""
    return advisory.load_specs(DECKS, EDENFS_TARGET)


_ADMITTED_SITES = {site for site, _, _ in admitted_strata(EDENFS_TARGET, DAEMON_LABEL)}


def _setup_failure(artifacts: str, result: ExplorationResult) -> str | None:
    """Return the runner's setup failure, if setup failed after the environment check."""
    try:
        stderr = Path(artifacts, "stderr.log").read_text()
    except OSError:
        stderr = ""
    for text in (result.execution.reason, stderr):
        if "setup-failed" in text:
            return text.strip()
    return None


@unittest.skipUnless(
    os.environ.get("EDEN_EXPLORATORY_REPLAYS") == "1",
    "set EDEN_EXPLORATORY_REPLAYS=1 for live deck replays",
)
class DecksLiveTest(unittest.TestCase):
    """One method per deck, generated below from `metadata.expected`."""

    @classmethod
    def tearDownClass(cls) -> None:
        WAITING.report()

    def _run(self, name: str, spec: RunSpec, red_control: str | None = None) -> DeckRun:
        """Run a deck on the in-tree daemon, keeping its artifacts under TPX.

        Skip when its sites are not admitted or the host fails the pre-setup
        environment check; fail when the runner's setup fails after that.
        """
        sites = {a.payload["site"] for a in spec.actions if a.kind == "fault_install"}
        if not sites <= _ADMITTED_SITES:
            self.skipTest(
                f"deck {name} uses {sorted(sites - _ADMITTED_SITES)}:"
                f" not admitted on daemon {DAEMON_LABEL!r}"
            )
        problem = live_environment_problem()
        if problem is not None:
            self.skipTest(f"live EdenFS unavailable: {problem}")
        with _artifact_directory(f"deck-{name}.") as artifacts:
            result = (
                run_live(spec, artifacts)
                if red_control is None
                else run_live(spec, artifacts, red_control=red_control)
            )
            failure = _setup_failure(artifacts, result)
            if failure is not None:
                self.fail(f"deck {name}: {failure} (artifacts {artifacts})")
            # A run that did not complete carries the supervisor's setup
            # record, which holds no daemon facts.
            if (
                result.execution.status == COMPLETED
                and result.execution.setup is not None
            ):
                self.assertEqual(
                    spec.metadata["fatal_eden_bug"],
                    result.execution.setup.get("fatal_eden_bug"),
                    f"deck {name} declares another fatal_eden_bug (artifacts {artifacts})",
                )
            run = DeckRun(result, artifacts, _execution_ids(Path(artifacts)))
        print(
            f"deck {name}: {result.execution.status} {result.verdict.kind}"
            f" execution {list(run.execution_ids)}"
            f" differential {differential_evaluations(result)} artifacts {artifacts}"
        )
        return run


@dataclass(frozen=True)
class DeckRun:
    """One live deck run, where its artifacts are, and its typed execution IDs."""

    result: ExplorationResult
    artifacts: str
    execution_ids: tuple[str, ...]


def _execution_ids(run_dir: Path) -> tuple[str, ...]:
    """Return the run's typed execution findings by stable ID, in order."""
    try:
        verdict: VerdictFile | SchemaError = load_verdict_file(run_dir / "verdict.json")
    except SchemaError as error:
        verdict = error
    return tuple(
        dict.fromkeys(
            f.execution_id for f in execution_findings(run_dir, EDENFS_TARGET, verdict)
        )
    )


def differential_evaluations(result: ExplorationResult) -> int:
    """Return how many differential obligations the run judged (satisfied or violated)."""
    report = result.verdict.report
    if report is None:
        return 0
    return sum(
        o.status.value != "not-evaluated"
        for r in report.results
        if r.property_id == "differential"
        for o in r.obligations
    )


def _accept_lane(name: str, spec: RunSpec) -> Callable[[DecksLiveTest], None]:
    def test(self: DecksLiveTest) -> None:
        run = self._run(name, spec)
        result, where = run.result, f"artifacts {run.artifacts}"
        self.assertEqual(
            result.execution.status, COMPLETED, f"{result.execution} {where}"
        )
        self.assertEqual(result.verdict.kind, ACCEPT, f"{result.verdict} {where}")
        if reference_clone(spec):
            self.assertGreater(
                differential_evaluations(result),
                0,
                f"deck {name} opts into the reference but compared nothing ({where})",
            )

    test.__doc__ = (
        f"Deck {name} is `expected: accept`; a reject fails the build, and an"
        " opted-in reference must compare at least one shared action."
    )
    return test


def _not_evaluated_lane(name: str, spec: RunSpec) -> Callable[[DecksLiveTest], None]:
    def test(self: DecksLiveTest) -> None:
        run = self._run(name, spec)
        result, where = run.result, f"artifacts {run.artifacts}"
        self.assertEqual(
            result.execution.status, COMPLETED, f"{result.execution} {where}"
        )
        self.assertEqual(
            result.verdict.kind, NOT_EVALUATED, f"{result.verdict} {where}"
        )
        self.assertEqual(
            spec.metadata["expected_not_evaluated"],
            advisory.not_evaluated_reasons(result.verdict),
            where,
        )

    test.__doc__ = (
        f"Deck {name} is `expected: not-evaluated`; any other verdict, or other"
        " not-evaluated reasons, fails the build."
    )
    return test


def deck_identity(result: ExplorationResult) -> tuple[list[str], list[str]]:
    """Return the sorted rules and canonical keys of a run's fully classified error sets."""
    found = keyed(result.verdict, EDENFS_TARGET).occurrences
    rules = sorted({rule for occurrence in found for rule in occurrence.rules})
    return rules, sorted(occurrence.task_key for occurrence in found)


def _reject_lane(name: str, spec: RunSpec) -> Callable[[DecksLiveTest], None]:
    def test(self: DecksLiveTest) -> None:
        run = self._run(name, spec)
        result, where = run.result, f"artifacts {run.artifacts}"
        task = spec.metadata["task"]
        if result.execution.status != COMPLETED:
            expected_execution = spec.metadata.get("expected_execution")
            if not expected_execution or not set(expected_execution) <= set(
                run.execution_ids
            ):
                self.fail(
                    f"deck {name} did not complete ({result.execution.status}):"
                    f" execution {list(run.execution_ids)}, expected"
                    f" {expected_execution} ({where})"
                )
            return
        self.assertEqual(
            result.verdict.kind,
            REJECT,
            f"deck {name}: {task} not reproduced ({result.verdict}, {where})",
        )
        expected = spec.metadata.get("expected_task_key")
        rules, keys = deck_identity(result)
        print(f"deck {name}: rules {rules} keys {keys} artifacts {run.artifacts}")
        if expected is None:
            self.skipTest(
                f"deck {name} completed rejecting with keys {keys}; it declares"
                f" no expected_task_key for {task}"
            )
        self.assertIn(
            expected, keys, f"deck {name}: rules {rules}, not {task}'s key ({where})"
        )

    test.__doc__ = (
        f"Deck {name} is `expected: reject`; a completed run must reject with its"
        " `expected_task_key`, and one that does not complete must leave its"
        " `expected_execution`."
    )
    return test


def _control_lane(
    control: str, spec: RunSpec, rules: list[str]
) -> Callable[[DecksLiveTest], None]:
    def test(self: DecksLiveTest) -> None:
        run = self._run(f"red-control-{control}", spec, red_control=control)
        result, where = run.result, f"artifacts {run.artifacts}"
        self.assertEqual(
            result.execution.status, COMPLETED, f"{result.execution} {where}"
        )
        self.assertEqual(result.verdict.kind, REJECT, f"{result.verdict} {where}")
        report = result.verdict.report
        assert report is not None
        fired = sorted({f.rule_id for r in report.results for f in r.findings})
        self.assertEqual(rules, fired, f"{result.verdict} {where}")

    test.__doc__ = (
        f"Red control {control} trips exactly {rules} on the red-control deck."
    )
    return test


_LANES = {
    ACCEPT: _accept_lane,
    REJECT: _reject_lane,
    NOT_EVALUATED: _not_evaluated_lane,
}
WAITING = Waiting()


def _deck_lane(
    name: str, spec: RunSpec, waiting: Waiting = WAITING
) -> Callable[[DecksLiveTest], None]:
    """Return the deck's lane; a deck with `waiting_on` is a waiting case."""
    lane = _LANES[spec.metadata["expected"]](name, spec)
    task = spec.metadata.get(WAITING_ON)
    if task is None:
        return lane

    def test(self: DecksLiveTest) -> None:
        with waiting.case(self, f"deck {name}", task):
            lane(self)

    test.__doc__ = lane.__doc__
    return test


for _name, _spec in load_specs().items():
    setattr(
        DecksLiveTest,
        f"test_{_name.replace('-', '_')}",
        _deck_lane(_name, _spec),
    )
    for _control, _rules in _spec.metadata.get("expected_controls", {}).items():
        setattr(
            DecksLiveTest,
            f"test_red_control_{_control.replace('-', '_')}",
            _control_lane(_control, _spec, _rules),
        )


class LiveSkipPolicyTest(unittest.TestCase):
    """Only the pre-setup environment check may skip a lane; a later setup failure fails it."""

    def _lane(
        self, expected: str, waiting: Waiting | None = None
    ) -> Callable[[DecksLiveTest], None]:
        name = "red-control" if expected == "accept" else "bug-788"
        spec = load_specs()[name]
        if waiting is None:
            return _LANES[expected](name, spec)
        metadata = {**spec.metadata, WAITING_ON: "T1"}
        return _deck_lane(name, dataclasses.replace(spec, metadata=metadata), waiting)

    def _run_lane(
        self,
        expected: str,
        problem: str | None,
        setup_fails: bool,
        waiting: Waiting | None = None,
    ) -> None:
        def run_live(_spec: RunSpec, artifacts: str) -> ExplorationResult:
            if not setup_fails:
                return ExplorationResult(
                    Execution(COMPLETED, "ok", 1, 0), Verdict(ACCEPT, ""), artifacts
                )
            reason = "setup-failed: RuntimeError: mount failed"
            Path(artifacts, "stderr.log").write_text(f"runner: {reason}\n")
            return ExplorationResult(
                Execution(
                    HARNESS_FAILURE, f"runner-failed: runner exited 1: {reason}", 0, 1
                ),
                Verdict("reject", "no-modeled-operations/history≠spec"),
                artifacts,
            )

        with (
            mock.patch(
                f"{__name__}.live_environment_problem",
                return_value=problem,
            ),
            mock.patch(f"{__name__}.run_live", side_effect=run_live) as run,
        ):
            try:
                self._lane(expected, waiting)(DecksLiveTest())
            finally:
                self.run_count = run.call_count

    def test_a_waiting_deck_is_skipped_until_it_passes(self) -> None:
        waiting = Waiting()
        with self.assertRaisesRegex(unittest.SkipTest, "^waiting on T1$"):
            self._run_lane("accept", None, True, waiting)
        self.assertEqual({"T1": 1}, dict(waiting.counts))
        out = StringIO()
        with redirect_stdout(out):
            self._run_lane("accept", None, False, waiting)
        self.assertIn(
            "deck red-control now passes; remove the marker\n", out.getvalue()
        )
        self.assertEqual(1, waiting.total())

    def test_environment_check_failure_skips_before_setup(self) -> None:
        for expected in ("accept", "reject"):
            with self.subTest(expected=expected):
                with self.assertRaisesRegex(unittest.SkipTest, "no user DBus"):
                    self._run_lane(expected, "no user DBus session bus", False)
                self.assertEqual(0, self.run_count)

    def test_setup_failure_after_the_check_fails_every_lane(self) -> None:
        for expected in ("accept", "reject"):
            with self.subTest(expected=expected):
                with self.assertRaisesRegex(AssertionError, "setup-failed"):
                    self._run_lane(expected, None, True)

    def test_completed_run_with_a_ready_environment_does_not_skip(self) -> None:
        self._run_lane("accept", None, False)
        self.assertEqual(1, self.run_count)

    def test_environment_check_reports_missing_fuse_then_missing_bus(self) -> None:
        with mock.patch.object(backends_fixtures.os, "access", return_value=False):
            self.assertIn("no FUSE", live_environment_problem() or "")
        absent = {"DBUS_SESSION_BUS_ADDRESS": "unix:path=/nonexistent/bus"}
        with (
            mock.patch.object(backends_fixtures.os, "access", return_value=True),
            mock.patch.dict(os.environ, absent),
        ):
            self.assertEqual(
                "no user DBus session bus at /nonexistent/bus",
                live_environment_problem(),
            )

    def _completed_reject_lane(self, deck: str, kind: str) -> None:
        """Run a reject lane over a completed live run whose verdict is `kind`."""
        result = ExplorationResult(
            Execution(COMPLETED, "ok", 1, 0, setup={"fatal_eden_bug": False}),
            Verdict(kind, "other"),
            "artifacts",
        )
        with (
            mock.patch(f"{__name__}.live_environment_problem", return_value=None),
            mock.patch(f"{__name__}.run_live", return_value=result),
            mock.patch(f"{__name__}._setup_failure", return_value=None),
            mock.patch(f"{__name__}._artifact_directory") as directory,
        ):
            directory.return_value.__enter__.return_value = "artifacts"
            _LANES["reject"](deck, load_specs()[deck])(DecksLiveTest())

    def test_a_completed_reject_deck_needs_reject_and_its_task_key(self) -> None:
        for kind in (ACCEPT, NOT_EVALUATED, "harness-suspect", REJECT):
            with self.subTest(kind=kind):
                with self.assertRaisesRegex(AssertionError, "bug-788"):
                    self._completed_reject_lane("bug-788", kind)
        with self.assertRaisesRegex(AssertionError, "not reproduced"):
            self._completed_reject_lane("bug-775", NOT_EVALUATED)
        with self.assertRaisesRegex(unittest.SkipTest, "no expected_task_key"):
            self._completed_reject_lane("bug-775", REJECT)

    def test_a_run_recording_another_fatal_eden_bug_fails(self) -> None:
        result = ExplorationResult(
            Execution(COMPLETED, "ok", 1, 0, setup={"fatal_eden_bug": True}),
            Verdict(ACCEPT, ""),
            "artifacts",
        )
        with (
            mock.patch(f"{__name__}.live_environment_problem", return_value=None),
            mock.patch(f"{__name__}.run_live", return_value=result),
            mock.patch(f"{__name__}._setup_failure", return_value=None),
            mock.patch(f"{__name__}._artifact_directory") as directory,
        ):
            directory.return_value.__enter__.return_value = "artifacts"
            with self.assertRaisesRegex(AssertionError, "fatal_eden_bug"):
                _LANES["accept"]("red-control", load_specs()["red-control"])(
                    DecksLiveTest()
                )

    def _harness_failure(self, deck: str, timed_out: str | None) -> None:
        """Run a reject lane whose live run times out, writing what a real run leaves."""
        spec = load_specs()[deck]
        source = DECKS / f"{deck}.json"

        def run_live(_spec: RunSpec, artifacts: str) -> ExplorationResult:
            run_dir = Path(artifacts)
            (run_dir / "run-spec.json").write_text(source.read_text())
            events = [{"type": "invoke", "time": 1.0, "action": "w0", "actor": "a"}]
            if timed_out is not None:
                events.append(
                    {
                        "type": "info",
                        "time": 121.0,
                        "action": timed_out,
                        "reason": "operation-timeout",
                    }
                )
            (run_dir / "history.jsonl").write_text(
                "".join(json.dumps(e) + "\n" for e in events)
            )
            execution = Execution(
                HARNESS_FAILURE,
                "runner-failed: runner exited 1: operation-timeout",
                2,
                1,
                setup={"run_dir": "/tmp/exploratory.x"},
            )
            (run_dir / "execution.json").write_text(json.dumps(asdict(execution)))
            return ExplorationResult(execution, Verdict(REJECT, "x"), artifacts)

        with (
            tempfile.TemporaryDirectory() as tmp,
            mock.patch(f"{__name__}.live_environment_problem", return_value=None),
            mock.patch(f"{__name__}.run_live", side_effect=run_live),
            mock.patch(f"{__name__}._artifact_directory") as directory,
        ):
            directory.return_value.__enter__.return_value = tmp
            _LANES["reject"](deck, spec)(DecksLiveTest())

    def test_non_completed_reject_runs_fail_unless_their_execution_is_expected(
        self,
    ) -> None:
        self.assertEqual(
            ["execution.operation-timeout"],
            load_specs()["bug-775"].metadata["expected_execution"],
        )
        self._harness_failure("bug-775", "s1")
        with self.assertRaisesRegex(AssertionError, "did not complete"):
            self._harness_failure("bug-775", None)
        with self.assertRaisesRegex(AssertionError, "did not complete"):
            self._harness_failure("bug-788", "s1")

    def test_every_red_control_has_a_live_method(self) -> None:
        controls = load_specs()["red-control"].metadata["expected_controls"]
        self.assertEqual(sorted(EDENFS_TARGET.red_controls), sorted(controls))
        for control in controls:
            self.assertTrue(
                hasattr(DecksLiveTest, f"test_red_control_{control.replace('-', '_')}")
            )
