#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import pathlib

from .lib import testcase


@testcase.eden_repo_test
class EdenClientTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_client_dir_for_mount(self) -> None:
        clone_path = pathlib.Path(self.tmp_dir, "test_checkout")
        self.eden.run_cmd("clone", self.repo_name, clone_path)
        self.assertEqual(
            self.eden.client_dir_for_mount(clone_path),
            pathlib.Path(self.eden_dir, "clients", "test_checkout"),
        )
