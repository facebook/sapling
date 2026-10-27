# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import contextlib
import io
import unittest

from eden.integration.correctness.exploratory.test.waiting import Waiting


class WaitingTest(unittest.TestCase):
    def test_a_failing_waiting_case_is_skipped_and_counted(self) -> None:
        waiting = Waiting()
        with self.assertRaisesRegex(unittest.SkipTest, "^waiting on T1$"):
            with waiting.case(self, "case", "T1"):
                self.fail("still broken")
        self.assertEqual({"T1": 1}, dict(waiting.counts))
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            waiting.report()
        self.assertEqual("waiting on T1: 1\n", out.getvalue())

    def test_a_passing_waiting_case_prints_the_notice_and_does_not_fail(
        self,
    ) -> None:
        waiting = Waiting()
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            with waiting.case(self, "case", "T1"):
                pass
        self.assertEqual("case now passes; remove the marker\n", out.getvalue())
        self.assertEqual(0, waiting.total())

    def test_a_case_without_a_task_fails_as_usual(self) -> None:
        with self.assertRaises(AssertionError):
            with Waiting().case(self, "case", None):
                self.fail("broken")
