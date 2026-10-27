# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The findings report over a real fake-target campaign: it writes only its output
directory and is deterministic, oversize or malformed gates are kept whole, malformed
run inputs and report fields are typed refusals, the CLI keeps a zero exit code and
emits only validated rows, and `replay` re-runs a finding's run directory.
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import replace
from pathlib import Path
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory import findings
from eden.integration.correctness.exploratory.cli import _main
from eden.integration.correctness.exploratory.core.checker import (
    check,
    decode_verdict,
    encode_verdict,
)
from eden.integration.correctness.exploratory.core.containment import load_history
from eden.integration.correctness.exploratory.core.history import (
    Event,
    event_to_json,
    history_problems,
    INVOKE,
)
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.fake.target import FAKE
from eden.integration.correctness.exploratory.schemas import (
    BOUNDS,
    canonical_json,
    load_admission,
    RunPurpose,
    SchemaError,
    VERDICT_OVERSIZE,
)
from eden.integration.correctness.exploratory.test.cli_fixtures import (
    cli_argv,
    fake_campaign,
    run_cli,
    tearing_fake,
)


def _hashes(root: Path) -> dict[str, str]:
    return {
        str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }


def _refuse(*_args: object, **_kwargs: object) -> None:
    raise AssertionError("the findings report must not spawn or contact a store")


