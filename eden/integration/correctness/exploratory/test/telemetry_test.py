# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Telemetry rows: campaign, summary, and finding rows have fixed scalar types and
admit only their allowlisted fields, work totals, maxima, and health checks add up
across verdicts, and emitting them is bounded and spawns nothing it should not.
"""

from __future__ import annotations

import dataclasses
import json
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.cli import _main
from eden.integration.correctness.exploratory.core.campaign import (
    account,
    campaign_health,
    CampaignAttempt,
    cause_accounting,
    report_document,
    report_rows,
    ROW_FIELDS,
    summary_row,
    validate_row,
    work_accounting,
)
from eden.integration.correctness.exploratory.core.checker import Verdict, Work
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    Execution,
    ExplorationResult,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    ObligationResult,
    ObligationStatus,
    path_subject,
    PropertyResult,
)
from eden.integration.correctness.exploratory.fake.target import FAKE
from eden.integration.correctness.exploratory.test.cli_fixtures import (
    cli_argv,
    fake_campaign,
)
from eden.integration.correctness.exploratory.test.tasking_fixtures import (
    satisfied_bytes,
    synthetic_verdict,
)


class CampaignRowsTest(unittest.TestCase):
    def test_report_rows_are_fixed_scalar_types(self) -> None:
        """Campaign, property, reason, symptom, and finding rows carry scalars only."""
        rows = report_rows(SCUBA_REPORT, FINDINGS)
        self.assertEqual(
            [
                "campaign",
                "property",
                "reason",
                "property",
                "reason",
                "action",
                "obligation",
                "work",
                "work",
                "work",
                "work",
                "symptom",
                "symptom",
                "symptom",
                "finding",
                "reason",
            ],
            [row["row_type"] for row in rows],
        )
        self.assertEqual(
            {
                "row_type": "reason",
                "property_id": "verdict",
                "reason_kind": "not_evaluated_cause",
                "reason_id": "blocked",
                "count": 1,
            },
            rows[4],
        )
        self.assertEqual(
            [
                ("construction", "groups", 3, 12, 5),
                ("search", "work", 3, 40, 31),
                ("component", "components", 3, None, 2),
                ("component", "decision_work", 3, None, 31),
            ],
            [
                (r["phase"], r["counter"], r["runs"], r["total"], r["max"])
                for r in rows
                if r["row_type"] == "work"
            ],
        )
        self.assertFalse(
            [v for row in rows for v in row.values() if isinstance(v, (dict, list))]
        )
        campaign = rows[0]
        self.assertEqual(
            (3, 2, 1, 1, 1, 1, True, True, None, 4, 7),
            (
                campaign["seeds"],
                campaign["completed"],
                campaign["harness_failures"],
                campaign["accept"],
                campaign["reject"],
                campaign["task_keys"],
                campaign["controls_supplied"],
                campaign["controls_passed"],
                campaign["stopped_reason"],
                campaign["per_site_witnessed_min"],
                campaign["families_witnessed_min"],
            ),
        )
        self.assertEqual(
            {
                "row_type": "reason",
                "property_id": "bytes",
                "reason_kind": "not_evaluated",
                "reason_id": "bytes.linearization-ambiguous",
                "count": 2,
            },
            rows[2],
        )
        self.assertEqual(
            (7, 1, 2),
            (rows[1]["satisfied"], rows[1]["violated"], rows[1]["not_evaluated"]),
        )
        self.assertEqual(
            ("bytes", "primary", 4, 4, 3),
            tuple(
                rows[5][k] for k in ("family", "mode", "planned", "started", "recorded")
            ),
        )
        self.assertNotIn("applicable", rows[5])
        self.assertNotIn("planned", rows[6])
        self.assertEqual(
            ("bytes", "fuse", "primary", 10, 8, 2),
            tuple(
                rows[6][k]
                for k in (
                    "family",
                    "channel",
                    "mode",
                    "applicable",
                    "evaluated",
                    "not_evaluated",
                )
            ),
        )
        self.assertEqual(
            [
                ("bytes.torn-read", "torn-read"),
                ("bytes.torn-read", "torn-read"),
                ("status.model-live-stale-status", "model-live-stale-status"),
            ],
            [
                (r["rule_id"], r["finding_kind"])
                for r in rows
                if r["row_type"] == "symptom"
            ],
        )
        self.assertEqual(
            (1, 1), (campaign["routed_harness"], campaign["routed_reference"])
        )
        self.assertEqual(
            (False, "sites-witnessed"),
            (campaign["health_passed"], campaign["health_failed"]),
        )
        digests = [r["roles_digest"] for r in rows if r["row_type"] == "symptom"]
        self.assertEqual(3, len(set(digests)))
        finding = rows[14]
        self.assertEqual(
            ("sha256:" + "d" * 64, 2, 1, 4),
            (
                finding["task_key"],
                finding["rules"],
                finding["runs"],
                finding["first_seed"],
            ),
        )

    def test_report_rows_without_findings_and_with_failed_controls(self) -> None:
        failed = {
            **FINDINGS,
            "controls": {"diff-loss": {"status.status-diff-mismatch": "did-not-fire"}},
        }
        self.assertFalse(report_rows(SCUBA_REPORT, failed)[0]["controls_passed"])
        stopped = {**SCUBA_REPORT, "stopped": {"reason": "streak", "seed": 2}}
        rows = report_rows(stopped)
        self.assertEqual(
            ("streak", False, 0),
            (
                rows[0]["stopped_reason"],
                rows[0]["controls_supplied"],
                rows[0]["task_keys"],
            ),
        )
        self.assertNotIn("finding", [row["row_type"] for row in rows])

    def test_validate_row_rejects_rich_or_oversize_rows(self) -> None:
        reason = {
            "row_type": "reason",
            "property_id": "bytes",
            "reason_kind": "errors",
            "reason_id": "r",
            "count": 1,
        }
        self.assertEqual(reason, validate_row(reason))
        with self.assertRaisesRegex(ValueError, "not scalar"):
            validate_row({**reason, "reason_id": ["path"]})
        self.assertEqual(
            {
                "row_type": "reason",
                "property_id": "telemetry",
                "reason_kind": "reason",
                "reason_id": "telemetry.row-oversize",
                "count": 9100,
            },
            validate_row({**reason, "reason_id": "x" * 9000}),
        )
        with self.assertRaisesRegex(ValueError, "unknown row type"):
            validate_row({"row_type": "reject"})

    def test_row_schemas_admit_only_their_allowlisted_fields(self) -> None:
        """Any field outside a row type's allowlist is refused, whatever its name."""
        rows = report_rows(SCUBA_REPORT, FINDINGS)
        self.assertEqual(set(ROW_FIELDS), {row["row_type"] for row in rows})
        for row in rows:
            row_type = row["row_type"]
            with self.subTest(row_type=row_type):
                self.assertEqual(ROW_FIELDS[row_type], set(row))
                for extra in ("elapsed_seconds", "cpu", "peak", "duration", "x"):
                    with self.assertRaisesRegex(ValueError, f"extra \\['{extra}'\\]"):
                        validate_row({**row, extra: 0})
                missing = sorted(ROW_FIELDS[row_type] - {"row_type"})[0]
                with self.assertRaisesRegex(ValueError, f"missing \\['{missing}'\\]"):
                    validate_row({k: v for k, v in row.items() if k != missing})

    def test_summary_row_carries_typed_findings_and_verdict_bytes(self) -> None:
        verdict = synthetic_verdict(
            PropertyResult(
                "bytes",
                (
                    ObligationResult(
                        rule_id="bytes.torn-read",
                        scope="r",
                        subject=path_subject("r"),
                        status=ObligationStatus.VIOLATED,
                        attribution=Attribution.PRODUCT,
                        reason="torn",
                    ),
                ),
            )
        )
        with tempfile.TemporaryDirectory() as temporary:
            (Path(temporary) / "verdict.json").write_bytes(b"x" * 17)
            result = ExplorationResult(
                Execution(COMPLETED, "ok", 1, 0), verdict, temporary
            )
            row = summary_row(3, None, ["sig"], result)
        self.assertEqual(
            [
                {
                    "property_id": "bytes",
                    "rule_id": "bytes.torn-read",
                    "attribution": "product",
                    "subject": "path:r",
                }
            ],
            row["findings"],
        )
        self.assertEqual((17, None), (row["verdict_bytes"], row["not_evaluated_cause"]))


