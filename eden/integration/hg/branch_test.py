#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class BranchTest(EdenHgTestCase):
    def populate_backing_repo(self, repo):
        repo.write_file("a_file.txt", "")
        repo.commit("first commit")

    def test_set_branch(self):
        original_branch = self.hg("branch")
        self.assertEqual("default", original_branch.rstrip())

        self.hg("branch", "foo-bar")
        new_branch = self.hg("branch")
        self.assertEqual("foo-bar", new_branch.rstrip())
