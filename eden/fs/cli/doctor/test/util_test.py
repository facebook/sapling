#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import unittest
from datetime import timedelta

from ..util import format_approx_duration


class UtilTest(unittest.TestCase):
    def test_format_approx_duration(self) -> None:
        self.assertEqual("2 days", format_approx_duration(timedelta(days=2, hours=1)))
        self.assertEqual("2 days", format_approx_duration(timedelta(days=2)))
        self.assertEqual("1 day", format_approx_duration(timedelta(days=1)))
        self.assertEqual(
            "23 hours", format_approx_duration(timedelta(days=1) - timedelta(seconds=1))
        )
        self.assertEqual(
            "59 minutes",
            format_approx_duration(timedelta(hours=1) - timedelta(seconds=1)),
        )
        self.assertEqual("1 minute", format_approx_duration(timedelta(minutes=1)))
        self.assertEqual(
            "59 seconds",
            format_approx_duration(timedelta(minutes=1) - timedelta(seconds=1)),
        )
        self.assertEqual("1 second", format_approx_duration(timedelta(seconds=1)))
        self.assertEqual(
            "a moment",
            format_approx_duration(timedelta(seconds=1) - timedelta(milliseconds=1)),
        )
        self.assertEqual("a moment", format_approx_duration(timedelta(seconds=0)))
        self.assertRaises(
            ValueError, lambda: format_approx_duration(timedelta(seconds=-1))
        )
