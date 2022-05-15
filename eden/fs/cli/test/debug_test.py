#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import unittest
from typing import Dict, List, NamedTuple, Tuple, Union

from eden.fs.cli.debug import FileStatsCMD


class DebugTest(unittest.TestCase):
    def test_get_largest_directories_by_count(self) -> None:
        class TestCase(NamedTuple):
            test_input: Tuple[List[Tuple[str, int]], int]
            test_output: List[Dict[str, Union[int, str]]]
            msg: str

        test_cases: List[TestCase] = [
            TestCase(
                test_input=([], 0),
                test_output=[{"path": ".", "file_count": 0}],
                msg="empty directory with minimum of 1",
            ),
            TestCase(
                test_input=([], 1),
                test_output=[],
                msg="empty directory with minimum of 1",
            ),
            TestCase(
                test_input=([("dirA/filename", 1000)], 1),
                test_output=[
                    {"path": ".", "file_count": 1},
                    {"path": "dirA", "file_count": 1},
                ],
                msg="single file with minimum of 1",
            ),
            TestCase(
                test_input=([("dirA/filename", 1000)], 2),
                test_output=[],
                msg="single file with minimum of 2",
            ),
            TestCase(
                test_input=([("dirA/filename", 1000), ("dirB/filename2", 50)], 1),
                test_output=[
                    {"path": ".", "file_count": 2},
                    {"path": "dirA", "file_count": 1},
                    {"path": "dirB", "file_count": 1},
                ],
                msg="two files with minimum of 1",
            ),
            TestCase(
                test_input=([("dirA/filename", 1000), ("dirB/filename2", 50)], 2),
                test_output=[{"path": ".", "file_count": 2}],
                msg="two files with minimum of 2",
            ),
            TestCase(
                test_input=([("filename", 1000), ("dirA/filename2", 50)], 1),
                test_output=[
                    {"path": ".", "file_count": 2},
                    {"path": "dirA", "file_count": 1},
                ],
                msg="file in root dir",
            ),
            TestCase(
                test_input=([("filename", 1000), ("dirA/dirB/dirC/filename2", 50)], 1),
                test_output=[
                    {"path": ".", "file_count": 2},
                    {"path": "dirA", "file_count": 1},
                    {"path": "dirA/dirB", "file_count": 1},
                    {"path": "dirA/dirB/dirC", "file_count": 1},
                ],
                msg="deeply nested file",
            ),
        ]

        for test_case in test_cases:
            path_and_sizes, min_file_count = test_case.test_input
            self.assertEqual(
                FileStatsCMD.get_largest_directories_by_count(
                    path_and_sizes, min_file_count
                ),
                test_case.test_output,
                test_case.msg,
            )
