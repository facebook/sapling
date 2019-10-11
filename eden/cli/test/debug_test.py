#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

from eden.cli.debug import FileStatsCMD


class DebugTest(unittest.TestCase):
    def test_get_largest_directories_by_count(self):

        test_cases = [
            {
                "input": ([], 0),
                "output": [{"path": ".", "file_count": 0}],
                "msg": "empty directory with no minimum",
            },
            {
                "input": ([], 1),
                "output": [],
                "msg": "empty directory with minimum of 1",
            },
            {
                "input": ([("dirA/filename", 1000)], 1),
                "output": [
                    {"path": ".", "file_count": 1},
                    {"path": "dirA", "file_count": 1},
                ],
                "msg": "single file with minimum of 1",
            },
            {
                "input": ([("dirA/filename", 1000)], 2),
                "output": [],
                "msg": "single file with minimum of 2",
            },
            {
                "input": ([("dirA/filename", 1000), ("dirB/filename2", 50)], 1),
                "output": [
                    {"path": ".", "file_count": 2},
                    {"path": "dirA", "file_count": 1},
                    {"path": "dirB", "file_count": 1},
                ],
                "msg": "two files with minimum of 1",
            },
            {
                "input": ([("dirA/filename", 1000), ("dirB/filename2", 50)], 2),
                "output": [{"path": ".", "file_count": 2}],
                "msg": "two files with minimum of 2",
            },
            {
                "input": ([("filename", 1000), ("dirA/filename2", 50)], 1),
                "output": [
                    {"path": ".", "file_count": 2},
                    {"path": "dirA", "file_count": 1},
                ],
                "msg": "file in root dir",
            },
            {
                "input": ([("filename", 1000), ("dirA/dirB/dirC/filename2", 50)], 1),
                "output": [
                    {"path": ".", "file_count": 2},
                    {"path": "dirA", "file_count": 1},
                    {"path": "dirA/dirB", "file_count": 1},
                    {"path": "dirA/dirB/dirC", "file_count": 1},
                ],
                "msg": "deeply nested file",
            },
        ]

        for test_case in test_cases:
            self.assertEqual(
                FileStatsCMD.get_largest_directories_by_count(*test_case["input"]),
                test_case["output"],
                test_case["msg"],
            )
