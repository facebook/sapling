#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class FoldTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("letters", "a\nb\nc\n")
        repo.write_file("numbers", "1\n2\n3\n")
        repo.commit("First commit.")

        repo.write_file("numbers", "4\n5\n6\n")
        repo.commit("Second commit.")

    def test_fold_two_commits_into_one(self) -> None:
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
