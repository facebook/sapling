# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Default solver budgets decide checkout-like legal histories.

Legal 55-node histories are never violated and are satisfied within the
default budgets, and the default decision budget keeps room for a fixed
140-node history that needs more than a hundredth of a component's share.
The full 140-node batches are in `solver_scale_slow_test`.
"""

from __future__ import annotations

import unittest

from eden.integration.correctness.exploratory.core import solver
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.test.problems import (
    check_legal_batches,
    mixed_history,
)

# (groups, observations, cells, acked rate, seeds, decided at least).
# 30/25/8 is the 55-node `mixed_history` shape the solver budgets were
# calibrated on (`solver.py`); the acked rate is the share of groups whose
# outcome is known. The floor is the number of seeds the default budgets
# satisfy, measured: all 300, so any budget-limited seed is a regression.
BATCHES = ((30, 25, 8, 0.6, range(300), 300),)
# A legal 140-node history (seed, groups, observations, cells, acked rate)
# whose largest component needs about 1.7M decision work: about 1/48 of a
# component's default share and twice what a 100x smaller budget leaves.
CAPACITY_HISTORY = (18, 80, 60, 12, 0.6)


class LegalHistoryScaleTest(unittest.TestCase):
    def test_legal_histories_within_default_budgets(self) -> None:
        """A legal history is never violated; most are satisfied within budget."""
        for batch, (satisfied, summary) in zip(
            BATCHES, check_legal_batches(self, BATCHES), strict=True
        ):
            self.assertGreaterEqual(satisfied, batch[-1], summary)

    def test_the_default_budgets_decide_the_capacity_history(self) -> None:
        """The largest calibrated legal history is satisfied within one component share."""
        solution = solver.solve(mixed_history(*CAPACITY_HISTORY))
        self.assertEqual(
            {ObligationStatus.SATISFIED},
            {o.status for o in solution.outcomes.values()},
        )
        share = solver.DECISION_WORK // solver.COMPONENT_SHARE
        work = max(c["decision_work"] for c in solution.components.values())
        self.assertLessEqual(work, share)
