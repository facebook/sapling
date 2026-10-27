# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Generated legal histories through `EDENFS_TARGET`: no false rejects, planted counts caught, sparse scale.

Histories come from `test/legal_simulator.py`, a simulator that shares
no code with the contracts, the collectors, or the engine, but shares
their modelling assumptions (named in its docstring) and covers only
registers, so only modified-modified conflicts. The legal batch must
never reject. The same batch with every acked Thrift
checkout's conflict count raised by one (a planted contradiction) is
judged by properties, never by seed lists: each planted count is violated,
or satisfied with a witness (server points inside the same windows that
reproduce every recorded result with the planted count), or not evaluated
for the engine's deterministic budget only; most are violated and few run
out of budget. Histories whose largest legal component exceeds
`SMALL_COMPONENT` nodes take most of the time, so this module judges the
planted counts of the others and `legal_batch_slow_test` all of them
(`test/planted_counts.py`). The sparse test re-judges legal
histories over 10k, 100k, and 1M-file repositories: the verdict is
identical and every deterministic work counter saturates by 100k files, so
nothing enumerates the repository.
"""

from __future__ import annotations

import unittest
from collections import Counter
from typing import Any

from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    REJECT,
    Verdict,
)
from eden.integration.correctness.exploratory.test.legal_simulator import (
    judge as _judge,
)
from eden.integration.correctness.exploratory.test.planted_counts import (
    check_planted_counts,
    legal_verdict,
    SEEDS,
)
from eden.integration.correctness.exploratory.test.waiting import Waiting

SPARSE_SIZES = (10_000, 100_000, 1_000_000)
# Measured 2026-10-24, seeds 0..399: 394
# accepted, 1 not evaluated (budget), 5 not modeled (the `--continue`
# destination rule). A history whose checkout walk overlaps a remove or
# transfer is refused and waits on T293953113; the floor counts it.
LEAST_ACCEPTED = 394
MOST_NOT_EVALUATED = 1


def _work(verdict: Verdict) -> dict[str, Any]:
    assert verdict.report is not None
    work = verdict.report.work
    return {
        "phases": {p: dict(sorted(c.items())) for p, c in sorted(work.phases.items())},
        "components": len(work.components),
    }


class LegalBatchTest(unittest.TestCase):
    maxDiff = None

    def test_generated_legal_histories_are_never_rejected(self) -> None:
        kinds: Counter[str] = Counter()
        waiting = Waiting()
        rejected = []
        for seed in SEEDS:
            verdict = legal_verdict(seed)
            if verdict is None:
                kinds["not-modeled"] += 1
                continue
            if isinstance(verdict, str):
                waiting.counts[verdict] += 1
                continue
            kinds[verdict.kind] += 1
            if verdict.kind == REJECT:
                rejected.append((seed, verdict.reason))
        tally = dict(sorted(kinds.items()))
        waiting.report()
        self.assertEqual([], rejected, tally)
        self.assertGreaterEqual(kinds[ACCEPT] + waiting.total(), LEAST_ACCEPTED, tally)
        self.assertLessEqual(kinds["not-evaluated"], MOST_NOT_EVALUATED, tally)

    def test_a_planted_conflict_count_is_caught_on_small_histories(self) -> None:
        check_planted_counts(self, small_only=True)

    def test_sparse_repositories_do_not_change_the_verdict_or_the_work(self) -> None:
        """10k, 100k, and 1M files: one verdict; the work saturates by 100k.

        Equivalence-class representatives of untouched generated paths grow
        until every class is inhabited (the contracts gate in
        `edenfs_contracts_checkout_structure_test` measures the same saturation), so 100k and 1M
        files cost exactly the same work and 10k builds and propagates at
        most that. The search is not compared that way: its candidate order
        restarts after a work threshold (Luby), so a smaller problem may
        search longer (measured: seed 10, two refused checkouts after an
        interrupted `sl goto`, restarts 2 at 10k and 1 at 1M).
        """
        compared = 0
        waiting = Waiting()
        for seed in range(40):
            verdicts = [_judge(seed, files=files) for files in SPARSE_SIZES]
            task = next((v for v in verdicts if isinstance(v, str)), None)
            if task is not None:
                waiting.counts[task] += 1
                continue
            if None in verdicts:
                # A complete Thrift status over 1M files exceeds the 512 KiB
                # event limit and is refused at validation.
                continue
            compared += 1
            with self.subTest(seed=seed):
                small, medium, large = verdicts
                assert isinstance(small, Verdict)
                assert isinstance(medium, Verdict) and isinstance(large, Verdict)
                for verdict in (medium, large):
                    self.assertEqual(
                        (small.kind, small.reason), (verdict.kind, verdict.reason)
                    )
                self.assertEqual(_work(medium), _work(large))
                for phase, counters in _work(small)["phases"].items():
                    if phase == "search":
                        continue
                    for counter, value in counters.items():
                        self.assertLessEqual(
                            value, _work(large)["phases"][phase][counter]
                        )
        waiting.report()
        self.assertGreater(compared + waiting.total(), 10)

    def test_a_refused_walk_overlap_waits_on_its_task(self) -> None:
        self.assertEqual("T293953113", _judge(0))
