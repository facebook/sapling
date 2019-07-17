#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

from ..top import format_pids


class TopTest(unittest.TestCase):
    def test_format_0_pids(self):
        pid_str = format_pids([])
        self.assertEquals(pid_str, "")

    def test_format_1_pid(self):
        pid_str = format_pids(range(1))
        self.assertEquals(pid_str, "0")

    def test_format_many_pids(self):
        pid_str = format_pids(range(3))
        self.assertEquals(pid_str, "0, 1, 2")

    def test_format_pids_exceeds_limit(self):
        # The character limit is set to 25.
        # len("0, 1, 2, 3, 4, 5, 6, 7, 8") = 25
        # len("0, 1, 2, 3, 4, 5, 6, 7, 8, 9") = 28

        # The 9 does not fit within the character limit,
        # so only numbers 0-8 should be displayed.

        pid_str = format_pids(range(10))
        self.assertEquals(pid_str, "0, 1, 2, 3, 4, 5, 6, 7, 8")

    def test_format_pids_exceeds_limit_2(self):
        # The character limit is set to 25.
        # len("0, 1, 2, 3, 4, 5, 6, 7, 10") = 26
        # len("0, 1, 2, 3, 4, 5, 6, 7, 8") = 25

        # The 10 is too long to fit, but the 8 does fit.

        LONG_NUM = 10
        pids = list(range(8)) + [LONG_NUM, 8]
        pid_str = format_pids(pids)
        self.assertEquals(pid_str, "0, 1, 2, 3, 4, 5, 6, 7, 8")
