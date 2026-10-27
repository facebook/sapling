# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Planted conflict counts on every generated legal history.

Histories whose largest legal component exceeds `SMALL_COMPONENT` nodes
take most of the time (about 6 minutes), so this module has its own
target, run by continuous CI only. `legal_batch_test` judges the others.
"""

from __future__ import annotations

import unittest

from eden.integration.correctness.exploratory.test.planted_counts import (
    check_planted_counts,
)


class PlantedCountSlowTest(unittest.TestCase):
    def test_a_planted_conflict_count_is_caught_on_every_history(self) -> None:
        check_planted_counts(self, small_only=False)
