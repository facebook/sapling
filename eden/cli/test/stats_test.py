#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import sys
import unittest
from io import StringIO

from .. import stats, stats_print


class StatsTest(unittest.TestCase):
    maxDiff = None

    def test_print_heading(self):
        expected_output = """\
                                   **********
                                   TheHeading
                                   **********

"""
        out = StringIO()
        stats_print.write_heading("TheHeading", out)
        self.assertEqual(out.getvalue(), expected_output)

    def test_print_latency_record(self):
        matrix = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12]]
        expected_output = """\
              |      p50               1                2           3          4
access        |      p90               5                6           7          8
              |      p99               9               10          11         12
--------------------------------------------------------------------------------
"""

        out = StringIO()
        stats_print.write_latency_record("access", matrix, out)
        self.assertEqual(out.getvalue(), expected_output)

    def test_print_mem_status(self):
        dictionary = {
            "memory_free": 1234567,
            "memory_free_percent": 50,
            "memory_usage": 45678912,
            "memory_usage_percent": 70,
        }
        expected_output = """\
                             memory free : 1.234567(GB)
                     memory free percent : 50%
                            memory usage : 45.678912(GB)
                    memory usage percent : 70%
"""
        out = StringIO()
        stats_print.write_mem_status_table(dictionary, out)
        self.assertEqual(expected_output, out.getvalue())

    def test_print_table(self):
        table = {
            "key1": [1, 2, 3, 4],
            "key2": [5, 6, 7, 8],
            "key3": [9, 10, 11, 12],
            "key4": [13, 14, 15, 16],
        }
        expected_output = """\
SystemCall      Last Minute       Last 10m      Last Hour       All Time
------------------------------------------------------------------------
key1                      1              2              3              4
key2                      5              6              7              8
key3                      9             10             11             12
key4                     13             14             15             16
"""
        out = StringIO()
        stats_print.write_table(table, "SystemCall", out)
        self.assertEqual(expected_output, out.getvalue())

    def test_print_table_with_shorter_header_and_key_column(self):
        table = {"key": [1, 2, 3, 4]}
        # Verifies the width of the first column depends on the header's and
        # key's lengths.
        expected_output = """\
SC       Last Minute       Last 10m      Last Hour       All Time
-----------------------------------------------------------------
key                1              2              3              4
"""
        out = StringIO()
        stats_print.write_table(table, "SC", out)
        self.assertEqual(expected_output, out.getvalue())

    def test_format_size(self):
        self.assertEqual("1.5 GB", stats_print.format_size(1500000000))
        # rounds up
        self.assertEqual("1.6 GB", stats_print.format_size(1590000000))
        self.assertEqual("123.4 MB", stats_print.format_size(123400000))
        self.assertEqual("12 B", stats_print.format_size(12))
        self.assertEqual("0", stats_print.format_size(0))

    def test_count_private_dirty_bytes(self):
        smaps = b"""\
7ff33e76c000-7ff33e770000 r--p 00025000 fc:03 263921                     /usr/lib64/libtinfo.so.5.9
Size:                 16 kB
Rss:                  16 kB
Pss:                  16 kB
Shared_Clean:          0 kB
Shared_Dirty:          0 kB
Private_Clean:         0 kB
Private_Dirty:        16 kB
Referenced:           16 kB
Anonymous:            16 kB
LazyFree:              0 kB
AnonHugePages:         0 kB
Shared_Hugetlb:        0 kB
Private_Hugetlb:       0 kB
Swap:                  0 kB
SwapPss:               0 kB
KernelPageSize:        4 kB
MMUPageSize:           4 kB
Locked:                0 kB
VmFlags: rd mr mw me ac
7ff33e770000-7ff33e771000 rw-p 00029000 fc:03 263921                     /usr/lib64/libtinfo.so.5.9
Size:                  4 kB
Rss:                   4 kB
Pss:                   4 kB
Shared_Clean:          0 kB
Shared_Dirty:          0 kB
Private_Clean:         0 kB
Private_Dirty:         4 kB
Referenced:            4 kB
Anonymous:             4 kB
LazyFree:              0 kB
AnonHugePages:         0 kB
Shared_Hugetlb:        0 kB
Private_Hugetlb:       0 kB
Swap:                  0 kB
SwapPss:               0 kB
KernelPageSize:        4 kB
MMUPageSize:           4 kB
Locked:                0 kB
VmFlags: rd wr mr mw me ac"""
        self.assertEqual(5 * 4096, stats.total_private_dirty(stats.parse_smaps(smaps)))

    @unittest.skipUnless(
        sys.platform == "linux", "/proc/self/smaps only exists on Linux"
    )
    def test_correctly_parses_real_smaps(self):
        with open("/proc/self/smaps", "rb") as f:
            smaps = f.read()
        total = stats.total_private_dirty(stats.parse_smaps(smaps))
        self.assertIsNotNone(total)
        self.assertGreater(total, 0)

    def test_parse_smaps_ignores_unknown_line_formats(self):
        smaps = b"""\
Weird line in front
7ff33e770000-7ff33e771000 rw-p 00029000 fc:03 263921                     /usr/lib64/libtinfo.so.5.9
Size:                  4 kB
No Colon
Private_Dirty:         4 kB
:Three:Colons:
line at end
        """
        mappings = stats.parse_smaps(smaps)
        self.assertEqual(1, len(mappings))
        self.assertEqual(4096, stats.total_private_dirty(mappings))

    def test_total_private_dirty_returns_None_if_unknown_value_format(self):
        self.assertEqual(0, stats.total_private_dirty([]))
        self.assertIs(None, stats.total_private_dirty([{b"Private_Dirty": b"4096 B"}]))
