# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The CLI's own wiring: the differential campaign and generate flags compose their
specs, and the run, explore, and campaign verbs work end to end on the fake target.
"""

from __future__ import annotations

import json
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import mock

from eden.integration.correctness.exploratory import cli
from eden.integration.correctness.exploratory.cli import _main
from eden.integration.correctness.exploratory.core.checker import Verdict
from eden.integration.correctness.exploratory.core.containment import (
    Execution,
    ExplorationResult,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec, validate
from eden.integration.correctness.exploratory.core.test.testing import (
    run_in_fresh_interpreter,
)
from eden.integration.correctness.exploratory.edenfs.actions import reference_clone
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    with_reference_clone,
)
from eden.integration.correctness.exploratory.test.cli_fixtures import cli_argv


class CliTest(unittest.TestCase):
    def test_importing_the_cli_builds_no_target(self) -> None:
        """The target registry fills on first use, once; importing the CLI builds nothing."""
        code = (
            "from eden.integration.correctness.exploratory import targets\n"
            "built = []\n"
            "real = targets.target_entries\n"
            "targets.target_entries = lambda: built.append(1) or real()\n"
            "from eden.integration.correctness.exploratory import cli\n"
            "print(len(built), len(cli.TARGETS))\n"
            "cli._registry()\n"
            "cli._registry()\n"
            "print(len(built), sorted(cli.TARGETS) == sorted(real()))\n"
        )
        self.assertEqual("0 0\n1 True\n", run_in_fresh_interpreter(code, "0"))

    def test_differential_environment_is_the_plain_draw_opted_into_the_reference(
        self,
    ) -> None:
        """The default sampler never opts in; `--differential` reuses its draws."""
        for seed in range(40):
            default = cli._environment_target("edenfs", seed, 64)
            differential = cli._environment_target("edenfs", seed, 64, True)
            self.assertEqual(default, cli._environment_target("edenfs", seed, 64))
            self.assertNotIn("reference_clone", default["setup_profile"])
            self.assertEqual(
                with_reference_clone({**default, "case_sensitive": True}),
                differential,
            )
        with self.assertRaisesRegex(ValueError, "no differential mode"):
            cli._environment_target("fake", 3, 8, True)

    def test_differential_campaign_is_its_own_campaign_of_paired_specs(self) -> None:
        """Every seed composes a validator-admitted paired RunSpec; the ID names the flag."""
        ids = []
        composed: list[RunSpec] = []

        def runner(spec: RunSpec, path: str, *_rest: object) -> ExplorationResult:
            composed.append(spec)
            return ExplorationResult(
                Execution("completed", "ok", 1, 0), Verdict("accept", ""), path
            )

        with (
            tempfile.TemporaryDirectory() as tmp,
            mock.patch.object(cli, "_run_campaign_seed", runner),
        ):
            for extra in ([], ["--differential"]):
                report = Path(tmp) / f"report{len(ids)}.json"
                with redirect_stdout(StringIO()):
                    _main(
                        [
                            "campaign",
                            "--target",
                            "edenfs",
                            "--seeds",
                            "0" if not extra else "6",
                            "--artifacts",
                            str(Path(tmp) / f"a{len(ids)}"),
                            "--report",
                            str(report),
                            *extra,
                        ]
                    )
                ids.append(json.loads(report.read_text())["campaign_id"])
            with redirect_stderr(StringIO()) as err:
                code = _main(
                    cli_argv(
                        "campaign",
                        target="fake",
                        seeds=1,
                        artifacts=Path(tmp) / "fake",
                        differential=True,
                    )
                )
        self.assertNotEqual(ids[0], ids[1])
        self.assertEqual(2, code)
        self.assertIn("--differential needs a differential target", err.getvalue())
        self.assertEqual(6, len(composed))
        for spec in composed:
            self.assertTrue(reference_clone(spec))
            validate(spec, EDENFS_TARGET)
            self.assertFalse(
                [a for a in spec.actions if a.kind.startswith("fault_")], spec.seed
            )

    def test_cli_campaign_plans_master_reachable_sites_by_default(self) -> None:
        """Without `--daemon`, campaign strata cover public-master sites only."""
        with tempfile.TemporaryDirectory() as artifact_dir:
            with redirect_stdout(StringIO()):
                code = _main(
                    cli_argv(
                        "campaign", target="edenfs", seeds=0, artifacts=artifact_dir
                    )
                )
            self.assertEqual(1, code)
            with open(os.path.join(artifact_dir, "strata.json")) as handle:
                strata = json.load(handle)
            observed = {tuple(row["stratum"]) for row in strata["observable"]}
            expected = {
                (site_name, behavior, kill_mode)
                for site_name, site in EDENFS_TARGET.sites.items()
                if site.daemon == "public-master"
                for behavior in site.behaviors
                for kill_mode in ("none", "process")
            }
            self.assertEqual(expected, observed)

    def test_generate_stratum_flag(self) -> None:
        """`--stratum SITE/BEHAVIOR/KILL` forces the emitted fault; others are errors."""
        code, stdout, _stderr = self._generate("tree-checkout/block/none")
        self.assertEqual(0, code)
        actions = json.loads(stdout)["actions"].values()
        install = next(a for a in actions if a["kind"] == "fault_install")
        self.assertEqual(
            ("tree-checkout", "block"), (install["site"], install["behavior"])
        )
        self.assertFalse({a["kind"] for a in actions} & {"kill_process"})
        for rejected in ("checkout-begin/kill/none", "x/block/daemon"):
            code, _stdout, stderr = self._generate(rejected)
            self.assertEqual(2, code, rejected)
            self.assertIn(rejected, stderr)

    def test_generate_stratum_on_excluded_site_names_the_daemon_label(self) -> None:
        """A stack-build site is a CLI error by default; `--daemon stack-build` admits it."""
        stratum = "fuse-lookup/block/none"
        code, stdout, stderr = self._generate(stratum)
        self.assertEqual((2, ""), (code, stdout))
        self.assertIn("is not admitted on daemon 'public-master'", stderr)
        code, stdout, stderr = self._generate(stratum, "--daemon", "stack-build")
        self.assertEqual(0, code, stderr)

    def _generate(self, stratum: str, *extra: str) -> tuple[int, str, str]:
        argv = cli_argv("generate", target="edenfs", seed=5, stratum=stratum)
        with redirect_stdout(StringIO()) as out, redirect_stderr(StringIO()) as err:
            code = _main([*argv, *extra])
        return code, out.getvalue(), err.getvalue()

    def test_cli_run_fake_end_to_end(self) -> None:
        spec = {
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
                "r": {
                    "kind": "read",
                    "deadline": "operation",
                    "path": "register",
                },
            },
        }
        with tempfile.TemporaryDirectory() as tmp:
            spec_path = os.path.join(tmp, "spec.json")
            artifact_dir = os.path.join(tmp, "artifacts")
            with open(spec_path, "w") as handle:
                json.dump(spec, handle)
            with redirect_stdout(StringIO()):
                code = _main(
                    cli_argv(
                        "run", target="fake", spec=spec_path, artifacts=artifact_dir
                    )
                )
            self.assertEqual(code, 0)
            self.assertTrue(os.path.isfile(os.path.join(artifact_dir, "verdict.json")))

    def test_cli_explore_fake_end_to_end(self) -> None:
        with tempfile.TemporaryDirectory() as artifact_dir:
            with redirect_stdout(StringIO()):
                code = _main(
                    cli_argv(
                        "explore",
                        target="fake",
                        seeds=2,
                        seed_start=20,
                        files=8,
                        artifacts=artifact_dir,
                    )
                )
            self.assertEqual(code, 0)
            with open(os.path.join(artifact_dir, "summary.jsonl")) as handle:
                rows = [json.loads(line) for line in handle]
            self.assertEqual([20, 21], [row["seed"] for row in rows])
            self.assertTrue(all(row["shape_signature"] for row in rows))

    def test_cli_campaign_fake_end_to_end(self) -> None:
        with tempfile.TemporaryDirectory() as artifact_dir:
            with redirect_stdout(StringIO()):
                code = _main(
                    cli_argv(
                        "campaign",
                        target="fake",
                        seeds=2,
                        seed_start=30,
                        files=8,
                        artifacts=artifact_dir,
                    )
                )
            self.assertEqual(code, 0)
            for name in ("summary.jsonl", "strata.json", "report.json"):
                self.assertTrue(os.path.isfile(os.path.join(artifact_dir, name)))
            with open(os.path.join(artifact_dir, "report.json")) as handle:
                report = json.load(handle)
            self.assertEqual(2, report["seeds"])
            self.assertEqual([], report["strata"]["missing"])
            with open(os.path.join(artifact_dir, "strata.json")) as handle:
                strata = json.load(handle)
            self.assertEqual([], strata["excluded"])
            admissions = [
                json.loads(path.read_text())
                for path in Path(artifact_dir).glob("seed-*/admission.json")
            ]
            self.assertEqual(2, len(admissions))
            for admission in admissions:
                self.assertEqual("explore", admission["run_purpose"])
