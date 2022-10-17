#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from .lib import testcase


@testcase.eden_repo_test(case_sensitivity_dependent=True)
class CasingTest(testcase.EdenRepoTest):
    """Verify that EdenFS behave properly when configured to be case
    insensitive and case preserving.
    """

    def populate_repo(self) -> None:
        self.repo.write_file("adir1/adir2/a", "Hello!\n")
        self.repo.commit("a")

    def test_insensitive(self) -> None:
        if not self.is_case_sensitive:
            self.assertEqual(self.read_file("adir1/adir2/A"), "Hello!\n")

    def test_case_preserving(self) -> None:
        if not self.is_case_sensitive:
            self.assertEqual(self.read_file("adir1/adir2/A"), "Hello!\n")
            self.assertEqual(os.listdir(self.get_path("adir1/adir2")), ["a"])

    def test_case_preserving_new_files(self) -> None:
        if not self.is_case_sensitive:
            self.write_file("MixedCaseFile", "content\n")
            self.assertIn("MixedCaseFile", os.listdir(self.get_path("")))
