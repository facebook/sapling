#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class RevertTest(EdenHgTestCase):

    def populate_backing_repo(self, repo):
        repo.write_file("hello.txt", "hola")
        repo.commit("Initial commit.\n")

    def test_make_local_change_and_attempt_revert(self):
        self.write_file("hello.txt", "hello")
        self.assert_status({"hello.txt": "M"})
        self.hg("revert", "--no-backup", "hello.txt")
        self.assert_status_empty()
        txt_contents = self.read_file("hello.txt")
        self.assertEqual("hola", txt_contents)
