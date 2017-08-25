# Copyright (c) 2017-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals
from .. import stats_print
from io import StringIO
import unittest


class StatsTest(unittest.TestCase):
    def test_print_heading(self):
        expected_output = (
            '                                   **********                      '
            '             \n'
            '                                   TheHeading                      '
            '             \n'
            '                                   **********                      '
            '             \n\n'
        )
        out = StringIO()
        stats_print.write_heading('TheHeading', out)
        self.assertEqual(out.getvalue(), expected_output)

    def test_print_latency_record(self):
        matrix = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12]]
        expected_output = (
            '              |      P50          1              2             3   '
            '       4     \n'
            '   access     |      p90          5              6             7   '
            '       8     \n'
            '              |      p99          9             10             11  '
            '       12    \n'
            '-------------------------------------------------------------------'
            '-------------\n'
        )
        out = StringIO()
        stats_print.write_latency_record('access', matrix, out)
        self.assertEqual(out.getvalue(), expected_output)

    def test_print_mem_status(self):
        dictionary = {
            'memory_free': 1234567,
            'memory_free_percent': 50,
            'memory_usage': 45678912,
            'memory_usage_percent': 70
        }
        expected_output = (
            '                             memory free : 1.234567(GB)        \n'
            '                     memory free percent : 50%                 \n'
            '                            memory usage : 45.678912(GB)       \n'
            '                    memory usage percent : 70%                 \n'
        )
        out = StringIO()
        stats_print.write_mem_status_table(dictionary, out)
        self.assertEqual(out.getvalue(), expected_output)

    def test_print_table(self):
        table = {
            'key1': [1, 2, 3, 4],
            'key2': [5, 6, 7, 8],
            'key3': [9, 10, 11, 12],
            'key4': [13, 14, 15, 16]
        }
        expected_output = (
            '     SystemCall       Last minute  Last 10 minutes   Last Hour     '
            ' All Time    \n'
            '-------------------------------------------------------------------'
            '-------------\n'
            '        key1               1              2              3         '
            '     4       \n'
            '        key2               5              6              7         '
            '     8       \n'
            '        key3               9             10             11         '
            '    12       \n'
            '        key4              13             14             15         '
            '    16       \n'
        )
        out = StringIO()
        stats_print.write_table(table, 'SystemCall', out)
        self.assertEqual(out.getvalue(), expected_output)
