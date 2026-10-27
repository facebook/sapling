# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Planted conflict counts on the recorded seed-13008 history.

The recorded count is 0 (`checkout_count_property_test`). Planted counts 1-3
spend the engine's whole explanation budget (about 50 s each) and 4-6 are
violated, so this module has its own target, run by continuous CI only.
"""

from __future__ import annotations

import unittest

from eden.integration.correctness.exploratory.core.property import (
    SOLVER_EXPLANATION_BUDGET,
)
from eden.integration.correctness.exploratory.edenfs.test.recorded_count import (
    count_outcome,
    recorded_verdict,
)

PLANTED = range(1, 7)
# Measured 2026-10-22: 4, 5, and 6 violated; 1, 2, and 3 not
# evaluated (explanation budget).
LEAST_VIOLATED = 3


class PlantedRecordedCountTest(unittest.TestCase):
    def test_planted_counts_are_never_satisfied(self) -> None:
        outcomes = []
        for conflicts in PLANTED:
            with self.subTest(conflicts=conflicts):
                verdict = recorded_verdict(conflicts)
                outcome = count_outcome(verdict)
                outcomes.append(outcome)
                self.assertIn(
                    outcome,
                    (
                        ("violated", None),
                        ("not-evaluated", SOLVER_EXPLANATION_BUDGET),
                    ),
                )
                self.assertNotEqual("accept", str(verdict.kind))
        print(f"planted seed-13008 counts {list(PLANTED)}: {outcomes}")
        self.assertGreaterEqual(outcomes.count(("violated", None)), LEAST_VIOLATED)
