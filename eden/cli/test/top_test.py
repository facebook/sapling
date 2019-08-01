#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

from facebook.eden.ttypes import AccessCounts

from ..top import Process, format_cmd, format_mount, format_time


class TopTest(unittest.TestCase):
    def setUp(self) -> None:
        self.process = Process(42, "ls", "fbsource")

    def test_increment_counts(self):
        self.assertEqual(self.process.access_counts, AccessCounts(0, 0, 0, 0))
        self.process.increment_counts(AccessCounts(42, 42, 42, 42))
        self.assertEqual(self.process.access_counts, AccessCounts(42, 42, 42, 42))

    def test_format_cmd(self):
        self.assertEqual(format_cmd(b"/bin/ls"), "ls")

    def test_format_cmd_with_arg(self):
        self.assertEqual(format_cmd(b"/bin/ls\x00-l"), "ls -l")

    def test_format_mount(self):
        self.assertEqual(format_mount("/data/users/zuck/fbsource"), "fbsource")

    def test_format_time(self):
        self.assertEqual(format_time(1), "1s")
        self.assertEqual(format_time(60), "1m")
        self.assertEqual(format_time(60 * 60), "1h")
        self.assertEqual(format_time(60 * 60 * 24), "1d")
