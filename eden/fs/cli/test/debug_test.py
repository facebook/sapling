#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import io
import sys
import unittest
from pathlib import Path
from typing import Dict, List, NamedTuple, Tuple, Union
from unittest.mock import MagicMock, patch

from eden.fs.cli.debug import FileStatsCMD, LocalRepoNameCmd
from eden.fs.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.test_support.temporary_directory import TemporaryDirectoryMixin


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


class LocalRepoNameCmdTest(unittest.TestCase, TemporaryDirectoryMixin):
    def _create_cmd(self) -> LocalRepoNameCmd:
        mock_parser = MagicMock(spec=argparse.ArgumentParser)
        return LocalRepoNameCmd(mock_parser)

    def _make_args(self, path: str | None = None) -> argparse.Namespace:
        return argparse.Namespace(
            path=path,
            config_dir=None,
            etc_eden_dir=None,
            home_dir=None,
        )

    def _setup_mock_checkout(
        self,
        mock_find_checkout: MagicMock,
        mount_name: str,
        rel_path: str,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount(mount_name)
        mock_find_checkout.return_value = (instance, checkout, Path(rel_path))

    def _run_and_capture_output(
        self, cmd: LocalRepoNameCmd, args: argparse.Namespace
    ) -> Tuple[int, str]:
        captured_output = io.StringIO()
        with patch.object(sys, "stdout", captured_output):
            result = cmd.run(args)
        return result, captured_output.getvalue().strip()

    @patch("eden.fs.cli.cmd_util.find_checkout")
    def test_no_args_in_repo_subdirectory(
        self,
        mock_find_checkout: MagicMock,
    ) -> None:
        """Test running localreponame with no args from a repo subdirectory."""
        self._setup_mock_checkout(mock_find_checkout, "my_custom_repo", "some/subdir")

        result, output = self._run_and_capture_output(
            self._create_cmd(), self._make_args()
        )

        self.assertEqual(result, 0)
        self.assertEqual(output, "my_custom_repo")

    @patch("eden.fs.cli.cmd_util.find_checkout")
    def test_no_args_in_repo_root(
        self,
        mock_find_checkout: MagicMock,
    ) -> None:
        """Test running localreponame with no args from the repo root."""
        self._setup_mock_checkout(mock_find_checkout, "fbsource_custom_name", ".")

        result, output = self._run_and_capture_output(
            self._create_cmd(), self._make_args()
        )

        self.assertEqual(result, 0)
        self.assertEqual(output, "fbsource_custom_name")

    @patch("eden.fs.cli.cmd_util.find_checkout")
    def test_no_args_not_in_repo(
        self,
        mock_find_checkout: MagicMock,
    ) -> None:
        """Test running localreponame with no args outside an Eden repo."""
        mock_find_checkout.return_value = (MagicMock(), None, None)

        result, output = self._run_and_capture_output(
            self._create_cmd(), self._make_args()
        )

        self.assertEqual(result, 1)
        self.assertEqual(output, "")

    @patch("eden.fs.cli.cmd_util.find_checkout")
    def test_with_path_in_repo_subdirectory(
        self,
        mock_find_checkout: MagicMock,
    ) -> None:
        """Test running localreponame with a path to a repo subdirectory."""
        self._setup_mock_checkout(mock_find_checkout, "another_repo", "deep/subdir")

        result, output = self._run_and_capture_output(
            self._create_cmd(), self._make_args("/some/path/another_repo/deep/subdir")
        )

        self.assertEqual(result, 0)
        self.assertEqual(output, "another_repo")

    @patch("eden.fs.cli.cmd_util.find_checkout")
    def test_with_path_in_repo_root(
        self,
        mock_find_checkout: MagicMock,
    ) -> None:
        """Test running localreponame with a path to the repo root."""
        self._setup_mock_checkout(mock_find_checkout, "special_repo_name", ".")

        result, output = self._run_and_capture_output(
            self._create_cmd(), self._make_args("/some/path/special_repo_name")
        )

        self.assertEqual(result, 0)
        self.assertEqual(output, "special_repo_name")

    @patch("eden.fs.cli.cmd_util.find_checkout")
    def test_with_path_not_in_repo(
        self,
        mock_find_checkout: MagicMock,
    ) -> None:
        """Test running localreponame with a path outside an Eden repo."""
        mock_find_checkout.return_value = (MagicMock(), None, None)

        result, output = self._run_and_capture_output(
            self._create_cmd(), self._make_args("/some/random/path")
        )

        self.assertEqual(result, 1)
        self.assertEqual(output, "")
