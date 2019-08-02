#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

from ..top import Process, format_cmd, format_duration, format_mount


class TopTest(unittest.TestCase):
    def setUp(self) -> None:
        self.process = Process(42, "ls", "fbsource")

    def test_format_cmd(self):
        self.assertEqual(format_cmd(b"/bin/ls"), "ls")
        self.assertEqual(format_cmd(b"chg[worker/0]"), "chg[worker/0]")

    def test_format_cmd_with_arg(self):
        self.assertEqual(format_cmd(b"/bin/ls\x00-l"), "ls -l")

    def test_format_mount(self):
        self.assertEqual(format_mount("/data/users/zuck/fbsource"), "fbsource")

    def test_format_duration(self):
        self.assertEqual(format_duration(1), "1ns")
        self.assertEqual(format_duration(1 * 1000), "1us")
        self.assertEqual(format_duration(1 * 1000 * 1000), "1ms")
        self.assertEqual(format_duration(1 * 1000 * 1000 * 1000), "1s")
        self.assertEqual(format_duration(1 * 1000 * 1000 * 1000 * 60), "1m")
        self.assertEqual(format_duration(1 * 1000 * 1000 * 1000 * 60 * 60), "1h")
        self.assertEqual(format_duration(1 * 1000 * 1000 * 1000 * 60 * 60 * 24), "1d")
