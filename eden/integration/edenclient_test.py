#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import pathlib

from .lib import testcase


@testcase.eden_repo_test
class EdenClientTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_client_dir_for_mount(self) -> None:
        clone_path = pathlib.Path(self.tmp_dir, "test_checkout")
        self.eden.run_cmd("clone", self.repo_name, str(clone_path))
        self.assertEqual(
            self.eden.client_dir_for_mount(clone_path),
            pathlib.Path(self.eden_dir, "clients", "test_checkout"),
        )