class FindingsReportTest(unittest.TestCase):
    """The findings report over a real fake-target campaign."""

    def test_the_report_writes_only_its_output_directory_and_is_deterministic(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = fake_campaign(root)
            before = _hashes(root)
            outputs = []
            for name in ("out1", "out2"):
                with (
                    mock.patch.object(subprocess, "run", _refuse),
                    mock.patch.object(subprocess, "Popen", _refuse),
                    mock.patch.object(os, "execv", _refuse),
                    mock.patch.object(os, "execvp", _refuse),
                ):
                    lines = findings.write_findings(
                        root / "report.json", artifacts, root / name, FAKE
                    )
                outputs.append((root / name / "findings.json").read_text())
            after = _hashes(root)
        changed = {path for path in after if before.get(path) != after[path]}
        self.assertTrue(changed)
        self.assertEqual(
            set(), {path for path in changed if not path.startswith(("out1/", "out2/"))}
        )
        self.assertEqual(outputs[0], outputs[1])
        (entry,) = json.loads(outputs[0])["findings"]
        self.assertEqual(["bytes.torn-read"], entry["rules"])
        self.assertEqual([9], [run["seed"] for run in entry["runs"]])
        self.assertIn("red-controls: not supplied", lines)

    def test_a_15_mib_malformed_gate_is_kept_whole(self) -> None:
        """Every gate problem survives in the retained verdict and is reported by ID."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = fake_campaign(root)
            run_dir = artifacts / "seed-9"
            spec = from_json((run_dir / "run-spec.json").read_text(), FAKE)
            execution = json.loads((run_dir / "execution.json").read_text())
            events = [
                Event(INVOKE, 1.0, {"action": "x" * 740 + str(index)})
                for index in range(20000)
            ]
            with open(run_dir / "history.jsonl", "wb") as handle:
                for event in events:
                    handle.write(canonical_json(event_to_json(event)) + b"\n")
            problems = history_problems(
                spec, load_history(str(run_dir / "history.jsonl"))
            )
            data = encode_verdict(check(FAKE, spec, events, execution=execution))
            (run_dir / "verdict.json").write_bytes(data)
            digest = "sha256:" + hashlib.sha256(data).hexdigest()
            findings.write_findings(root / "report.json", artifacts, root / "out", FAKE)
            decisions = json.loads((root / "out" / "findings.json").read_text())
            retained = (run_dir / "verdict.json").read_bytes()
        self.assertGreater(len(data), 15 * 1024 * 1024)
        self.assertEqual(data, retained)
        report = decode_verdict(retained).report
        assert report is not None
        self.assertEqual(tuple(problems), report.gate[: len(problems)])
        self.assertGreaterEqual(len(problems), 20000)
        by_id = {
            entry["execution_id"]: entry["detail"]
            for entry in decisions["execution"]
            if entry["seed"] == 9
        }
        self.assertEqual(f"{len(data)} bytes, {digest}", by_id[VERDICT_OVERSIZE])
        self.assertIn(
            f"(+{len(report.gate) - 1} more)", by_id["execution.invalid-history"]
        )
        self.assertEqual([], decisions["findings"])

    def test_an_oversize_verdict_keeps_its_key_and_names_its_size(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = fake_campaign(root)
            data = (artifacts / "seed-9" / "verdict.json").read_bytes()
            findings.write_findings(
                root / "report.json",
                artifacts,
                root / "out",
                FAKE,
                bounds=replace(BOUNDS, verdict_operational_bytes=len(data) - 1),
            )
            decisions = json.loads((root / "out" / "findings.json").read_text())
        (entry,) = decisions["findings"]
        self.assertEqual(["bytes.torn-read"], entry["rules"])
        self.assertIn(
            f"{len(data)} bytes, sha256:" + hashlib.sha256(data).hexdigest(),
            [e["detail"] for e in decisions["execution"] if e["seed"] == 9],
        )

    def test_findings_cli_exits_one_on_a_failed_check_after_writing(self) -> None:
        """A fake campaign has no daemon provenance, so its provenance check fails."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = fake_campaign(root)
            with redirect_stdout(io.StringIO()) as out:
                code = _main(
                    cli_argv(
                        "findings",
                        target="fake",
                        report=root / "report.json",
                        artifacts=artifacts,
                    )
                )
            self.assertEqual(1, code)
            self.assertIn("FAIL provenance on every seed", out.getvalue())
            unsupplied = findings.write_findings(
                root / "report.json", artifacts, root / "unsupplied", FAKE, []
            )
            self.assertIn(
                "FAIL red-control torn-write bytes.torn-read: missing", unsupplied
            )
            self.assertTrue((root / "findings" / "findings.json").is_file())
            with redirect_stdout(io.StringIO()) as rows:
                self.assertEqual(
                    0,
                    _main(
                        cli_argv(
                            "emit",
                            report=root / "report.json",
                            findings=root / "findings" / "findings.json",
                        )
                    ),
                )
        self.assertIn("findings: 1 written", out.getvalue())
        types = [json.loads(line)["row_type"] for line in rows.getvalue().splitlines()]
        self.assertEqual("campaign", types[0])
        self.assertIn("finding", types)

    def test_bindings_summary_and_paths_are_checked_per_run(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = fake_campaign(root)
            spec = artifacts / "seed-9" / "run-spec.json"
            changed = json.loads(spec.read_text())
            changed["seed"] = 8
            spec.write_text(json.dumps(changed))
            with open(artifacts / "summary.jsonl", "a") as handle:
                handle.write(
                    json.dumps(
                        {"seed": 10, "status": "completed", "artifacts": str(root)}
                    )
                    + "\n"
                )
                handle.write("{not json\n")
            execution = artifacts / "seed-0" / "execution.json"
            execution.write_text(
                execution.read_text().replace('"completed"', '"harness-failure"')
            )
            findings.write_findings(root / "report.json", artifacts, root / "out", FAKE)
            decisions = json.loads((root / "out" / "findings.json").read_text())
        by_seed: dict[int, set[str]] = {}
        for entry in decisions["execution"]:
            by_seed.setdefault(entry["seed"], set()).add(entry["execution_id"])
        self.assertIn("execution.binding-mismatch", by_seed[9])
        self.assertIn("execution.summary-mismatch", by_seed[0])
        self.assertIn("execution.summary-mismatch", by_seed[10])
        self.assertTrue(
            any("execution.summary-unreadable" in ids for ids in by_seed.values())
        )

    def test_malformed_run_inputs_are_typed_per_run_evidence(self) -> None:
        """A malformed run field is that run's typed evidence; the pass goes on."""

        def edit_json(path: Path, change: Any) -> None:
            document = json.loads(path.read_text())
            change(document)
            path.write_text(json.dumps(document))

        def edit_summary(seed: int, field: str, value: Any) -> Any:
            def change(artifacts: Path) -> None:
                path = artifacts / "summary.jsonl"
                rows = [json.loads(line) for line in path.read_text().splitlines()]
                for row in rows:
                    if row["seed"] == seed:
                        row[field] = value
                path.write_text("".join(json.dumps(r) + "\n" for r in rows))

            return change

        def edit_run(name: str, field: str, value: Any) -> Any:
            return lambda artifacts: edit_json(
                artifacts / "seed-0" / name, lambda d: d.__setitem__(field, value)
            )

        cases = {
            "execution status list": (
                edit_run("execution.json", "status", ["completed"]),
                "execution.execution-unreadable",
            ),
            "execution reason object": (
                edit_run("execution.json", "reason", {"x": 1}),
                "execution.binding-mismatch",
            ),
            "summary stratum int": (
                edit_summary(0, "stratum", 7),
                "execution.summary-mismatch",
            ),
            "summary stratum of ints": (
                edit_summary(0, "stratum", [1, 2]),
                "execution.summary-mismatch",
            ),
            "summary status list": (
                edit_summary(0, "status", ["completed"]),
                "execution.summary-mismatch",
            ),
            "admission run_purpose list": (
                edit_run("admission.json", "run_purpose", ["discovery"]),
                None,
            ),
            "admission diagnostics list": (
                edit_run("admission.json", "diagnostics", [["a", 1]]),
                None,
            ),
        }
        for label, (mutate, execution_id) in cases.items():
            with self.subTest(case=label), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                artifacts = fake_campaign(root)
                mutate(artifacts)
                findings.write_findings(
                    root / "report.json", artifacts, root / "out", FAKE
                )
                decisions = json.loads((root / "out" / "findings.json").read_text())
                seed0 = {
                    e["execution_id"] for e in decisions["execution"] if e["seed"] == 0
                }
                if execution_id is not None:
                    self.assertIn(execution_id, seed0)
                else:
                    run = findings.load_run(
                        {
                            "seed": 0,
                            "status": "completed",
                            "artifacts": str(artifacts / "seed-0"),
                        },
                        FAKE,
                        artifacts,
                    )
                    self.assertIsNone(run.admission)
                    self.assertEqual("purpose:invalid-admission", run.admission_error)

    def test_every_malformed_report_field_is_a_schema_refusal(self) -> None:
        cases = {
            "strata": {"completed": {}, "missing": "none"},
            "seeds": "1",
            "completed": -1,
            "verdicts": [],
            "harness_failures": "0",
            "environment_degraded": True,
            "rate": "0",
            "properties": [],
            "rejects": {},
            "excluded_sites": {},
            "leaked_run_dirs": "x",
            "sites": [],
            "input_families": {"f": {"witnessed": "1"}},
            "under_witnessed_families": 0,
            "work": {"runs": 1, "phases": {"search": {"work": 3}}, "components": {}},
            "not_evaluated_causes": {"blocked": -1},
            "stopped": "budget",
            "campaign_id": 5,
            "daemon_identity": [],
            "daemon_sha256": 1,
            "daemon_source_commit": {},
            "daemon_log": 2,
            "fatal_eden_bug": "no",
            "diagnostics": [],
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = fake_campaign(root)
            report = root / "report.json"
            pristine = report.read_text()
            for name, value in cases.items():
                report.write_text(json.dumps({**json.loads(pristine), name: value}))
                with self.subTest(field=name):
                    with self.assertRaisesRegex(SchemaError, name):
                        findings.write_findings(report, artifacts, root / "out", FAKE)
            # The verb maps the refusal to exit 2 and names the field.
            argv = cli_argv(
                "findings", target="fake", report=report, artifacts=artifacts
            )
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()) as err:
                self.assertEqual(2, _main(argv))
            self.assertIn("diagnostics", err.getvalue())

    def test_emit_rejects_a_document_that_is_not_a_findings_report(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fake_campaign(root)
            broken = root / "findings.json"
            broken.write_text(json.dumps({"schema_version": 2}))
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()) as err:
                code = _main(
                    cli_argv("emit", report=root / "report.json", findings=broken)
                )
        self.assertEqual(2, code)
        self.assertIn("schema:findings", err.getvalue())

    def test_replay_reruns_a_findings_run_directory_and_compares_keys(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = fake_campaign(root)
            with redirect_stdout(io.StringIO()) as out:
                code = _main(
                    cli_argv(
                        "replay",
                        target="fake",
                        run=artifacts / "seed-9",
                        artifacts=root / "again",
                    )
                )
            replayed = (root / "again" / "verdict.json").is_file()
            purpose = load_admission(root / "again").run_purpose
            with redirect_stderr(io.StringIO()) as err:
                refused = _main(
                    cli_argv(
                        "replay",
                        target="fake",
                        run=root / "no-such-run",
                        artifacts=root / "never",
                    )
                )
        # The original key does not reproduce, so replay exits 1.
        self.assertEqual(1, code)
        self.assertTrue(replayed)
        self.assertEqual(RunPurpose.REPLAY, purpose)
        self.assertEqual(2, refused)
        self.assertIn("replay: refused", err.getvalue())
        # seed-9 is a torn-write red-control run; its RunSpec alone injects nothing.
        self.assertIn("seed-9: reject\n", out.getvalue())
        self.assertIn("again: accept\n", out.getvalue())
        self.assertRegex(out.getvalue(), r"KEY sha256:[0-9a-f]{64}: original only")

    def test_replay_of_a_planted_product_bug_reproduces_its_key(self) -> None:
        """A finding from a bug in the target itself, not a red control, replays to `both`."""
        with tempfile.TemporaryDirectory() as temporary, tearing_fake():
            root = Path(temporary)
            artifacts = fake_campaign(root, red_control=None)
            argv = cli_argv(
                "findings",
                target="fake",
                report=root / "report.json",
                artifacts=artifacts,
            )
            with redirect_stdout(io.StringIO()):
                self.assertEqual(1, _main(argv))
            document = findings.read_findings(
                root / "findings" / findings.FINDINGS_NAME
            )
            purpose = load_admission(artifacts / "seed-9").run_purpose
            with redirect_stdout(io.StringIO()) as out:
                code = _main(
                    cli_argv(
                        "replay",
                        target="fake",
                        run=artifacts / "seed-9",
                        artifacts=root / "again",
                    )
                )
        self.assertNotEqual(RunPurpose.RED_CONTROL, purpose)
        (entry,) = [e for e in document["findings"] if "bytes.torn-read" in e["rules"]]
        self.assertEqual([9], [run["seed"] for run in entry["runs"]])
        self.assertEqual(0, code)
        self.assertIn("seed-9: reject\n", out.getvalue())
        self.assertIn("again: reject\n", out.getvalue())
        self.assertIn(f"KEY {entry['task_key']}: both\n", out.getvalue())

    def test_a_corrupt_run_spec_keeps_the_runs_finding(self) -> None:
        """Keys come from `verdict.json`; an unreadable RunSpec is a binding mismatch beside them."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = fake_campaign(root)
            (artifacts / "seed-9" / "run-spec.json").write_text("{")
            with redirect_stdout(io.StringIO()) as out:
                _main(
                    cli_argv(
                        "findings",
                        target="fake",
                        report=root / "report.json",
                        artifacts=artifacts,
                    )
                )
        self.assertRegex(
            out.getvalue(), r"(?m)^FINDING bytes\.torn-read .* seeds \[9\]"
        )
        self.assertNotIn("execution.run-unreadable", out.getvalue())

    def test_replay_admission_names_the_daemon_it_ran_against(self) -> None:
        provenance = {
            "daemon_sha256": "e" * 64,
            "daemon_source_commit": "a" * 40,
            "fatal_eden_bug": False,
            "daemon_log": None,
        }
        with (
            tempfile.TemporaryDirectory() as temporary,
            mock.patch(
                "eden.integration.correctness.exploratory.cli._campaign_provenance",
                return_value=(provenance, None),
            ),
        ):
            root = Path(temporary)
            artifacts = fake_campaign(root)
            argv = cli_argv(
                "replay", target="fake", run=artifacts / "seed-9", artifacts=root / "r"
            )
            with redirect_stdout(io.StringIO()):
                _main(argv)
            daemon = load_admission(root / "r").daemon
        assert daemon is not None
        self.assertEqual(
            ("e" * 64, "a" * 40), (daemon.binary_sha256, daemon.source_commit)
        )

    def test_a_copied_campaign_reads_every_run_from_its_new_place(self) -> None:
        """Summary rows name run directories relative to the artifact directory."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            run_cli(
                "campaign",
                target="fake",
                seeds=3,
                artifacts=root / "out" / "artifacts",
                report=root / "out" / "report.json",
                keep="rejects",
            )
            shutil.copytree(root / "out", root / "copy")
            shutil.rmtree(root / "out")
            with redirect_stdout(io.StringIO()) as out:
                _main(
                    cli_argv(
                        "findings",
                        target="fake",
                        report=root / "copy" / "report.json",
                        artifacts=root / "copy" / "artifacts",
                    )
                )
            document = findings.read_findings(
                root / "copy" / "findings" / findings.FINDINGS_NAME
            )
        self.assertNotIn("summary-mismatch", out.getvalue())
        self.assertEqual([], document["execution"])
