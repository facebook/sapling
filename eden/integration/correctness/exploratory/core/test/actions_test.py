# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import unittest

from eden.integration.correctness.exploratory.core.actions import CORE_ACTION_DEFS
from eden.integration.correctness.exploratory.core.facts import (
    decode_result,
    ResultProblem,
    TypedResult,
)


class CoreActionsTest(unittest.TestCase):
    def test_core_kinds_exact(self) -> None:
        self.assertEqual(
            frozenset(
                {
                    "sleep",
                    "fault_install",
                    "fault_await",
                    "fault_remove",
                    "kill_process",
                    "blocked_faults",
                }
            ),
            frozenset(CORE_ACTION_DEFS),
        )

    def test_fault_await_has_a_fixed_schema(self) -> None:
        schema = CORE_ACTION_DEFS["fault_await"].result
        hit = {"fault_id": "F", "hit": True, "witness": "/mount"}
        completed = {"fault_id": "F", "target_completed": True, "target_outcome": "ok"}
        for raw in (hit, completed):
            with self.subTest(raw=raw):
                self.assertIsInstance(decode_result(schema, "ok", raw), TypedResult)
        merged = decode_result(schema, "ok", {**completed, "bytes_written": 64})
        self.assertIsInstance(merged, ResultProblem)
        assert isinstance(merged, ResultProblem)
        self.assertIn("undeclared fields ['bytes_written']", merged.problems[0])
