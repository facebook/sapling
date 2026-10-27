# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The nightly job definition: it runs at master with a daemon built from master, runs
each campaign's own red controls, writes each campaign's findings report, never files
or contacts a store, and provides only rejecting seeds and report artifacts.
"""

from __future__ import annotations

import io
import json
import re
import subprocess
import tempfile
import unittest
from collections import Counter
from contextlib import redirect_stdout
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory.cli import _main, _registry, BINARY_ENV
from eden.integration.correctness.exploratory.core import runspec
from eden.integration.correctness.exploratory.core.campaign import (
    admitted_strata,
    plan,
    WITNESS_FLOOR,
)
from eden.integration.correctness.exploratory.core.sites import PUBLIC_MASTER
from eden.integration.correctness.exploratory.test.cli_fixtures import cli_argv, run_cli

OUTPUT = "edenfs-exploratory-nightly/"
DECKS = Path(__file__).parents[2] / "decks"
CAMPAIGNS = ("fixed", "rotating", "differential")
TARGET_VERBS = frozenset({"campaign", "run", "findings", "red-controls"})


def _spec(name: str) -> dict[str, Any]:
    return json.loads((Path(__file__).parent.with_name(name)).read_text())


def _recorded_commands(
    shell: str, output: Path, red_controls: str = "", failing: str = ""
) -> list[list[str]]:
    """Run the job's campaign section with `explore` replaced by an argv recorder.

    The recorded campaign writes a report naming campaign `id-<dir name>`;
    every verb named in `failing` exits 1 after it is recorded.
    """
    section = shell[shell.index("rotating_start=") :]
    output.mkdir(parents=True, exist_ok=True)
    script = (
        "set -euo pipefail\n"
        f"output={output}\n"
        f"log={output}/argv.log\n"
        f"red_controls='{red_controls}'\n"
        f"failing=' {failing} '\n"
        "red_control_spec=red-control.json\n"
        'record() { printf "%s\\x1f" "$@" >> "$log"; printf "\\n" >> "$log"; '
        'if [ "$1" = campaign ]; then '
        "for ((i = 1; i <= $#; i++)); do "
        'if [ "${!i}" = --report ]; then j=$((i + 1)); '
        'mkdir -p "$(dirname "${!j}")"; '
        'name=$(basename "$(dirname "${!j}")"); '
        'echo "{\\"campaign_id\\": \\"id-$name\\"}" > "${!j}"; fi; done; fi; '
        'case "$failing" in *" $1 "*) return 1;; esac; }\n'
        "explore=record\n"
        f"{section}\n"
    )
    subprocess.run(["bash", "-c", script], check=True, capture_output=True)
    lines = (output / "argv.log").read_text().splitlines()
    return [line.split("\x1f")[:-1] for line in lines]


class NightlyTest(unittest.TestCase):
    def setUp(self) -> None:
        self.spec = _spec("nightly.json")
        self.steps = self.spec["args"]["steps"]
        self.shell = self.steps[0]["shell"]
        self.emit = self.steps[1]["shell"]

    def test_job_is_pinned_to_master_and_validates_its_shell(self) -> None:
        for shell in (self.shell, self.emit):
            subprocess.run(["bash", "-n"], input=shell, text=True, check=True)
        self.assertEqual("master", self.spec["hash"])
        for needle in ("--target edenfs", "--daemon public-master", "--keep rejects"):
            self.assertIn(needle, self.shell)
        self.assertNotIn("wrapper", self.shell)
        self.assertNotIn("EDEN_TEST_CLEANUP", self.shell)

    def test_daemon_is_built_at_master_before_the_harness_runs_at_the_job_hash(
        self,
    ) -> None:
        """Source commit and build happen on `remote/master`; the harness then runs at the job's own hash."""
        goto_master = self.shell.index("sl goto remote/master")
        source = self.shell.index("EDENFS_DAEMON_SOURCE_COMMIT=")
        build = self.shell.index(
            "buck2 build @fbcode//mode/opt fbcode//eden/fs/service:edenfs"
        )
        goto_back = self.shell.index('sl goto "$job_commit"')
        self.assertLess(self.shell.index("job_commit=$(sl"), goto_master)
        self.assertLess(goto_master, source)
        self.assertLess(source, build)
        self.assertLess(build, goto_back)
        self.assertLess(goto_back, self.shell.index("explore="))
        self.assertIn("EDENFS_DAEMON_SOURCE_TIMESTAMP=", self.shell)
        self.assertIn(
            "EDENFS_SERVER_PATH=$(built fbcode//eden/fs/service:edenfs)", self.shell
        )
        self.assertNotIn("EDENFS_DAEMON_BINARY", self.shell)

    def test_every_reported_binary_is_built_at_master_and_exported(self) -> None:
        """Each binary the report hashes is named by the job: built at master, or the setuid privhelper."""
        start = self.shell.index("build_output=$(mktemp)")
        block = self.shell[start : self.shell.index('sl goto "$job_commit"')]
        self.assertLess(self.shell.index("sl goto remote/master"), start)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            outputs = []
            for target in re.findall(r"fbcode//\S+:\S+", block.splitlines()[1]):
                binary = root / target.replace("/", "_").replace(":", "_")
                binary.write_text("")
                binary.chmod(0o755)
                outputs.append(f"{target} {binary}")
            privhelper = root / "privhelper"
            privhelper.write_text("")
            privhelper.chmod(0o755)
            (root / "buck2").write_text(
                "#!/bin/bash\nprintf '%s\\n' " + " ".join(map(repr, outputs)) + "\n"
            )
            (root / "buck2").chmod(0o755)
            script = (
                f"set -euo pipefail\nfatal_eden_bug=false\nexport PATH={root}:$PATH\n"
                + block.replace(
                    "/usr/local/libexec/eden/edenfs_privhelper", str(privhelper)
                )
                + "".join(
                    f'printf "%s=%s\\n" {e} "${e}"\n' for e in BINARY_ENV.values()
                )
            )
            out = subprocess.run(
                ["bash", "-c", script], capture_output=True, text=True, check=True
            ).stdout
        exported = dict(line.split("=", 1) for line in out.splitlines())
        self.assertEqual(set(BINARY_ENV.values()), set(exported))
        for name, env in BINARY_ENV.items():
            self.assertTrue(exported[env].startswith(tmp), (name, exported[env]))
        self.assertEqual(exported["EDEN_HG_BINARY"], exported["HG_REAL_BIN"])

    def test_fatal_eden_bug_comes_from_a_capability_probe(self) -> None:
        probe = self.shell.index(
            "grep -q fatal_eden_bug fbcode/eden/fs/service/EdenMain.cpp"
        )
        export = self.shell.index(
            'export EDENFS_DAEMON_FATAL_EDEN_BUG="$fatal_eden_bug"'
        )
        self.assertLess(probe, export)
        self.assertLess(export, self.shell.index("explore="))
        self.assertLess(probe, self.shell.index('sl goto "$job_commit"'))
        for literal in ("FATAL_EDEN_BUG=false", "FATAL_EDEN_BUG=true"):
            self.assertNotIn(literal, self.shell)

    def test_campaigns_report_findings_after_red_controls(self) -> None:
        self.assertLess(
            self.shell.index("red-controls --target edenfs"),
            self.shell.index("run_campaign fixed"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            commands = _recorded_commands(self.shell, Path(tmp))
        verbs = [command[0] for command in commands]
        self.assertEqual(["campaign", "findings"] * 3 + ["run"], verbs)
        for index, name in enumerate(CAMPAIGNS):
            campaign, findings = commands[2 * index : 2 * index + 2]
            self.assertEqual(name == "differential", "--differential" in campaign)
            self.assertIn(f"{tmp}/campaigns/{name}/report.json", campaign)
            self.assertIn(f"{tmp}/campaigns/{name}/report.json", findings)

    def test_each_campaign_runs_its_own_red_controls_bound_to_its_id(self) -> None:
        """A shared control set would let one execution speak for every campaign."""
        with tempfile.TemporaryDirectory() as tmp:
            commands = _recorded_commands(
                self.shell, Path(tmp), red_controls="torn-write status-loss"
            )
        verbs = [command[0] for command in commands]
        self.assertEqual(["campaign", "run", "run", "findings"] * 3 + ["run"], verbs)
        for index, name in enumerate(CAMPAIGNS):
            runs = commands[4 * index + 1 : 4 * index + 3]
            findings = commands[4 * index + 3]
            for control, run in zip(("torn-write", "status-loss"), runs):
                self.assertEqual(control, run[run.index("--red-control") + 1])
                self.assertEqual(f"id-{name}", run[run.index("--campaign-id") + 1])
                self.assertEqual(
                    f"{tmp}/campaigns/{name}/red-controls/{control}",
                    run[run.index("--artifacts") + 1],
                )
            self.assertEqual(
                f"{tmp}/campaigns/{name}/red-controls/*",
                findings[findings.index("--red-controls") + 1],
            )

    def test_seed_blocks_are_a_fixed_regression_block_and_a_rotating_block(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            commands = _recorded_commands(self.shell, Path(tmp))
        blocks = {}
        for command in commands:
            if command[0] == "campaign":
                start = int(command[command.index("--seed-start") + 1])
                count = int(command[command.index("--seeds") + 1])
                blocks[command[command.index("--report") + 1]] = (start, count)
        fixed, rotating, differential = (
            blocks[f"{tmp}/campaigns/{name}/report.json"] for name in CAMPAIGNS
        )
        self.assertEqual(0, fixed[0])
        self.assertGreaterEqual(rotating[0], 100000)
        self.assertEqual(0, (rotating[0] - 100000) % 64)
        self.assertEqual(rotating[0], differential[0])

    def test_the_job_never_files_or_contacts_stores(self) -> None:
        for shell in (self.shell, self.emit):
            self.assertNotIn("--dry-run", shell)
            self.assertNotRegex(shell, r"--file\b|--ledger\b|--offline\b")
            self.assertNotRegex(shell, r"\bmeta (tasks|artifacts)\.")
            for needle in ("--owner", "EDEN_EXPLORATORY_FILE", "scribe_cat"):
                self.assertNotIn(needle, shell)
        self.assertNotIn("--sink", self.shell)
        self.assertNotIn("--sink", self.emit)
        self.assertNotIn("~", self.shell)
        self.assertNotIn(".local", self.shell)

    def test_emit_writes_each_campaigns_rows_to_a_job_artifact(self) -> None:
        self.assertIn("for name in fixed rotating differential; do", self.emit)
        self.assertIn(
            'emit --report "$dir/report.json" --findings'
            ' "$dir/findings/findings.json" > "$dir/telemetry.jsonl"',
            self.emit,
        )
        provided = {
            path
            for artifact in self.steps[1]["provide_artifacts"]
            for path in artifact["paths"]
        }
        self.assertEqual(
            {f"{OUTPUT}campaigns/{name}/telemetry.jsonl" for name in CAMPAIGNS},
            provided,
        )

    def test_recorded_findings_command_runs_end_to_end(self) -> None:
        """The job's own findings argv, on a fake campaign, writes the findings report."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            commands = _recorded_commands(self.shell, root / "job")
            findings = commands[1]
            directory = root / "job" / "campaigns" / "fixed"
            run_cli(
                "campaign",
                target="fake",
                seeds=2,
                artifacts=directory / "artifacts",
                report=directory / "report.json",
            )
            argv = [
                "fake" if previous == "--target" else arg
                for previous, arg in zip(["", *findings], findings)
            ]
            argv = [a for a in argv if not a.startswith(f"{directory}/red-controls")]
            argv[argv.index("--red-controls") : argv.index("--red-controls") + 1] = []
            with redirect_stdout(io.StringIO()) as out:
                self.assertEqual(1, _main(argv))
            self.assertTrue((directory / "findings" / "findings.json").is_file())
        self.assertRegex(out.getvalue(), re.compile(r"^findings: \d+ written", re.M))
        self.assertRegex(
            out.getvalue(), re.compile(r"^FAIL provenance on every seed", re.M)
        )

    def test_every_target_bearing_verb_names_the_edenfs_target(self) -> None:
        """The CLI defaults no target, so each recorded command names one."""
        with tempfile.TemporaryDirectory() as tmp:
            commands = _recorded_commands(
                self.shell, Path(tmp), red_controls="torn-write"
            )
        recorded = {command[0] for command in commands}
        self.assertEqual({"campaign", "run", "findings"}, recorded)
        for command in commands:
            self.assertEqual(
                "edenfs", command[command.index("--target") + 1], command[0]
            )
        for verb in TARGET_VERBS - recorded:
            self.assertIn(f"{verb} --target edenfs", self.shell)

    def test_job_description_names_the_conveyor_node_and_the_daemon(self) -> None:
        description = self.spec["description"]
        for needle in (
            "Sandcastle node in the eden/eden conveyor",
            'beside "eden smoke tests"',
            "release package's edenfs",
            "EDENFS_SERVER_PATH",
        ):
            self.assertIn(needle, description)

    def test_every_run_fits_the_job_timeout_at_its_deadline(self) -> None:
        """Campaign seeds at the default runner deadline, control and vehicle runs at the deck's.

        The sum counts only runner deadlines; the margin to the job timeout
        covers the build, daemon startup, findings, and emit.
        """
        target = _registry()["edenfs"]
        with redirect_stdout(io.StringIO()) as out:
            self.assertEqual(0, _main(["red-controls", "--target", "edenfs"]))
        controls = out.getvalue().split()
        self.assertTrue(controls)
        with redirect_stdout(io.StringIO()) as out:
            self.assertEqual(0, _main(cli_argv("generate", target="edenfs", seed=0)))
        composed = json.loads(out.getvalue())
        deck = DECKS / "red-control.json"
        deck_deadline = runspec.from_json(
            deck.read_text(), target
        ).budgets.runner_deadline_seconds
        with tempfile.TemporaryDirectory() as tmp:
            commands = _recorded_commands(
                self.shell, Path(tmp), red_controls=" ".join(controls)
            )
        total = 0.0
        for command in commands:
            if command[0] == "campaign":
                self.assertNotIn("--runner-deadline", command)
                deadline = composed["budgets"]["runner_deadline_seconds"]
                self.assertEqual(runspec.DEFAULT_RUNNER_DEADLINE_SECONDS, deadline)
                total += int(command[command.index("--seeds") + 1]) * deadline
            else:
                self.assertEqual(command[0] == "run", "--spec" in command)
                total += deck_deadline * (command[0] == "run")
        self.assertEqual(3 * len(controls) + 1, sum(c[0] == "run" for c in commands))
        self.assertLessEqual(total, self.spec["args"]["timeout"])

    def test_release_summary_names_its_target_and_decks(self) -> None:
        shell = _spec("release-decks.json")["args"]["steps"][0]["shell"]
        subprocess.run(["bash", "-n"], input=shell, text=True, check=True)
        self.assertIn(
            "release-summary --target edenfs --decks"
            " fbcode/eden/integration/correctness/exploratory/decks",
            shell,
        )

    def test_provided_artifacts_exclude_accepting_seeds(self) -> None:
        provided = {
            path.removeprefix(OUTPUT)
            for artifact in self.steps[0]["provide_artifacts"]
            for path in artifact["paths"]
        }
        expected = {
            f"campaigns/{name}/{suffix}"
            for name in CAMPAIGNS
            for suffix in (
                "report.json",
                "artifacts/summary.jsonl",
                "artifacts/rejects",
                "findings",
                "red-controls",
            )
        } | {"vehicle"}
        self.assertEqual(expected, provided)

    def test_each_seed_block_plan_meets_the_site_floor_and_covers_every_stratum(
        self,
    ) -> None:
        """Every health check that reads the plan can pass when every planned run is witnessed."""
        target = _registry()["edenfs"]
        strata = set(admitted_strata(target, PUBLIC_MASTER))
        with tempfile.TemporaryDirectory() as tmp:
            commands = _recorded_commands(self.shell, Path(tmp))
        blocks = [
            c for c in commands if c[0] == "campaign" and "--differential" not in c
        ]
        self.assertEqual(2, len(blocks))
        for command in blocks:
            start = int(command[command.index("--seed-start") + 1])
            count = int(command[command.index("--seeds") + 1])
            entries = plan(target, start, count, PUBLIC_MASTER)
            self.assertEqual(strata, {e.stratum for e in entries})
            per_site = Counter(e.stratum[0] for e in entries if e.stratum)
            self.assertEqual({s for s, _, _ in strata}, set(per_site))
            self.assertGreaterEqual(min(per_site.values()), WITNESS_FLOOR, per_site)

    def test_failed_findings_or_vehicle_fail_the_job_after_emit(self) -> None:
        """Gate failures are recorded while every campaign still runs; emit writes rows, then fails."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            commands = _recorded_commands(
                self.shell, root, red_controls="torn-write", failing="run findings"
            )
            self.assertEqual(3, [c[0] for c in commands].count("campaign"))
            failed = (root / "failed-gates").read_text().split()
            self.assertEqual([*CAMPAIGNS, "vehicle"], failed)
            vehicle = commands[-1]
            self.assertNotIn("--red-control", vehicle)
            self.assertEqual(
                f"{tmp}/vehicle", vehicle[vehicle.index("--artifacts") + 1]
            )
            emit = self.emit.replace("edenfs-exploratory-nightly", str(root))
            script = "set -euo pipefail\nrecord() { echo row; }\n" + emit.replace(
                'explore="', 'unused="'
            ).replace("$explore", "record")
            result = subprocess.run(
                ["bash", "-c", script], capture_output=True, text=True
            )
            self.assertEqual(1, result.returncode, result.stdout)
            for name in CAMPAIGNS:
                self.assertEqual(
                    "row\n", (root / "campaigns" / name / "telemetry.jsonl").read_text()
                )
