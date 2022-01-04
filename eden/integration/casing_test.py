#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import sys

from .lib import testcase


@testcase.eden_repo_test
# pyre-ignore[13]: T62487924
class CasingTest(testcase.EdenRepoTest):
    """Verify that EdenFS behave properly when configured to be case
    insensitive and case preserving.
    """

    is_case_insensitive: bool

    def populate_repo(self) -> None:
        self.is_case_insensitive = sys.platform == "win32"

        self.repo.write_file("adir1/adir2/a", "Hello!\n")
        self.repo.commit("a")

    def test_insensitive(self) -> None:
        if self.is_case_insensitive:
            self.assertEqual(self.read_file("adir1/adir2/A"), "Hello!\n")

    def test_case_preserving(self) -> None:
        if self.is_case_insensitive:
            self.assertEqual(self.read_file("adir1/adir2/A"), "Hello!\n")
            self.assertEqual(os.listdir(self.get_path("adir1/adir2")), ["a"])

    def test_case_preserving_new_files(self) -> None:
        if self.is_case_insensitive:
            self.write_file("MixedCaseFile", "content\n")
            self.assertIn("MixedCaseFile", os.listdir(self.get_path("")))
