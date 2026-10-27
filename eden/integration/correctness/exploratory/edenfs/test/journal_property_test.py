# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The journal property: a write acknowledged before a complete same-generation journal
window began appears in that window. Complements the corpus family `journal`: these
tests pin the evidence an incomplete window keeps, which window decides, generation
establishment by records and daemon exits, which writes are covered, the omitting
window's witness, shared obligations, and the obligation bound linear in the writes.
"""

from __future__ import annotations

import dataclasses
import unittest
from collections.abc import Iterable, Mapping
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.effects import ObligationRef, Status
from eden.integration.correctness.exploratory.core.history import DAEMON_EXIT, Event
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    MAX_RUN_OBLIGATIONS,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.properties import journal
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge,
    judge_isolated,
    obligations_by_scope,
    REGISTERED_RULE_IDS,
    statuses_by_scope,
)
from eden.integration.correctness.exploratory.test.corpus.author import (
    R0,
    Run,
    WRITE_OK,
)


JOURNAL = journal.MISSING_ACKED_WRITE


def _window(listed: Iterable[str] = (), **overrides: Any) -> dict[str, Any]:
    facts: dict[str, Any] = {
        "mount_generation": 1,
        "sequence_number": 9,
        "since_sequence": 0,
        "changed_paths": [
            {"path": path, "existed_before": True, "existed_after": True}
            for path in sorted(listed)
        ],
        "truncated": False,
        "lost_changes": [],
    }
    facts.update(overrides)
    return facts


class _RecordingSink:
    """A CollectorSink that keeps each obligation's outcome by scope."""

    def __init__(self) -> None:
        self.found: dict[str, str] = {}
        self.groups: dict[str, str] = {}

    def obligation(
        self, rule_id: str, scope: str, *, subject: Any, witnesses: Any = ()
    ) -> ObligationRef:
        self.found.setdefault(scope, "declared")
        return ObligationRef(rule_id, scope)

    def observe(self, observation: Any) -> None:
        pass

    def constrain(self, constraint: Any) -> None:
        self.found[constraint.obligation.scope] = "constrained"
        self.groups[constraint.obligation.scope] = constraint.group

    def covered(self, obligation_id: str, count: int = 1) -> None:
        pass

    def satisfied(self, ref: ObligationRef) -> None:
        self.found[ref.scope] = "satisfied"

    def violated(
        self,
        ref: ObligationRef,
        attribution: Attribution,
        reason: str,
        witnesses: Iterable[Witness] = (),
    ) -> None:
        self.found[ref.scope] = "violated"

    def violated_unless_engine(
        self,
        ref: ObligationRef,
        attribution: Attribution,
        reason: str,
        witnesses: Iterable[Witness] = (),
    ) -> None:
        self.found[ref.scope] = "violated-unless-engine"

    def not_evaluated(
        self, ref: ObligationRef, reason_id: str, detail: str = ""
    ) -> None:
        self.found[ref.scope] = reason_id


def _daemon_exit(run: Run) -> None:
    run.run_event(
        "info",
        reason=DAEMON_EXIT,
        returncode=-9,
        signal="SIGKILL",
        log_tail=[],
        core_path=None,
    )


def _journal_run(
    window: Mapping[str, Any] | None,
    *,
    since: int | None = 0,
    overlap: bool = False,
    write_outcome: str = "ok",
) -> Run:
    """A write to r0, then (or, with `overlap`, across) journal window `j`."""
    run = Run("t", "journal", "a write and a journal window")
    run.act("a", "w", "write", path=R0, value_id="w")
    payload: dict[str, Any] = {} if since is None else {"since_sequence": since}
    run.act("b", "j", "journal_window", **payload)
    write_facts: dict[str, Any] = (
        dict(WRITE_OK) if write_outcome == "ok" else {"reason": "operation-timeout"}
    )
    if overlap:
        run.inv("w", "j")
        run.done("j", "ok", **(window or {}))
        run.done("w", write_outcome, **write_facts)
    else:
        run.call("w", write_outcome, **write_facts)
        if window is None:
            run.call("j", "fail", reason="daemon gone", source="thrift")
        else:
            run.call("j", "ok", **window)
    run.finish()
    return run


