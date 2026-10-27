# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Default budgets on 140-node checkout-like legal histories.

The 60 x 140-node batches run until budget exhaustion on their hardest
seeds (about 9 minutes), so this module has its own target: continuous CI
only, with the `long_running` timeout tier.
"""

from __future__ import annotations

import unittest

from eden.integration.correctness.exploratory.core.test.problems import (
    check_legal_batches,
)

# (groups, observations, cells, acked rate, seeds, decided at least).
# 80/60/12 is the 140-node `mixed_history` calibration shape (`solver.py`),
# at three acked rates: fewer known outcomes leave more indeterminate
# groups to place, the hard case. Each floor is the number of seeds the
# default budgets satisfy, measured; at 0.6 one seed runs out of budget.
BATCHES = (
    (80, 60, 12, 0.6, range(7000, 7020), 19),
    (80, 60, 12, 0.85, range(7000, 7020), 20),
    (80, 60, 12, 0.95, range(7000, 7020), 20),
)


class LargeLegalHistoryScaleTest(unittest.TestCase):
    def test_large_legal_histories_within_default_budgets(self) -> None:
        for batch, (satisfied, summary) in zip(
            BATCHES, check_legal_batches(self, BATCHES), strict=True
        ):
            self.assertGreaterEqual(satisfied, batch[-1], summary)