SCUBA_REPORT: dict[str, Any] = {
    "campaign_id": "sha256:" + "c" * 64,
    "daemon_sha256": "ab" * 32,
    "daemon_source_commit": "c" * 40,
    "fatal_eden_bug": False,
    "seeds": 3,
    "completed": 2,
    "harness_failures": 1,
    "environment_degraded": 0,
    "verdicts": {"accept": 1, "reject": 1, "harness-suspect": 1},
    "sites": {
        "fetch-tree": {"admitted": 3, "installs": 3, "witnessed": 9, "completed": 3},
        "tree-checkout": {"admitted": 3, "installs": 3, "witnessed": 4, "completed": 2},
    },
    "input_families": {"background_read": {"specs": 2, "completed": 2, "witnessed": 7}},
    "strata": {"completed": {"fetch-tree/error/none": 3}, "missing": []},
    "leaked_run_dirs": [],
    "work": {
        "runs": 3,
        "phases": {
            "construction": {"groups": {"total": 12, "max": 5}},
            "search": {"work": {"total": 40, "max": 31}},
        },
        "components": {"components": 2, "decision_work": 31},
    },
    "not_evaluated_causes": {"blocked": 1},
    "properties": {
        "bytes": {
            "runs": 3,
            "no_obligation_runs": 0,
            "obligations": {"satisfied": 7, "violated": 1, "not-evaluated": 2},
            "not_evaluated": {"bytes.linearization-ambiguous": 2},
            "errors": {},
            "findings": {"bytes.torn-read": 1},
            "coverage": {"bytes.write-observed": 4},
        },
        "status": {
            "runs": 3,
            "no_obligation_runs": 3,
            "obligations": {"satisfied": 0, "violated": 0, "not-evaluated": 0},
            "not_evaluated": {},
            "errors": {},
            "findings": {},
            "coverage": {},
        },
    },
    "actions": [
        {
            "family": "bytes",
            "mode": "primary",
            "planned": 4,
            "started": 4,
            "recorded": 3,
        }
    ],
    "obligations": [
        {
            "family": "bytes",
            "channel": "fuse",
            "mode": "primary",
            "applicable": 10,
            "observed": 8,
            "evaluated": 8,
            "not_evaluated": 2,
            "violated": 1,
        }
    ],
}


