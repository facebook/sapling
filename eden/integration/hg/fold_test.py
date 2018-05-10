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
class FoldTest(EdenHgTestCase):

    def populate_backing_repo(self, repo):
        repo.write_file("letters", "a\nb\nc\n")
        repo.write_file("numbers", "1\n2\n3\n")
        repo.commit("First commit.")

        repo.write_file("numbers", "4\n5\n6\n")
        repo.commit("Second commit.")

    def test_fold_two_commits_into_one(self):
        commits = self.repo.log(template="{desc}")
        self.assertEqual(["First commit.", "Second commit."], commits)
        files = self.repo.log(template="{files}")
        self.assertEqual(["letters numbers", "numbers"], files)

        editor = self.create_editor_that_writes_commit_messages(["Combined commit."])

        self.hg(
            "fold",
            "--config",
            "ui.interactive=true",
            "--config",
            "ui.interface=text",
            "--from",
            ".^",
            hgeditor=editor,
        )

        self.assert_status_empty()
        commits = self.repo.log(template="{desc}")
        self.assertEqual(["Combined commit."], commits)
        files = self.repo.log(template="{files}")
        self.assertEqual(["letters numbers"], files)