class JournalTest(unittest.TestCase):
    def test_completeness_twins(self) -> None:
        """The old twins: only complete windows after an ack decide; gaps are kept.

        A twin the journal rule does not judge still accepts: the namespace and
        result-shape rules judge its write.
        """
        scope = f"j:{R0}"
        cases: list[tuple[str, Run, dict[str, str], str]] = [
            (
                "lost",
                _journal_run(_window(lost_changes=["MOUNT_GENERATION_CHANGED"])),
                {scope: "not-evaluated"},
                "not-evaluated",
            ),
            (
                "nonzero",
                _journal_run(_window(since_sequence=12), since=12),
                {scope: "not-evaluated"},
                "not-evaluated",
            ),
            (
                "truncated-but-listed",
                _journal_run(_window([R0], truncated=True)),
                {scope: "satisfied"},
                "accept",
            ),
            (
                "cursor",
                _journal_run({"mount_generation": 1, "sequence_number": 9}, since=None),
                {},
                "accept",
            ),
            ("failed-window", _journal_run(None), {}, "accept"),
        ]
        for name, run, expected, kind in cases:
            with self.subTest(case=name):
                verdict = judge(run)
                self.assertEqual(expected, statuses_by_scope(verdict, JOURNAL))
                self.assertEqual(kind, verdict.kind, verdict.reason)

    def test_violation_is_an_engine_proof_on_must_appear_in(self) -> None:
        verdict = judge_isolated(("journal",), _journal_run(_window()))
        item = obligations_by_scope(verdict, JOURNAL)[f"j:{R0}"]
        self.assertEqual(Attribution.PRODUCT, item.attribution)
        self.assertIsNotNone(item.proof)
        assert verdict.report is not None
        proof = next(p for p in verdict.report.proofs if p.id == item.proof)
        self.assertEqual((item.rule_id,), tuple(r.rule_id for r in proof.obligations))
        self.assertEqual(1, len(proof.constraints))

    def test_incomplete_window_keeps_truncation_and_loss_evidence(self) -> None:
        lost = ["JOURNAL_TRUNCATED", "MOUNT_GENERATION_CHANGED"]
        verdict = judge_isolated(
            ("journal",), _journal_run(_window(truncated=True, lost_changes=lost))
        )
        item = obligations_by_scope(verdict, JOURNAL)[f"j:{R0}"]
        self.assertEqual(journal.INCOMPLETE_WINDOW, item.not_evaluated_reason)
        gaps = {w.value for w in item.witnesses if w.role == "window-gap"}
        self.assertEqual({"j: truncated=True", f"j: lost_changes={lost}"}, gaps)
        self.assertIn("truncated=True", item.reason or "")

    def test_a_write_listed_in_its_generation_owes_a_newer_one_nothing(self) -> None:
        """w is listed by generation-1 window j1; generation-2 window j2 omits it."""
        run = Run("t", "lists-then-newer", "a window per generation after one write")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "j1", "journal_window", since_sequence=0)
        run.act("a", "j2", "journal_window", since_sequence=0)
        run.call("w", "ok", **WRITE_OK)
        run.call("j1", "ok", **_window([R0]))
        run.call("j2", "ok", **_window(mount_generation=2))
        run.finish()
        verdict = judge_isolated(("journal",), run)
        self.assertEqual({f"j1:{R0}": "satisfied"}, statuses_by_scope(verdict, JOURNAL))
        self.assertEqual("accept", verdict.kind, verdict.reason)

    def test_the_latest_established_write_decides(self) -> None:
        """Peek k0 records generation 0, then w1, then peek k1 records generation 1,
        then w2, then generation-1 window j omits r0: w1's generation is
        unestablished but w2's is not, so the obligation is violated on w2.
        """
        run = Run("t", "latest", "two writes around a same-generation peek")
        run.act("a", "k0", "journal_window")
        run.act("a", "w1", "write", path=R0, value_id="w1")
        run.act("a", "k1", "journal_window")
        run.act("a", "w2", "write", path=R0, value_id="w2")
        run.act("a", "j", "journal_window", since_sequence=0)
        run.call("k0", "ok", mount_generation=0, sequence_number=3)
        run.call("w1", "ok", **WRITE_OK)
        run.call("k1", "ok", mount_generation=1, sequence_number=1)
        run.call("w2", "ok", **WRITE_OK)
        run.call("j", "ok", **_window())
        run.finish()
        verdict = judge_isolated(("journal",), run)
        self.assertEqual({f"j:{R0}": "violated"}, statuses_by_scope(verdict, JOURNAL))
        item = obligations_by_scope(verdict, JOURNAL)[f"j:{R0}"]
        self.assertIsNotNone(item.proof)
        self.assertEqual(Attribution.PRODUCT, item.attribution)
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        facts = checker.HistoryFacts.build(
            spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
        )
        derived = checker.contract_effects(
            EDENFS_TARGET, spec, facts, REGISTERED_RULE_IDS
        )
        assert not isinstance(derived, checker.PropertyError)
        sink = _RecordingSink()
        journal.collect_journal(CheckInput(spec, facts, *derived), sink)
        (group,) = [g for g in derived[1].groups if g.id == sink.groups[f"j:{R0}"]]
        self.assertEqual("w2", group.action)

    def test_the_latest_of_several_established_writes_is_named(self) -> None:
        run = Run("t", "latest-named", "two acked writes, then a window omitting r0")
        run.act("a", "w1", "write", path=R0, value_id="w1")
        run.act("a", "w2", "write", path=R0, value_id="w2")
        run.act("a", "j", "journal_window", since_sequence=0)
        run.call("w1", "ok", **WRITE_OK)
        run.call("w2", "ok", **WRITE_OK)
        run.call("j", "ok", **_window())
        run.finish()
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        facts = checker.HistoryFacts.build(
            spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
        )
        derived = checker.contract_effects(
            EDENFS_TARGET, spec, facts, REGISTERED_RULE_IDS
        )
        assert not isinstance(derived, checker.PropertyError)
        sink = _RecordingSink()
        journal.collect_journal(CheckInput(spec, facts, *derived), sink)
        (group,) = [g for g in derived[1].groups if g.id == sink.groups[f"j:{R0}"]]
        self.assertEqual("w2", group.action)

    def test_daemon_exits_bound_the_generation(self) -> None:
        """An exit between the ack and the window: the window owes nothing. An exit
        before the write with no later record, or at the ack's instant (ties are
        not ordered): unestablished. A same-generation peek after the exit and
        before the write: established.
        """
        cases = (
            ("between", "satisfied", None),
            ("before", "not-evaluated", journal.GENERATION_UNESTABLISHED),
            ("before-then-peek", "violated", None),
            ("at-ack", "not-evaluated", journal.GENERATION_UNESTABLISHED),
        )
        for name, expected, reason in cases:
            with self.subTest(case=name):
                run = Run("t", "exit", "a daemon exit around one write")
                if name == "before-then-peek":
                    run.act("a", "k", "journal_window")
                run.act("a", "w", "write", path=R0, value_id="w")
                run.act("a", "j", "journal_window", since_sequence=0)
                if name == "between":
                    run.call("w", "ok", **WRITE_OK)
                    _daemon_exit(run)
                elif name == "at-ack":
                    run.call("w", "ok", **WRITE_OK)
                    _daemon_exit(run)
                    exit_event = run.events.pop()
                    run.events.append(
                        Event("info", run.events[-1].time, exit_event.data)
                    )
                else:
                    _daemon_exit(run)
                    if name == "before-then-peek":
                        run.call("k", "ok", mount_generation=1, sequence_number=1)
                    run.call("w", "ok", **WRITE_OK)
                run.call("j", "ok", **_window())
                run.finish()
                verdict = judge_isolated(("journal",), run)
                self.assertEqual(
                    {f"j:{R0}": expected}, statuses_by_scope(verdict, JOURNAL)
                )
                item = obligations_by_scope(verdict, JOURNAL)[f"j:{R0}"]
                self.assertEqual(reason, item.not_evaluated_reason)

    def test_only_acked_writes_are_covered(self) -> None:
        """An indeterminate write owes the journal nothing even when a window began
        after its whole effect window; its acked twin is judged.

        The contracts keeps an indeterminate write's window open to
        `run-closed`, so no real window covers it: the twin replaces
        the contract group's status to exercise the ACKED filter directly.
        """
        run = Run("t", "indeterminate", "a write, then a complete window")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "j", "journal_window", since_sequence=0)
        run.call("w", "ok", **WRITE_OK)
        run.call("j", "ok", **_window())
        run.finish()
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        facts = checker.HistoryFacts.build(
            spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
        )
        derived = checker.contract_effects(
            EDENFS_TARGET, spec, facts, REGISTERED_RULE_IDS
        )
        assert not isinstance(derived, checker.PropertyError)
        pinned, effects = derived
        for status, expected in (
            (Status.ACKED, {f"j:{R0}": "constrained"}),
            (Status.INDETERMINATE, {}),
        ):
            with self.subTest(status=status):
                groups = tuple(
                    dataclasses.replace(g, status=status) if g.action == "w" else g
                    for g in effects.groups
                )
                twin = dataclasses.replace(effects, groups=groups)
                sink = _RecordingSink()
                journal.collect_journal(CheckInput(spec, facts, pinned, twin), sink)
                self.assertEqual(expected, sink.found)

    def test_a_write_acked_at_the_window_start_is_not_covered(self) -> None:
        """Ties are not ordered: an ack at the window's invocation instant is in flight."""
        run = Run("t", "tie", "a write acked at the instant the window begins")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("b", "j", "journal_window", since_sequence=0)
        run.call("w", "ok", **WRITE_OK)
        acked = run.events[-1].time
        run.events.append(Event("invoke", acked, {"action": "j", "actor": "b"}))
        run.done("j", "ok", **_window())
        run.finish()
        verdict = judge_isolated(("journal",), run)
        self.assertEqual({}, statuses_by_scope(verdict, JOURNAL))
        self.assertNotEqual("reject", verdict.kind, verdict.reason)

    def test_the_omitting_window_is_named(self) -> None:
        """j1 lists r0 and j2 omits it: the scope is j1, the evidence names j2."""
        run = Run("t", "later-drops", "two complete windows after one write")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "j1", "journal_window", since_sequence=0)
        run.act("a", "j2", "journal_window", since_sequence=0)
        run.call("w", "ok", **WRITE_OK)
        run.call("j1", "ok", **_window([R0]))
        run.call("j2", "ok", **_window())
        run.finish()
        item = obligations_by_scope(judge_isolated(("journal",), run), JOURNAL)[
            f"j1:{R0}"
        ]
        self.assertEqual("violated", str(item.status))
        self.assertIn(Witness("omitting-window", "j2"), item.witnesses)
        self.assertIn(Witness("window-action", "j1"), item.witnesses)

    def test_writes_with_the_same_covering_windows_share_one_obligation(self) -> None:
        run = Run("t", "shared", "two acked writes before one window, one after")
        for name in ("w1", "w2", "w3"):
            run.act("a", name, "write", path=R0, value_id=name)
        run.act("b", "j1", "journal_window", since_sequence=0)
        run.act("b", "j2", "journal_window", since_sequence=0)
        run.call("w1", "ok", **WRITE_OK)
        run.call("w2", "ok", **WRITE_OK)
        run.call("j1", "ok", **_window([R0]))
        run.call("w3", "ok", **WRITE_OK)
        run.call("j2", "ok", **_window())
        run.finish()
        verdict = judge_isolated(("journal",), run)
        self.assertEqual(
            {f"j1:{R0}": "violated", f"j2:{R0}": "violated"},
            statuses_by_scope(verdict, JOURNAL),
        )
        writers = {
            w.value
            for w in obligations_by_scope(verdict, JOURNAL)[f"j1:{R0}"].witnesses
            if w.role == "write-action"
        }
        self.assertEqual({"w1", "w2"}, writers)

    def test_obligations_stay_linear_in_writes(self) -> None:
        """64 paths x 300 windows would exceed the run bound; the writes bound it."""
        paths = [f"tree/registers/r{index}.reg" for index in range(64)]
        run = Run(
            "t", "many-windows", "64 writes, then 300 complete windows", registers=paths
        )
        for index, path in enumerate(paths):
            run.act("a", f"w{index}", "write", path=path, value_id=f"v{index}")
        for index in range(300):
            run.act("b", f"j{index}", "journal_window", since_sequence=0)
        for index in range(len(paths)):
            run.call(f"w{index}", "ok", **WRITE_OK)
        for index in range(300):
            run.call(f"j{index}", "ok", **_window(paths))
        run.finish()
        self.assertGreater(300 * len(paths), MAX_RUN_OBLIGATIONS)
        verdict = judge_isolated(("journal",), run)
        self.assertEqual("accept", verdict.kind, verdict.reason)
        self.assertEqual(
            {f"j0:{path}": "satisfied" for path in paths},
            statuses_by_scope(verdict, JOURNAL),
        )
