#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import unittest
from io import StringIO

from .. import stats_print
from ..stats import DiagInfoCounters, get_hg_importer_counters


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
        matrix = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12], [13, 14, 15, 16]]
        expected_output = """\
              |      avg               1                2           3          4
              |      p50               5                6           7          8
access        |      p90               9               10          11         12
              |      p99              13               14          15         16
--------------------------------------------------------------------------------
"""

        out = StringIO()
        stats_print.write_latency_record("access", matrix, out)
        self.assertEqual(out.getvalue(), expected_output)

    def test_print_mem_status(self):
        dictionary = {
            "memory_free": 1_234_567,
            "memory_free_percent": 50,
            "memory_usage": 45_678_912,
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
        self.assertEqual("1.5 GB", stats_print.format_size(1_500_000_000))
        # rounds up
        self.assertEqual("1.6 GB", stats_print.format_size(1_590_000_000))
        self.assertEqual("123.4 MB", stats_print.format_size(123_400_000))
        self.assertEqual("12 B", stats_print.format_size(12))
        self.assertEqual("0", stats_print.format_size(0))


class HgImporterStatsTest(unittest.TestCase):
    def test_call_counts_are_zero_if_no_data_was_logged(self) -> None:
        counters: DiagInfoCounters = {}
        table = get_hg_importer_counters(counters)
        metrics = [
            "cat_file",
            "fetch_tree",
            "manifest",
            "manifest_node_for_commit",
            "prefetch_files",
        ]
        for metric in metrics:
            self.assertEqual(
                table.get(metric), [0, 0, 0, 0], f"Metric {metric} should be zero"
            )

    def test_cat_file_call_counts_are_extracted_from_counters(self) -> None:
        counters: DiagInfoCounters = {
            "hg_importer.cat_file.count": 10,
            "hg_importer.cat_file.count.3600": 9,
            "hg_importer.cat_file.count.60": 1,
            "hg_importer.cat_file.count.600": 7,
        }
        table = get_hg_importer_counters(counters)
        self.assertEqual(table.get("cat_file"), [1, 7, 9, 10])

    def test_table_includes_unknown_counters(self) -> None:
        counters: DiagInfoCounters = {
            "hg_importer.dog_file.count": 100,
            "hg_importer.dog_file.count.3600": 90,
            "hg_importer.dog_file.count.60": 10,
            "hg_importer.dog_file.count.600": 70,
        }
        table = get_hg_importer_counters(counters)
        self.assertEqual(table.get("dog_file"), [10, 70, 90, 100])