FINDINGS: dict[str, Any] = {
    "controls": {"torn-write": {"bytes.torn-read": "pass"}},
    "findings": [
        {
            "task_key": "sha256:" + "d" * 64,
            "rules": ["bytes.torn-read", "status.model-live-stale-status"],
            "errors": [
                ["bytes.torn-read", "torn-read", [["channel", "fuse"]]],
                ["bytes.torn-read", "torn-read", [["channel", "fuse"]]],
                ["bytes.torn-read", "torn-read", [["channel", "thrift"]]],
                ["status.model-live-stale-status", "model-live-stale-status", []],
            ],
            "runs": [{"seed": 4}],
        }
    ],
    "execution": [{"seed": 2, "execution_id": "execution.harness-failure"}],
    "unclassified": [],
    "routed": [{"route": "harness"}, {"route": "reference"}],
}


class CampaignTelemetryTest(unittest.TestCase):
    """Deterministic work counters, not-evaluated causes, and health checks."""

    def _campaign(self, root: Path) -> dict[str, Any]:
        with redirect_stdout(StringIO()):
            _main(
                cli_argv(
                    "campaign",
                    target="fake",
                    seeds=2,
                    artifacts=root / "runs",
                    report=root / "report.json",
                )
            )
        return json.loads((root / "report.json").read_text())

    def test_fake_campaign_work_sums_every_verdict_and_reruns_identically(
        self,
    ) -> None:
        with (
            tempfile.TemporaryDirectory() as first,
            tempfile.TemporaryDirectory() as second,
        ):
            report = self._campaign(Path(first))
            again = self._campaign(Path(second))
            verdicts = [
                json.loads(
                    (Path(first) / "runs" / f"seed-{seed}" / "verdict.json").read_text()
                )
                for seed in (0, 1)
            ]
        work = report["work"]
        self.assertEqual(work, again["work"])
        self.assertEqual(2, work["runs"])
        self.assertIn("construction", work["phases"])
        for phase, counters in work["phases"].items():
            for counter, values in counters.items():
                per_run = [v["work"]["phases"][phase].get(counter, 0) for v in verdicts]
                self.assertEqual(
                    {"total": sum(per_run), "max": max(per_run)},
                    values,
                    (phase, counter),
                )
        self.assertEqual(
            max(len(v["work"]["components"]) for v in verdicts),
            work["components"]["components"],
        )
        rows = report_rows(report)
        work_rows = [r for r in rows if r["row_type"] == "work"]
        self.assertEqual(
            sum(len(c) for c in work["phases"].values()) + len(work["components"]),
            len(work_rows),
        )
        for row in rows:
            self.assertEqual(ROW_FIELDS[row["row_type"]], set(row))

    def test_work_totals_maxima_and_component_high_water_marks(self) -> None:
        def run(
            phases: dict[str, dict[str, int]], components: dict[str, dict[str, int]]
        ) -> CampaignAttempt:
            verdict = synthetic_verdict(satisfied_bytes())
            assert verdict.report is not None
            report = dataclasses.replace(
                verdict.report, work=Work(phases=phases, components=components)
            )
            return CampaignAttempt(
                0,
                None,
                ExplorationResult(
                    Execution(COMPLETED, "ok", 1, 0),
                    dataclasses.replace(verdict, report=report),
                    "",
                ),
            )

        # The largest run comes first, so a maximum cannot be the last value.
        rows = [
            run(
                {"search": {"work": 9}},
                {"c0": {"decision_work": 1}, "c1": {"decision_work": 9, "states": 2}},
            ),
            run(
                {"search": {"work": 5, "branches": 2}},
                {"c0": {"decision_work": 5, "states": 3}},
            ),
        ]
        self.assertEqual(
            {
                "runs": 2,
                "phases": {
                    "search": {
                        "branches": {"total": 2, "max": 2},
                        "work": {"total": 14, "max": 9},
                    }
                },
                "components": {"components": 2, "decision_work": 9, "states": 3},
            },
            work_accounting(rows),
        )

    def test_recording_target_campaign_counts_its_not_evaluated_causes(self) -> None:
        rows = [
            CampaignAttempt(
                seed,
                None,
                ExplorationResult(
                    Execution(COMPLETED, "ok", 1, 0),
                    Verdict(
                        "not-evaluated",
                        "no-evaluation",
                        not_evaluated_cause="no-evaluation",
                    ),
                    "",
                ),
            )
            for seed in (0, 1)
        ]
        self.assertEqual({"no-evaluation": 2}, cause_accounting(rows))
        self.assertEqual(0, work_accounting(rows)["runs"])
        document = report_document(account(rows, (), FAKE), {"seeds": 2})
        self.assertEqual({"no-evaluation": 2}, document["not_evaluated_causes"])
        causes = [
            (r["reason_id"], r["count"])
            for r in report_rows({**SCUBA_REPORT, **document})
            if r["row_type"] == "reason" and r["property_id"] == "verdict"
        ]
        self.assertEqual([("no-evaluation", 2)], causes)

    def test_each_health_gate_names_its_own_failure(self) -> None:
        healthy = {
            **SCUBA_REPORT,
            "sites": {"s": {"witnessed": 8}},
            "input_families": {"f": {"witnessed": 8}},
        }
        self.assertEqual([], [g.name for g in campaign_health(healthy) if not g.ok])
        bytes_totals = SCUBA_REPORT["properties"]["bytes"]
        cases = {
            "harness-failures": {"seeds": 10, "completed": 2},
            "not-stopped": {"stopped": {"reason": "streak"}},
            "strata-covered": {
                "strata": {"completed": {}, "missing": [["a", "b", "c"]]}
            },
            "sites-witnessed": {"sites": {"s": {"witnessed": 5}}},
            "no-property-errors": {
                "properties": {"bytes": {**bytes_totals, "errors": {"KeyError": 1}}}
            },
            "no-leaked-run-dirs": {"leaked_run_dirs": [{"seed": 1, "path": "/x"}]},
            "obligations-judged": {
                "properties": {
                    "bytes": {
                        **bytes_totals,
                        "obligations": {
                            "satisfied": 0,
                            "violated": 0,
                            "not-evaluated": 2,
                        },
                    }
                }
            },
            "no-engine-shortfall": {
                "properties": {
                    "bytes": {
                        **bytes_totals,
                        "not_evaluated": {"core.solver.budget": 1},
                    }
                }
            },
        }
        for gate, change in cases.items():
            with self.subTest(gate=gate):
                failed = [g for g in campaign_health({**healthy, **change}) if not g.ok]
                self.assertEqual([gate], [g.name for g in failed])
                self.assertTrue(failed[0].detail)
                row = report_rows({**healthy, **change})[0]
                self.assertEqual(
                    (False, gate), (row["health_passed"], row["health_failed"])
                )
        timeouts = {**healthy, "seeds": 9, "completed": 2, "harness_failures": 0}
        self.assertEqual(
            [("harness-failures", "7 > 6")],
            [(g.name, g.detail) for g in campaign_health(timeouts) if not g.ok],
        )
        degraded = {**healthy, "seeds": 9, "completed": 2, "environment_degraded": 6}
        self.assertEqual([], [g.name for g in campaign_health(degraded) if not g.ok])
        invalid = {**healthy, "not_evaluated_causes": {"invalid-history": 2}}
        failed = [g for g in campaign_health(invalid) if not g.ok]
        self.assertEqual(
            [("no-property-errors", "2 errors")], [(g.name, g.detail) for g in failed]
        )
        families = {**healthy, "input_families": {"f": {"witnessed": 0}}}
        self.assertEqual([], [g.name for g in campaign_health(families) if not g.ok])


def _refuse(*_args: object, **_kwargs: object) -> None:
    raise AssertionError("this path must not spawn or build a Meta store")


def _fake_report(root: Path) -> Path:
    """Write a one-seed fake campaign under `root` and return its report."""
    fake_campaign(root, red_seeds=())
    return root / "report.json"


class EmitTest(unittest.TestCase):
    def test_emit_stdout_prints_scalar_rows_and_spawns_nothing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report = _fake_report(Path(tmp))
            with (
                mock.patch.object(subprocess, "run", _refuse),
                mock.patch.object(subprocess, "Popen", _refuse),
                redirect_stdout(StringIO()) as out,
            ):
                code = _main(cli_argv("emit", report=report))
        self.assertEqual(0, code)
        rows = [json.loads(line) for line in out.getvalue().splitlines()]
        self.assertEqual("campaign", rows[0]["row_type"])
        self.assertTrue(all("time" not in row for row in rows))
