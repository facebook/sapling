# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Planted conflict counts over the generated legal batch, judged by properties.

Shared by `legal_batch_test` (histories whose largest legal component has
at most `SMALL_COMPONENT` nodes) and `legal_batch_slow_test` (every
history).
"""

from __future__ import annotations

import functools
import unittest
from collections import Counter

from eden.integration.correctness.exploratory.core.checker import REJECT, Verdict
from eden.integration.correctness.exploratory.test.legal_simulator import (
    count_witness,
    judge,
    simulate,
)
from eden.integration.correctness.exploratory.test.waiting import Waiting

SEEDS = range(400)
COUNT_RULE = "checkout.conflict-count"
BUDGET_REASONS = {
    "core.solver.budget",
    "core.solver.explanation-budget",
    "core.solver.history-budget",
}
# The default lane's bound on a legal history's largest component. The
# planted counts of 31..40-node histories cost most of the lane's time
# and catch no planted bug the smaller ones miss; the slow lane judges
# them.
SMALL_COMPONENT = 30
# Least planted counts violated and most left to the budget, by lane
# (`small_only`). Measured 2026-10-24, seeds 0..399: 79
# violated, 3 satisfied, 0 budget among histories of at most 30 nodes;
# 169/10/24 over all. Every budgeted history has a component of 40 or
# more nodes. A history whose checkout walk overlaps a remove or transfer
# is refused and waits on T293953113; the floors count it.
LEAST_VIOLATED = {True: 79, False: 169}
MOST_BUDGET = {True: 0, False: 24}


def _statuses(verdict: Verdict, rule: str) -> set[str]:
    assert verdict.report is not None
    return {
        o.status.value
        for result in verdict.report.results
        for o in result.obligations
        if o.rule_id == rule
    }


def _reasons(verdict: Verdict, rule: str) -> set[str]:
    assert verdict.report is not None
    return {
        o.not_evaluated_reason
        for result in verdict.report.results
        for o in result.obligations
        if o.rule_id == rule and o.not_evaluated_reason is not None
    }


@functools.cache
def legal_verdict(seed: int) -> Verdict | str | None:
    """Return the legal history's verdict, judged once per process."""
    return judge(seed)


def _planted(
    test: unittest.TestCase, seed: int, waiting: Waiting, *, small_only: bool
) -> str | None:
    """Classify one seed's planted count: violated, budget, satisfied, or None (unjudged).

    A seed whose only acked Thrift checkouts resume an interrupted one has
    nothing planted (`legal_simulator`) and is unjudged; a refused one is
    counted in `waiting` under its task.
    """
    legal = legal_verdict(seed)
    if legal is None or simulate(seed, "count").events == simulate(seed).events:
        return None
    if isinstance(legal, str):
        waiting.counts[legal] += 1
        return None
    judged = _statuses(legal, COUNT_RULE)
    if not judged or "not-evaluated" in judged:
        return None
    assert legal.report is not None
    largest = max(
        (c.get("nodes", 0) for c in legal.report.work.components.values()),
        default=0,
    )
    if small_only and largest > SMALL_COMPONENT:
        return None
    mutated = judge(seed, "count")
    assert isinstance(mutated, Verdict)
    statuses = _statuses(mutated, COUNT_RULE)
    if "violated" in statuses:
        test.assertEqual(REJECT, mutated.kind, seed)
        return "violated"
    if "not-evaluated" in statuses:
        test.assertLessEqual(_reasons(mutated, COUNT_RULE), BUDGET_REASONS, seed)
        return "budget"
    return "satisfied"


def check_planted_counts(test: unittest.TestCase, *, small_only: bool) -> None:
    """Every acked Thrift checkout reports one conflict too many.

    A count the legal history judged is violated, satisfied only with a
    legal witness, or left to the budget; violations dominate and budget
    exhaustion is rare. A count the legal history leaves not evaluated
    (`checkout.conflict-count-unmodeled`, a declared reason) is skipped.
    A refused history waits on its task; the floors count it as judged.
    Five violated seeds serve as controls: no points reproduce them.
    """
    tally: Counter[str] = Counter()
    waiting = Waiting()
    found: dict[str, list[int]] = {}
    for seed in SEEDS:
        outcome = _planted(test, seed, waiting, small_only=small_only)
        if outcome is not None:
            tally[outcome] += 1
            found.setdefault(outcome, []).append(seed)
    counts = dict(sorted(tally.items()))
    print(f"planted counts {counts}")
    waiting.report()
    test.assertGreater(sum(tally.values()) + waiting.total(), 50, counts)
    test.assertGreaterEqual(
        tally["violated"] + waiting.total(), LEAST_VIOLATED[small_only], counts
    )
    test.assertLessEqual(tally["budget"], MOST_BUDGET[small_only], counts)
    for seed in found.get("satisfied", []):
        with test.subTest(satisfied=seed):
            test.assertIsNotNone(count_witness(seed, 64))
    for seed in found["violated"][:5]:
        with test.subTest(violated=seed):
            test.assertIsNone(count_witness(seed, 200))
