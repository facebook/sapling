# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""What the findings report records around a key: bindings compare semantic and
identity digests, red-control outcomes are judged per rule and bound to their
campaign, and execution findings and run records have stable typed IDs.
"""

from __future__ import annotations

import dataclasses
import json
import tempfile
import unittest
from pathlib import Path
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory import findings
from eden.integration.correctness.exploratory.core.checker import semantic_digest
from eden.integration.correctness.exploratory.core.containment import (
    MOUNT_WEDGED_PREFIX,
)
from eden.integration.correctness.exploratory.core.facts import (
    FieldKind,
    FieldSpec,
    ResultSchema,
)
from eden.integration.correctness.exploratory.core.identity import error_set_key
from eden.integration.correctness.exploratory.findings import (
    binding_mismatches,
    execution_findings,
    load_run,
    read_findings,
    read_report,
    red_control_outcome,
)
from eden.integration.correctness.exploratory.schemas import (
    admission_for,
    RunPurpose,
    SchemaError,
    write_admission,
)
from eden.integration.correctness.exploratory.test.tasking_fixtures import (
    Ob,
    script,
    SCRIPTED,
    stale,
    torn,
    write_run,
)


TORN_FUSE = error_set_key([("bytes.torn-read", "torn-read", (("channel", "fuse"),))])


class BindingTest(unittest.TestCase):
    def test_bindings_compare_the_semantic_and_identity_digests(self) -> None:
        getattr_def = SCRIPTED.actions["getattr"]
        resourced = dataclasses.replace(
            SCRIPTED,
            actions={
                **SCRIPTED.actions,
                "getattr": dataclasses.replace(
                    getattr_def,
                    result=ResultSchema(
                        ok=(
                            *getattr_def.result.ok,
                            FieldSpec(
                                "trace", FieldKind.JSON, required=False, consumed=False
                            ),
                        )
                    ),
                ),
            },
        )
        reprofiled = dataclasses.replace(SCRIPTED, semantics_profile="scripted-v2")
        self.assertNotEqual(semantic_digest(SCRIPTED), semantic_digest(resourced))
        with tempfile.TemporaryDirectory() as temporary:
            run_dir = Path(temporary) / "seed-1"
            verdict = write_run(run_dir, script(torn()))
            found = {
                name: binding_mismatches(run_dir, target, verdict)
                for name, target in (
                    ("same", SCRIPTED),
                    ("semantic", resourced),
                    ("identity", reprofiled),
                )
            }
        self.assertEqual(
            {"same": (), "semantic": ("semantic",), "identity": ("identity",)}, found
        )


CAMPAIGN = "campaign-1"


def _control_dir(root: Path, control: str, *obligations: Ob, **kwargs: Any) -> Path:
    directory = root / control
    write_run(directory, script(*obligations), **kwargs)
    write_admission(directory, admission_for(RunPurpose.RED_CONTROL, CAMPAIGN))
    return directory


class ControlTest(unittest.TestCase):
    """Red controls calibrate each mapped rule on its own, with scoped gaps."""

    def test_a_completed_control_without_a_report_fails_its_bindings(self) -> None:
        """A decodable verdict with no report names `binding-report`, never aborts the pass."""
        load = findings.load_verdict_file

        def reportless(path: Path) -> Any:
            loaded = load(path)
            return dataclasses.replace(
                loaded, verdict=dataclasses.replace(loaded.verdict, report=None)
            )

        with tempfile.TemporaryDirectory() as temporary:
            directory = _control_dir(Path(temporary), "torn-write", torn())
            with mock.patch.object(findings, "load_verdict_file", reportless):
                outcome = red_control_outcome(directory, SCRIPTED, CAMPAIGN)
        self.assertEqual({"bytes.torn-read": "binding-report"}, outcome.per_rule)
        self.assertIsNone(outcome.runspec_digest)

    def test_each_mapped_rule_is_judged_on_its_own(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            outcomes = [
                red_control_outcome(
                    _control_dir(
                        root,
                        "torn-write",
                        torn(),
                        Ob("readdir.duplicate-entry", "path:x", status="not-evaluated"),
                    ),
                    SCRIPTED,
                    CAMPAIGN,
                ),
                red_control_outcome(
                    _control_dir(root, "stale-write", torn()), SCRIPTED, CAMPAIGN
                ),
                red_control_outcome(
                    _control_dir(
                        root,
                        "status-loss",
                        Ob(
                            "status.model-live-stale-status",
                            "path:r0",
                            status="not-evaluated",
                        ),
                    ),
                    SCRIPTED,
                    CAMPAIGN,
                ),
                red_control_outcome(
                    _control_dir(root, "not-a-control"), SCRIPTED, CAMPAIGN
                ),
            ]
        self.assertEqual({"bytes.torn-read": "pass"}, outcomes[0].per_rule)
        self.assertEqual({"bytes.stale-read": "did-not-fire"}, outcomes[1].per_rule)
        self.assertEqual(
            {
                "status.model-live-stale-status": (
                    "incomplete-status.model-live-stale-status@path:r0#0"
                )
            },
            outcomes[2].per_rule,
        )
        self.assertEqual({}, outcomes[3].per_rule)

    def test_a_raising_owner_leaves_its_rule_incomplete_only(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "torn-write"
            write_run(directory, script(stale(), raises=("bytes",)))
            write_admission(directory, admission_for(RunPurpose.RED_CONTROL, CAMPAIGN))
            outcome = red_control_outcome(directory, SCRIPTED, CAMPAIGN)
        self.assertEqual({"bytes.torn-read": "incomplete-bytes"}, outcome.per_rule)

    def test_bindings_purpose_and_campaign_bind_the_execution(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = _control_dir(Path(temporary), "torn-write", torn())
            other = red_control_outcome(directory, SCRIPTED, "campaign-2")
            unbound = red_control_outcome(directory, SCRIPTED)
            bound = red_control_outcome(directory, SCRIPTED, CAMPAIGN)
            write_admission(directory, admission_for(RunPurpose.MANUAL, CAMPAIGN))
            manual = red_control_outcome(directory, SCRIPTED, CAMPAIGN)
            write_admission(directory, admission_for(RunPurpose.RED_CONTROL, CAMPAIGN))
            (directory / "run-spec.json").write_text(
                (directory / "run-spec.json").read_text().replace("written", "other")
            )
            rebound = red_control_outcome(directory, SCRIPTED, CAMPAIGN)
        self.assertEqual({"bytes.torn-read": "unbound-campaign"}, other.per_rule)
        self.assertEqual({"bytes.torn-read": "unbound-campaign"}, unbound.per_rule)
        self.assertEqual({"bytes.torn-read": "pass"}, bound.per_rule)
        self.assertEqual({"bytes.torn-read": "purpose-manual"}, manual.per_rule)
        self.assertEqual({"bytes.torn-read": "binding-runspec"}, rebound.per_rule)


class ExecutionFindingTest(unittest.TestCase):
    def test_statuses_and_verdict_problems_have_stable_ids(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            (run / "execution.json").write_text(
                json.dumps({"status": "harness-failure", "reason": "runner-failed"})
            )
            found = execution_findings(
                run, SCRIPTED, SchemaError("execution.missing-verdict", "x")
            )
        self.assertEqual(
            ["execution.harness-failure", "execution.missing-verdict"],
            [f.execution_id for f in found],
        )

    def test_an_aborted_fuse_connection_is_a_mount_wedged_finding(self) -> None:
        """Cleanup that had to abort the run's FUSE connection is typed; other cleanup is not."""
        for cleanup, expected in (
            (
                f"{MOUNT_WEDGED_PREFIX}blocked threads []; unmounted 1, removed",
                ["execution.containment-timeout", "execution.mount-wedged"],
            ),
            ("supervisor: unmounted 1, removed", ["execution.containment-timeout"]),
        ):
            with self.subTest(cleanup=cleanup), tempfile.TemporaryDirectory() as tmp:
                run = Path(tmp)
                (run / "execution.json").write_text(
                    json.dumps(
                        {
                            "status": "containment-timeout",
                            "reason": "runner exceeded 240s",
                            "cleanup": cleanup,
                        }
                    )
                )
                found = execution_findings(run, SCRIPTED, SchemaError("x", "y"))
                self.assertEqual(expected + ["x"], [f.execution_id for f in found])

    def test_harness_failure_detail_carries_the_backend_error(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            (run / "execution.json").write_text(
                json.dumps(
                    {
                        "status": "harness-failure",
                        "reason": "invalid-backend-response",
                        "backend_error": "RuntimeError: backend exploded",
                    }
                )
            )
            found = execution_findings(
                run, SCRIPTED, SchemaError("execution.missing-verdict", "x")
            )
        self.assertEqual(
            "invalid-backend-response (RuntimeError: backend exploded)",
            found[0].detail,
        )

    def test_operation_timeout_names_the_modeled_input_kind(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            write_run(run, ())
            history = (run / "history.jsonl").read_text().splitlines()
            timed_out = [
                (
                    json.dumps(
                        {
                            "type": "info",
                            "time": json.loads(line)["time"],
                            "action": "w",
                            "actor": "a",
                            "reason": "operation-timeout",
                        }
                    )
                    if json.loads(line).get("action") == "w"
                    and json.loads(line)["type"] == "ok"
                    else line
                )
                for line in history
            ]
            (run / "history.jsonl").write_text("\n".join(timed_out) + "\n")
            found = execution_findings(
                run, SCRIPTED, SchemaError("execution.verdict-unreadable", "x")
            )
        self.assertEqual(
            [
                ("execution.verdict-unreadable", "execution.verdict-unreadable: x"),
                ("execution.operation-timeout", "write"),
            ],
            [(f.execution_id, f.detail) for f in found],
        )


class RunRecordTest(unittest.TestCase):
    def test_missing_execution_status_is_a_typed_execution_failure(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "seed-1").mkdir()
            row = {"seed": 1, "status": "completed", "artifacts": str(root / "seed-1")}
            run = load_run(row, SCRIPTED, root)
        self.assertEqual("execution.missing-execution", run.execution_status)
        self.assertIn(
            "execution.execution-unreadable", [f.execution_id for f in run.execution]
        )

    def test_a_row_naming_a_run_outside_the_artifacts_is_a_summary_mismatch(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            (root / "seed-1").mkdir()
            (artifacts / "link").symlink_to(root / "seed-1")
            for where in ("../seed-1", "link", str(root / "seed-1")):
                with self.subTest(artifacts=where):
                    row = {"seed": 1, "status": "completed", "artifacts": where}
                    run = load_run(row, SCRIPTED, artifacts)
                    self.assertEqual(
                        ["execution.summary-mismatch"],
                        [f.execution_id for f in run.execution],
                    )

    def test_report_and_findings_readers_require_exact_documents(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "report.json"
            path.write_text(json.dumps({"seeds": 1}))
            with self.assertRaises(SchemaError):
                read_report(path)
            path.write_text(json.dumps({"schema_version": True}))
            with self.assertRaises(SchemaError):
                read_findings(path)
            path.write_text(json.dumps({"schema_version": 1, "findings": []}))
            with self.assertRaisesRegex(SchemaError, "missing .*'controls'"):
                read_findings(path)
