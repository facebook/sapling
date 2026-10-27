# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Running a campaign: every receipt records the daemon's provenance and source age, the
campaign stops after repeated identical harness failures or a signal exit, forces
every planned row, keeps only rejected seeds when asked, and a broken history tail
keeps the report and the admission.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.cli import (
    _campaign_provenance,
    _main,
    _write_admission,
    BINARY_ENV,
)
from eden.integration.correctness.exploratory.core import containment
from eden.integration.correctness.exploratory.core.campaign import (
    admitted_strata,
    CampaignReport,
    KEEP_REJECTS,
    plan,
    retain,
    run_campaign,
)
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    CONTAINMENT_TIMEOUT,
    ENVIRONMENT_DEGRADED,
    ExplorationResult,
    HARNESS_FAILURE,
)
from eden.integration.correctness.exploratory.core.generate import (
    compose,
    derived_rng,
    generation_target,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.fake.target import (
    FAKE,
    sample_environment,
)
from eden.integration.correctness.exploratory.schemas import load_admission, RunPurpose
from eden.integration.correctness.exploratory.targets import target_entries
from eden.integration.correctness.exploratory.test.cli_fixtures import (
    cli_argv,
    fake_spec,
)
from eden.integration.correctness.exploratory.test.tasking_fixtures import (
    exploration_result,
)


# The CLI's EdenFS entry; generation, strata, and sites need only its vocabulary.
EDENFS_TARGET = target_entries()["edenfs"].target


def _campaign_arguments(root: Path) -> list[str]:
    return cli_argv(
        "campaign",
        target="edenfs",
        seeds=2,
        files=32,
        artifacts=root / "artifacts",
        report=root / "report.json",
    )


class CampaignRunTest(unittest.TestCase):
    def test_campaign_refuses_a_non_empty_artifact_dir(self) -> None:
        runner = mock.Mock()
        with tempfile.TemporaryDirectory() as artifact_dir:
            Path(artifact_dir, "summary.jsonl").write_text("")
            with self.assertRaisesRegex(ValueError, "is not empty"):
                run_campaign(
                    FAKE, 1, 1, lambda _seed: {}, artifact_dir, runner, "label"
                )
        runner.assert_not_called()

    def test_campaign_records_daemon_provenance_in_every_receipt(self) -> None:
        """Completed and failed attempts identify the daemon `EDENFS_SERVER_PATH` names."""
        for runner_result in (0, 3, OSError("runner unavailable")):
            with self.subTest(runner_result=runner_result):
                captured = self._daemon_campaign(60, runner_result)
                expected = {
                    "daemon_sha256": hashlib.sha256(b"actual daemon").hexdigest(),
                    "daemon_source_commit": "a" * 40,
                    "fatal_eden_bug": False,
                    "daemon_log": None,
                }
                for document in (captured["report"], captured["aggregate"]):
                    for field, value in expected.items():
                        self.assertEqual(value, document[field])
                for execution, admission in zip(
                    captured["executions"], captured["admissions"]
                ):
                    self.assertFalse(set(expected) & set(execution))
                    self.assertEqual(
                        {
                            "binary_sha256": expected["daemon_sha256"],
                            "source_commit": expected["daemon_source_commit"],
                            "fatal_eden_bug": False,
                            "log": None,
                        },
                        {
                            key: admission["daemon"][key]
                            for key in (
                                "binary_sha256",
                                "source_commit",
                                "fatal_eden_bug",
                                "log",
                            )
                        },
                    )
                    self.assertEqual(
                        ("explore", captured["report"]["campaign_id"]),
                        (admission["run_purpose"], admission["campaign_id"]),
                    )
                self.assertEqual(
                    f"sha256:{expected['daemon_sha256']}",
                    captured["report"]["daemon_identity"],
                )
                self.assertEqual(2, captured["spawn_count"])

    def test_campaign_daemon_source_age_boundary(self) -> None:
        """Only a source commit strictly older than seven days degrades a campaign."""
        for age, degraded in ((604799, False), (604800, False), (604801, True)):
            with self.subTest(age=age):
                captured = self._daemon_campaign(age)
                count = 2 if degraded else 0
                self.assertEqual(2 - count, captured["spawn_count"])
                self.assertEqual(count, captured["report"]["environment_degraded"])
                self.assertEqual(count, captured["aggregate"]["environment_degraded"])
                self.assertEqual(2 - count, captured["aggregate"]["completed"])
                self.assertEqual(0, captured["aggregate"]["harness_failures"])
                for execution, summary in zip(
                    captured["executions"], captured["summary"]
                ):
                    expected = ENVIRONMENT_DEGRADED if degraded else COMPLETED
                    self.assertEqual(expected, execution["status"])
                    self.assertEqual(expected, summary["status"])
                    self.assertEqual(execution["reason"], summary["reason"])
                    if degraded:
                        self.assertIn("older than 7 days", execution["reason"])
                        self.assertIsNone(execution["returncode"])
                # A spawned runner's history holds only the supervisor's run-closed.
                for replay in captured["replays"]:
                    self.assertEqual(
                        [] if degraded else ["run-closed"],
                        [event["type"] for event in replay["history"]],
                    )

    def test_campaign_report_lists_every_supplied_binary_with_its_sha256(
        self,
    ) -> None:
        captured = self._daemon_campaign(60)
        binaries = captured["report"]["binaries"]
        self.assertEqual(set(BINARY_ENV), set(binaries))
        self.assertEqual(
            hashlib.sha256(b"actual daemon").hexdigest(),
            binaries["edenfs"]["sha256"],
        )
        for name in set(BINARY_ENV) - {"edenfs"}:
            self.assertEqual(
                hashlib.sha256(name.encode()).hexdigest(), binaries[name]["sha256"]
            )
        with tempfile.TemporaryDirectory() as temporary:
            daemon = Path(temporary) / "daemon"
            daemon.write_bytes(b"actual daemon")
            environment = {
                "EDENFS_SERVER_PATH": str(daemon),
                "EDENFS_DAEMON_SOURCE_COMMIT": "a" * 40,
                "EDENFS_DAEMON_SOURCE_TIMESTAMP": "0",
                "EDENFS_DAEMON_FATAL_EDEN_BUG": "false",
            }
            with mock.patch.dict(os.environ, environment):
                for name, env in BINARY_ENV.items():
                    if name != "edenfs":
                        os.environ.pop(env, None)
                provenance, _ = _campaign_provenance("edenfs")
        self.assertEqual({"path": None, "sha256": None}, provenance["binaries"]["sl"])

    def test_campaign_id_names_the_fatal_eden_bug_fact(self) -> None:
        """Campaigns that differ only in `fatal_eden_bug` never share an ID."""
        ids = {
            fatal: self._daemon_campaign(60, fatal=fatal)["report"]["campaign_id"]
            for fatal in ("false", "true")
        }
        self.assertNotEqual(ids["false"], ids["true"])

    def test_malformed_provenance_exits_2_before_any_run(self) -> None:
        """Provenance is checked once, as the admission reader checks it, before seed 0."""
        cases = {
            "EDENFS_DAEMON_FATAL_EDEN_BUG": "1",
            "EDENFS_DAEMON_SOURCE_TIMESTAMP": "NaN",
            "EDENFS_DAEMON_SOURCE_COMMIT": "a" * 5000,
            "EDENFS_SERVER_PATH": "/nonexistent/edenfs",
        }
        for variable, value in cases.items():
            with self.subTest(variable=variable):
                with self.assertRaises(SystemExit) as raised:
                    self._daemon_campaign(60, overrides={variable: value})
                self.assertEqual(
                    (2, 0), (raised.exception.args[0], raised.exception.args[1])
                )

    def _daemon_campaign(
        self,
        source_age: float,
        runner_result: int | OSError = 0,
        fatal: str = "false",
        overrides: dict[str, str] | None = None,
    ) -> dict[str, Any]:
        """Exercise real campaign accounting while substituting only its runner process."""
        original_popen = subprocess.Popen

        def spawn_runner(argv: list[str], **kwargs: Any) -> subprocess.Popen[str]:
            if isinstance(runner_result, OSError):
                raise runner_result
            return original_popen(
                [sys.executable, "-c", f"raise SystemExit({runner_result})"], **kwargs
            )

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            daemon = root / "daemon"
            daemon.write_bytes(b"actual daemon")
            environment = {
                "EDENFS_SERVER_PATH": str(daemon),
                "EDENFS_DAEMON_SOURCE_COMMIT": "a" * 40,
                "EDENFS_DAEMON_SOURCE_TIMESTAMP": str(1800000000 - source_age),
                "EDENFS_DAEMON_FATAL_EDEN_BUG": fatal,
            }
            for name, env in BINARY_ENV.items():
                if name != "edenfs":
                    (root / name).write_bytes(name.encode())
                    environment[env] = str(root / name)
            environment.update(overrides or {})
            with (
                mock.patch.dict(os.environ, environment),
                mock.patch("time.time", return_value=1800000000),
                mock.patch.object(
                    containment.subprocess, "Popen", side_effect=spawn_runner
                ) as spawn,
            ):
                code = _main(_campaign_arguments(root))
                if code == 2:
                    raise SystemExit(code, spawn.call_count)
            captured = self._campaign_receipts(root)
            captured["spawn_count"] = spawn.call_count
            return captured

    def _campaign_receipts(self, root: Path) -> dict[str, Any]:
        """Read both reports and every seed's durable artifact set."""
        artifacts = root / "artifacts"
        captured: dict[str, Any] = {
            "report": json.loads((root / "report.json").read_text()),
            "aggregate": json.loads((artifacts / "report.json").read_text()),
            "summary": [
                json.loads(line)
                for line in (artifacts / "summary.jsonl").read_text().splitlines()
            ],
            "executions": [],
            "admissions": [],
            "replays": [],
        }
        for seed in (0, 1):
            seed_dir = artifacts / f"seed-{seed}"
            for name in (
                "run-spec.json",
                "history.jsonl",
                "execution.json",
                "verdict.json",
                "replay.json",
                "stderr.log",
            ):
                self.assertTrue((seed_dir / name).is_file(), name)
            captured["executions"].append(
                json.loads((seed_dir / "execution.json").read_text())
            )
            captured["replays"].append(
                json.loads((seed_dir / "replay.json").read_text())
            )
            captured["admissions"].append(
                json.loads((seed_dir / "admission.json").read_text())
            )
        return captured

    @mock.patch("eden.integration.correctness.exploratory.cli.run_campaign")
    def test_cli_report_records_early_stop(self, run_campaign: mock.Mock) -> None:
        """The optional CLI report preserves campaign stop provenance."""
        run_campaign.return_value = CampaignReport(
            attempts=1,
            completed=0,
            harness_failures=1,
            environment_degraded=0,
            verdicts={"reject": 1},
            completed_strata={},
            missing_strata=(),
            rejects=(),
            stopped={
                "reason": "signal",
                "seed": 361000,
                "signature": ["runner-failed: runner exited -11", "reject", "boom"],
            },
        )
        with tempfile.TemporaryDirectory() as temporary:
            report_path = Path(temporary) / "report.json"
            _main(
                cli_argv(
                    "campaign",
                    target="fake",
                    seeds=1,
                    artifacts=Path(temporary) / "artifacts",
                    report=report_path,
                )
            )
            report = json.loads(report_path.read_text())
        self.assertEqual(run_campaign.return_value.stopped, report["stopped"])

    def test_campaign_stops_after_repeated_identical_harness_failures(self) -> None:
        """Repeated harness failures indicate a harness defect, not a known shape."""
        target = generation_target(EDENFS_TARGET)
        environment = {
            "repository": {
                "file_count": 32,
                "commit_count": 4,
                "registers": ["tree/registers/r0.reg", "tree/registers/r1.reg"],
            },
            "channel": "fuse",
            "case_sensitive": True,
            "fixture_source": "backing",
            "setup_profile": {"materialize": False, "scatter_dirty": 0},
        }

        with tempfile.TemporaryDirectory() as artifact_dir:
            report = run_campaign(
                target,
                55,
                361000,
                lambda _seed: environment,
                artifact_dir,
                lambda _spec, path, _target: exploration_result(
                    HARNESS_FAILURE,
                    "reject",
                    "no-modeled-operations/history≠spec",
                    path,
                    execution_reason="runner-failed: runner exited 1: fault-witness-failed",
                ),
                "public-master",
            )
            stopped = json.loads((Path(artifact_dir) / "report.json").read_text())[
                "stopped"
            ]

        self.assertEqual(5, report.attempts)
        self.assertEqual(5, report.harness_failures)
        self.assertEqual("streak", stopped["reason"])
        self.assertEqual(361004, stopped["seed"])
        self.assertEqual(
            [
                "runner-failed: runner exited 1: fault-witness-failed",
                "reject",
                "no-modeled-operations/history≠spec",
            ],
            stopped["signature"],
        )

    def test_campaign_stops_after_signal_exit(self) -> None:
        """A runner signal exit is a harness defect and stops diagnosis."""
        target = generation_target(EDENFS_TARGET)
        environment = {
            "repository": {
                "file_count": 32,
                "commit_count": 4,
                "registers": ["tree/registers/r0.reg", "tree/registers/r1.reg"],
            },
            "channel": "fuse",
            "case_sensitive": True,
            "fixture_source": "backing",
            "setup_profile": {"materialize": False, "scatter_dirty": 0},
        }

        with tempfile.TemporaryDirectory() as artifact_dir:
            report = run_campaign(
                target,
                55,
                361000,
                lambda _seed: environment,
                artifact_dir,
                lambda _spec, path, _target: exploration_result(
                    HARNESS_FAILURE,
                    "reject",
                    "no-modeled-operations/history≠spec",
                    path,
                    execution_reason="runner-failed: runner exited -11",
                    returncode=-11,
                ),
                "public-master",
            )
            stopped = json.loads((Path(artifact_dir) / "report.json").read_text())[
                "stopped"
            ]

        self.assertEqual(1, report.attempts)
        self.assertEqual(1, report.harness_failures)
        self.assertEqual(
            {
                "reason": "signal",
                "seed": 361000,
                "signature": [
                    "runner-failed: runner exited -11",
                    "reject",
                    "no-modeled-operations/history≠spec",
                ],
            },
            stopped,
        )

    def test_run_admission_names_the_red_control_purpose(self) -> None:
        """`run --red-control` admits a red-control run; a plain `run` is manual."""
        control = sorted(FAKE.red_controls)[0]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            spec = root / "spec.json"
            spec.write_text(fake_spec())
            purposes = {}
            for name, red_control in (("plain", None), ("control", control)):
                with redirect_stdout(StringIO()):
                    _main(
                        cli_argv(
                            "run",
                            target="fake",
                            spec=spec,
                            artifacts=root / name,
                            red_control=red_control,
                        )
                    )
                purposes[name] = load_admission(root / name).run_purpose
        self.assertEqual(
            {"plain": RunPurpose.MANUAL, "control": RunPurpose.RED_CONTROL}, purposes
        )

    def test_a_containment_timeout_never_stops_the_campaign(self) -> None:
        """The supervisor's own SIGKILL of a timed-out runner is not a harness defect."""
        with tempfile.TemporaryDirectory() as artifact_dir:
            report = run_campaign(
                FAKE,
                3,
                0,
                lambda seed: dict(
                    sample_environment(derived_rng(seed, "exploratory-environment"))
                ),
                artifact_dir,
                lambda _spec, path, _target: exploration_result(
                    CONTAINMENT_TIMEOUT,
                    "accept",
                    "",
                    path,
                    execution_reason="runner exceeded 600s",
                    returncode=-9,
                ),
                "public-master",
                no_fault=True,
            )
        self.assertEqual((3, None), (report.attempts, report.stopped))

    @mock.patch("eden.integration.correctness.exploratory.cli.run_campaign")
    def test_campaign_exits_1_unless_every_attempt_completed_and_accepted(
        self, run_campaign: mock.Mock
    ) -> None:
        """The campaign exit follows the run rule: completed and accept, every attempt."""
        for verdicts, code in (
            ({"accept": 2}, 0),
            ({"not-evaluated": 2}, 1),
            ({"accept": 1, "harness-suspect": 1}, 1),
        ):
            with self.subTest(verdicts=verdicts):
                run_campaign.return_value = CampaignReport(
                    attempts=2,
                    completed=2,
                    harness_failures=0,
                    environment_degraded=0,
                    verdicts=verdicts,
                    completed_strata={},
                    missing_strata=(),
                    rejects=(),
                )
                with tempfile.TemporaryDirectory() as temporary:
                    argv = cli_argv(
                        "campaign",
                        target="fake",
                        seeds=2,
                        artifacts=Path(temporary) / "artifacts",
                    )
                    with redirect_stdout(StringIO()):
                        self.assertEqual(code, _main(argv))

    def test_campaign_forces_every_planned_row(
        self,
    ) -> None:
        target = generation_target(EDENFS_TARGET)
        strata = admitted_strata(target, "public-master")
        environment = {
            "repository": {
                "file_count": 32,
                "commit_count": 4,
                "registers": ["tree/registers/r0.reg", "tree/registers/r1.reg"],
            },
            "channel": "fuse",
            "case_sensitive": True,
            "fixture_source": "backing",
            "setup_profile": {"materialize": False, "scatter_dirty": 0},
        }

        def runner(spec, artifacts, target):
            self.assertEqual(target.name, spec.target)
            return exploration_result(COMPLETED, "accept", "ok", artifacts)

        with (
            tempfile.TemporaryDirectory() as artifact_dir,
            mock.patch(
                "eden.integration.correctness.exploratory.core.campaign.compose",
                wraps=compose,
            ) as compose_call,
        ):
            report = run_campaign(
                target,
                len(strata) + 1,
                500,
                lambda _seed: environment,
                artifact_dir,
                runner,
                "public-master",
            )

        self.assertEqual(len(strata) + 1, report.attempts)
        planned = plan(target, 500, len(strata) + 1, "public-master")
        for call, entry in zip(compose_call.call_args_list, planned):
            self.assertEqual(entry.stratum, call.kwargs["stratum"])
            self.assertTrue(call.kwargs.get("forced", False))

    @mock.patch.object(containment, "run_exploration")
    def test_keep_rejects_prunes_accept_stderr_and_relocates_other_seeds(
        self, run: mock.Mock
    ) -> None:
        """`--keep rejects` leaves accepts without `stderr.log` and moves every other seed, harness failures included, under `rejects/`."""

        def explore(spec: RunSpec, artifact_dir: str, **_: Any) -> ExplorationResult:
            Path(artifact_dir).mkdir(parents=True)
            for name in ("run-spec.json", "stderr.log"):
                (Path(artifact_dir) / name).write_text("")
            if spec.seed == 2:
                return exploration_result(
                    HARNESS_FAILURE, "not-evaluated", "harness", artifact_dir
                )
            verdict = "accept" if spec.seed == 0 else "reject"
            return exploration_result(COMPLETED, verdict, "ok", artifact_dir)

        run.side_effect = explore
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = root / "artifacts"
            with redirect_stdout(StringIO()):
                _main(
                    cli_argv(
                        "campaign",
                        target="fake",
                        seeds=3,
                        artifacts=artifacts,
                        report=root / "report.json",
                        keep="rejects",
                    )
                )
            self.assertEqual(
                ["admission.json", "run-spec.json"],
                sorted(p.name for p in (artifacts / "seed-0").iterdir()),
            )
            self.assertFalse((artifacts / "seed-1").exists())
            moved = artifacts / "rejects" / "seed-1"
            self.assertTrue((moved / "stderr.log").is_file())
            harness = artifacts / "rejects" / "seed-2"
            self.assertTrue((harness / "stderr.log").is_file())
            summary = [
                json.loads(line)
                for line in (artifacts / "summary.jsonl").read_text().splitlines()
            ]
            self.assertEqual(
                ["seed-0", "rejects/seed-1", "rejects/seed-2"],
                [row["artifacts"] for row in summary],
            )
            aggregate = json.loads((artifacts / "report.json").read_text())
            self.assertEqual(str(moved), aggregate["rejects"][0]["artifacts"])


class RetainTest(unittest.TestCase):
    def test_an_accepted_seed_without_stderr_stays_in_place(self) -> None:
        """`--keep rejects` on an accepted seed whose `stderr.log` is already gone."""
        with tempfile.TemporaryDirectory() as temporary:
            seed_dir = Path(temporary) / "seed-0"
            seed_dir.mkdir()
            result = exploration_result(COMPLETED, "accept", "ok", str(seed_dir))
            self.assertEqual(str(seed_dir), retain(str(seed_dir), result, KEEP_REJECTS))
            self.assertTrue(seed_dir.is_dir())


class TornHistoryTest(unittest.TestCase):
    """A broken history tail keeps the campaign report and the admission."""

    RUNNERS = {
        "torn record": b'{"type": "invoke", "act',
        "invalid UTF-8": b"\xff\xfe not utf8\n",
        "large torn record": b'{"type": "info", "time": 1, "text": "' + b"x" * 100_000,
        "stray output": b"warning: something printed to stdout\n",
    }

    def test_campaign_report_and_admission_survive_a_broken_tail(self) -> None:
        for label, output in self.RUNNERS.items():
            runner = [
                sys.executable,
                "-c",
                f"import sys; sys.stdout.buffer.write({output!r}); sys.stdout.flush()",
            ]

            def runner(
                spec: RunSpec, path: str, target: Any, argv: list[str] = runner
            ) -> ExplorationResult:
                return containment.run_exploration(
                    spec, path, target=target, runner_argv=argv
                )

            with self.subTest(runner=label), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                report = run_campaign(
                    FAKE,
                    1,
                    0,
                    lambda seed: dict(
                        sample_environment(derived_rng(seed, "exploratory-environment"))
                    ),
                    str(root),
                    runner,
                    "public-master",
                    no_fault=True,
                )
                seed = root / "seed-0"
                execution = json.loads((seed / "execution.json").read_text())
                self.assertEqual(HARNESS_FAILURE, execution["status"])
                self.assertTrue(
                    execution["reason"].startswith("broken-history-transport:")
                )
                self.assertTrue((root / "report.json").is_file())
                self.assertEqual(1, report.harness_failures)
                rows = (root / "summary.jsonl").read_text().splitlines()
                self.assertEqual(1, len(rows))
                self.assertEqual(
                    [], containment.durable_history(seed / "history.jsonl")
                )
                _write_admission(seed, RunPurpose.EXPLORE, "c", {}, None)
                admission = load_admission(seed)
                self.assertEqual(RunPurpose.EXPLORE, admission.run_purpose)
                self.assertEqual("c", admission.campaign_id)
