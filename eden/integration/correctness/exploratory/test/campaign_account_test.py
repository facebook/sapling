# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Campaign accounting: failures, rejects, provenance, and coverage are tabulated, a
fault install is witnessed only when its install event was consumed, property results
and obligation funnels are summed.
"""

from __future__ import annotations

import dataclasses
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.cli import _main
from eden.integration.correctness.exploratory.core import campaign, containment, history
from eden.integration.correctness.exploratory.core.campaign import (
    account,
    ACTION_FUNNEL,
    action_subjects,
    CampaignAttempt,
    funnel_accounting,
)
from eden.integration.correctness.exploratory.core.checker import check
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    ENVIRONMENT_DEGRADED,
    Execution,
    ExplorationResult,
    HARNESS_FAILURE,
)
from eden.integration.correctness.exploratory.core.effects import Footprint
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    ObligationResult,
    ObligationStatus,
    path_subject,
    property_error,
    PropertyResult,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import (
    from_json,
    RunSpec,
    to_json,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import WORKED_EXAMPLE
from eden.integration.correctness.exploratory.fake.target import FAKE, READ
from eden.integration.correctness.exploratory.targets import target_entries
from eden.integration.correctness.exploratory.test.cli_fixtures import cli_argv
from eden.integration.correctness.exploratory.test.tasking_fixtures import (
    COMPLETED_EXECUTION,
    exploration_result,
    history_of,
    make_spec,
    register_actions,
    synthetic_verdict,
)


# The CLI's EdenFS entry; generation, strata, and sites need only its vocabulary.
EDENFS_TARGET = target_entries()["edenfs"].target


def _obligation(
    rule: str,
    status: ObligationStatus,
    scope: str,
    *,
    attribution: Attribution | None = None,
    reason: str | None = None,
    not_evaluated: str | None = None,
    channel: str | None = None,
) -> ObligationResult:
    return ObligationResult(
        rule_id=rule,
        scope=scope,
        subject=path_subject("tree/a"),
        status=status,
        witnesses=() if channel is None else (Witness("channel", channel),),
        attribution=attribution,
        reason=reason,
        not_evaluated_reason=not_evaluated,
    )


class CampaignAccountTest(unittest.TestCase):
    def test_account_reports_failures_reject_provenance_and_coverage(self) -> None:
        strata = (("site", "block", "none"), ("site", "block", "process"))
        rows = [
            CampaignAttempt(
                1,
                strata[0],
                exploration_result(
                    COMPLETED,
                    "reject",
                    "bytes/stale-read: observed old bytes",
                    "seed-1",
                ),
            ),
            CampaignAttempt(
                2,
                strata[1],
                exploration_result(HARNESS_FAILURE, "accept", "ok", "seed-2"),
            ),
            CampaignAttempt(
                3,
                strata[1],
                exploration_result(ENVIRONMENT_DEGRADED, "accept", "ok", "seed-3"),
            ),
        ]
        report = account(rows, strata, EDENFS_TARGET)
        self.assertEqual(1 / 3, report.harness_failure_rate)
        self.assertEqual(1, report.completed)
        self.assertEqual(1, report.harness_failures)
        self.assertEqual(1, report.environment_degraded)
        self.assertEqual((strata[1],), report.missing_strata)
        self.assertEqual(
            [(1, "bytes/stale-read: observed old bytes", "seed-1")],
            [(item.seed, item.reason, item.artifacts) for item in report.rejects],
        )

    def _fault_spec(self, behavior: str) -> RunSpec:
        """The worked example (`f1` installs on `c0`, `h1` awaits, `x1` removes)."""
        document = json.loads(WORKED_EXAMPLE)
        actions = document["actions"]
        actions["f1"]["behavior"] = behavior
        if behavior != "block":
            actions["c0"]["deadline"] = "operation"
            actions["h1"]["requires"] = [["c0", "finished"]]
        return from_json(json.dumps(document), EDENFS_TARGET)

    def _fault_attempt(
        self,
        root: Path,
        seed: int,
        stratum: tuple[str, str, str],
        install: str | None,
        cleanup: dict[str, Any] | None,
        status: str = COMPLETED,
    ) -> CampaignAttempt:
        """Write one run whose install completed `install` (None: never), whose
        await completed `ok`, and whose unwind recorded `cleanup` facts."""
        events = [
            {"type": history.INVOKE, "time": 0.0, "action": "f1"},
            {"type": history.INVOKE, "time": 2.0, "action": "c0"},
            # c0 finishes before the await starts, as an error stratum requires.
            {"type": history.OK, "time": 2.5, "action": "c0"},
            {"type": history.INVOKE, "time": 3.0, "action": "h1"},
            {"type": history.OK, "time": 4.0, "action": "h1", "fault_id": "F"},
        ]
        if install is not None:
            events.append(
                {"type": install, "time": 1.0, "action": "f1", "fault_id": "F"}
            )
        if cleanup is not None:
            events.append({"type": history.CLEANUP_REMOVED, "time": 9.0, **cleanup})
        events.sort(key=lambda event: event["time"])
        path = root / f"seed-{seed}"
        path.mkdir()
        (path / "run-spec.json").write_text(to_json(self._fault_spec(stratum[1])))
        (path / "history.jsonl").write_text(
            "".join(json.dumps(e) + "\n" for e in events)
        )
        return CampaignAttempt(
            seed, stratum, exploration_result(status, "accept", "ok", str(path))
        )

    def test_account_tabulates_witnessed_installs_per_site(self) -> None:
        """Per site: admitted strata, installs, witnessed installs, completed runs.

        An install is the install kind's `ok` completion; the target decides
        from the `cleanup-removed` facts whether it was witnessed.
        """
        strata = (("site", "block", "none"), ("site", "error", "none"))
        removed = {"fault_id": "F", "hit": False, "miss": False}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows = [
                self._fault_attempt(
                    root, 0, strata[0], history.OK, {**removed, "hit": True}
                ),
                self._fault_attempt(
                    root, 1, strata[1], history.OK, {**removed, "miss": True}
                ),
                self._fault_attempt(
                    root,
                    2,
                    strata[0],
                    history.OK,
                    {**removed, "hit": True},
                    HARNESS_FAILURE,
                ),
                self._fault_attempt(root, 3, strata[1], None, None),
                CampaignAttempt(
                    9, strata[0], exploration_result(COMPLETED, "accept", "ok", "")
                ),
            ]
            report = account(rows, strata, EDENFS_TARGET)
        self.assertEqual(
            {"site": {"admitted": 2, "installs": 3, "witnessed": 2, "completed": 4}},
            report.sites,
        )

    def test_account_loads_each_run_once_for_every_table(self) -> None:
        """The funnel and site tables share one load of each run's artifacts."""
        stratum = ("site", "block", "none")
        removed = {"fault_id": "F", "hit": True, "miss": False}
        with tempfile.TemporaryDirectory() as tmp:
            rows = [
                self._fault_attempt(Path(tmp), seed, stratum, history.OK, removed)
                for seed in (0, 1)
            ]
            with mock.patch.object(
                campaign, "_load_run", wraps=campaign._load_run
            ) as load:
                report = account(rows, (stratum,), EDENFS_TARGET)
        self.assertEqual(
            sorted(row.result.dir for row in rows),
            sorted(call.args[0] for call in load.call_args_list),
        )
        self.assertEqual(2, report.sites["site"]["installs"])

    def test_a_torn_run_spec_is_accounted_as_an_unloaded_run(self) -> None:
        """A torn `run-spec.json` counts its run as not loaded, the same as a
        missing one; accounting never raises on it."""
        stratum = ("site", "block", "none")
        removed = {"fault_id": "F", "hit": True, "miss": False}
        with tempfile.TemporaryDirectory() as tmp:
            row = self._fault_attempt(Path(tmp), 0, stratum, history.OK, removed)
            Path(row.result.dir, "run-spec.json").write_text('{"schema": ')
            report = account([row], (stratum,), EDENFS_TARGET)
        self.assertEqual(
            {"site": {"admitted": 1, "installs": 0, "witnessed": 0, "completed": 1}},
            report.sites,
        )

    def test_install_count_keys_on_the_install_kind_not_other_fault_events(
        self,
    ) -> None:
        """Only the install kind's `ok` completion is an install: an `ok`
        `fault_await` carrying `fault_id` is not one, and an install whose cleanup
        facts carry no witness stays unwitnessed."""
        stratum = ("site", "block", "none")
        unwitnessed = {"fault_id": "F", "hit": False, "miss": True}
        for install, installs in ((history.FAIL, 0), (history.OK, 1)):
            with self.subTest(install=install), tempfile.TemporaryDirectory() as tmp:
                row = self._fault_attempt(Path(tmp), 0, stratum, install, unwitnessed)
                site = account([row], (stratum,), EDENFS_TARGET).sites["site"]
            self.assertEqual((installs, 0), (site["installs"], site["witnessed"]))

    def test_error_install_is_witnessed_when_its_fault_was_consumed(self) -> None:
        """An error install never parks (`hit: false`); `miss: false` is its witness."""
        stratum = ("site", "error", "none")
        consumed = {"fault_id": "F", "hit": False, "miss": False, "consumed": True}
        with tempfile.TemporaryDirectory() as tmp:
            row = self._fault_attempt(Path(tmp), 0, stratum, history.OK, consumed)
            report = account([row], (stratum,), EDENFS_TARGET)
        self.assertEqual(
            {"site": {"admitted": 1, "installs": 1, "witnessed": 1, "completed": 1}},
            report.sites,
        )

    def test_fake_target_reports_an_empty_sites_table(self) -> None:
        """A target without fault sites admits no strata and tabulates nothing."""
        row = CampaignAttempt(
            0, None, exploration_result(COMPLETED, "accept", "ok", "")
        )
        self.assertEqual({}, account([row], (), FAKE).sites)

    @mock.patch.object(containment, "run_exploration")
    def test_campaign_sums_property_results(self, run: mock.Mock) -> None:
        """Both report files sum every property's obligations by status, not-evaluated reasons, errors, findings, and coverage."""
        results = (
            PropertyResult(
                "journal",
                (
                    _obligation("journal.acked", ObligationStatus.SATISFIED, "a"),
                    _obligation(
                        "journal.acked",
                        ObligationStatus.NOT_EVALUATED,
                        "b",
                        not_evaluated="journal.incomplete-window",
                        reason="window",
                    ),
                ),
                coverage={"journal.acked-write-observed": 2},
            ),
            PropertyResult(
                "cross-channel",
                (
                    _obligation(
                        "cross-channel.byte-disagreement",
                        ObligationStatus.VIOLATED,
                        "c",
                        attribution=Attribution.UNDETERMINED,
                        reason="bytes/fuse≠thrift: unequal",
                        channel="fuse",
                    ),
                ),
            ),
            PropertyResult("status", error=property_error("KeyError", "boom")),
        )
        run.return_value = ExplorationResult(
            Execution(COMPLETED, "ok", 1, 0), synthetic_verdict(*results), "seed"
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _main(
                cli_argv(
                    "campaign",
                    target="fake",
                    seeds=2,
                    artifacts=root / "runs",
                    report=root / "report.json",
                )
            )
            for path in (root / "report.json", root / "runs" / "report.json"):
                document = json.loads(path.read_text())
                totals = document["properties"]
                self.assertEqual(
                    {
                        "journal": {"satisfied": 2, "violated": 0, "not-evaluated": 2},
                        "cross-channel": {
                            "satisfied": 0,
                            "violated": 2,
                            "not-evaluated": 0,
                        },
                        "status": {"satisfied": 0, "violated": 0, "not-evaluated": 0},
                    },
                    {
                        pid: t["obligations"]
                        for pid, t in totals.items()
                        if not pid.startswith("core.")
                    },
                )
                self.assertEqual(
                    {"journal.incomplete-window": 2}, totals["journal"]["not_evaluated"]
                )
                self.assertEqual(
                    {"journal.acked-write-observed": 4}, totals["journal"]["coverage"]
                )
                self.assertEqual(
                    {"cross-channel.byte-disagreement": 2},
                    totals["cross-channel"]["findings"],
                )
                self.assertEqual(
                    ({"KeyError": 2}, 2),
                    (
                        totals["status"]["errors"],
                        totals["status"]["no_obligation_runs"],
                    ),
                )
                funnel = {
                    (row["family"], row["channel"]): row
                    for row in document["obligations"]
                }
                self.assertEqual(
                    {
                        "applicable": 2,
                        "observed": 0,
                        "evaluated": 2,
                        "not_evaluated": 0,
                        "violated": 2,
                    },
                    {
                        k: v
                        for k, v in funnel[("cross-channel", "fuse")].items()
                        if k not in ("family", "channel", "mode")
                    },
                )
                self.assertEqual(
                    (4, 2, 2),
                    tuple(
                        funnel[("journal", "-")][k]
                        for k in ("applicable", "evaluated", "not_evaluated")
                    ),
                )


class ActionSubjectsTest(unittest.TestCase):
    def test_a_subtree_footprint_is_unbounded(self) -> None:
        """A subtree footprint feeds every PATH subject; an exact one only its paths."""
        subtree = dataclasses.replace(
            FAKE.actions[READ],
            footprint=lambda _spec, action: Footprint(
                subtrees_read=frozenset({action.payload["path"]})
            ),
        )
        target = dataclasses.replace(FAKE, actions={**FAKE.actions, READ: subtree})
        spec = make_spec(target=target, actions=register_actions(r2="elsewhere"))
        self.assertIsNone(action_subjects(target, spec, "r2"))
        self.assertEqual(
            frozenset({"path:elsewhere"}), action_subjects(FAKE, spec, "r2")
        )
        self.assertIsNone(action_subjects(FAKE, spec, "missing"))


class ObligationAccountingTest(unittest.TestCase):
    """A real fake campaign reports the obligation funnel by family, channel, and mode."""

    def test_fake_campaign_funnel(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
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
            report = json.loads((root / "report.json").read_text())
            specs = [
                json.loads(
                    (root / "runs" / f"seed-{seed}" / "run-spec.json").read_text()
                )
                for seed in (0, 1)
            ]
        reads = sum(
            action["kind"] == "read"
            for spec in specs
            for action in spec["actions"].values()
        )
        actions = {(row["family"], row["mode"]): row for row in report["actions"]}
        funnel = {
            (row["family"], row["channel"], row["mode"]): row
            for row in report["obligations"]
        }
        self.assertEqual(
            {
                "family": "bytes",
                "mode": "primary",
                "planned": reads,
                "started": reads,
                "recorded": reads,
            },
            actions[("bytes", "primary")],
        )
        self.assertNotIn(("bytes", "-", "primary"), funnel)
        self.assertFalse([row for row in report["obligations"] if "planned" in row])
        observed = funnel[("bytes", "fake", "primary")]
        self.assertEqual(observed["applicable"], observed["observed"])
        self.assertEqual(
            observed["applicable"], observed["evaluated"] + observed["not_evaluated"]
        )
        self.assertEqual(
            report["properties"]["bytes"]["obligations"]["satisfied"]
            + report["properties"]["bytes"]["obligations"]["violated"],
            observed["evaluated"],
        )
        self.assertGreater(observed["applicable"], 0)
        self.assertEqual(0, observed["violated"])
        self.assertIn(("core.input-legality", "-", "primary"), funnel)

    def _attempt(self, root: Path, undecoded: str | None) -> CampaignAttempt:
        """A real fake run of reads of two paths, one read's result undecodable."""
        run_dir = root / f"seed-{undecoded}"
        run_dir.mkdir()
        spec = make_spec(
            target=FAKE,
            actions={
                "w": {
                    "kind": "write",
                    "deadline": "operation",
                    "path": "register",
                    "value_id": "written",
                },
                "r1": {"kind": "read", "deadline": "operation", "path": "register"},
                "r2": {"kind": "read", "deadline": "operation", "path": "elsewhere"},
            },
        )
        events = [
            (
                history.Event(e.type, e.time, {**e.data, "bogus": 1})
                if e.type == "ok" and e.data.get("action") == undecoded
                else e
            )
            for e in history_of(spec, FAKE)
        ]
        (run_dir / "run-spec.json").write_text(to_json(spec))
        with open(run_dir / "history.jsonl", "wb") as handle:
            for event in events:
                handle.write(history.event_line(event))
        verdict = check(
            FAKE, spec, events, execution={**COMPLETED_EXECUTION, "events": len(events)}
        )
        result = ExplorationResult(
            Execution(COMPLETED, "ok", len(events), 0), verdict, str(run_dir)
        )
        return CampaignAttempt(0, None, result)

    def test_observed_counts_obligations_whose_inputs_were_all_recorded(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            clean = funnel_accounting([self._attempt(root, None)], FAKE)
            gapped = funnel_accounting([self._attempt(root, "r2")], FAKE)

        def rows(table: list[dict[str, Any]], family: str) -> list[dict[str, Any]]:
            return [r for r in table if r["family"] == family]

        (clean_reads,) = rows(clean[0], "bytes")
        (gapped_reads,) = rows(gapped[0], "bytes")
        self.assertEqual((2, 2, 2), tuple(clean_reads[k] for k in ACTION_FUNNEL))
        self.assertEqual((2, 2, 1), tuple(gapped_reads[k] for k in ACTION_FUNNEL))
        legality = {
            name: next(r for r in table[1] if r["family"] == "core.input-legality")
            for name, table in (("clean", clean), ("gapped", gapped))
        }
        self.assertEqual(
            {"clean": (3, 3), "gapped": (3, 2)},
            {
                name: (row["applicable"], row["observed"])
                for name, row in legality.items()
            },
        )
        schema = next(r for r in gapped[1] if r["family"] == "core.result-schema")
        self.assertEqual((3, 0), (schema["applicable"], schema["observed"]))
