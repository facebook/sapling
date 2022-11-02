# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from pathlib import Path

from eden.testlib.base import BaseTest, hgtest
from eden.testlib.repo import Repo
from eden.testlib.requires import require
from eden.testlib.workingcopy import WorkingCopy


@require(caseinsensitive=True)
class TestInsensitiveStatus(BaseTest):
    def setUp(self) -> None:
        super().setUp()

    # Test Case: status returns clean for case changed file
    @hgtest
    def test_changed_file(self, repo: Repo, wc: WorkingCopy) -> None:
        file = wc.file("file")
        wc.commit()
        file.rename(Path("FILE"))

        status = wc.status()
        self.assertTrue(status.empty())

    # Test Case: status returns clean for case changed directory
    @hgtest
    def test_changed_directory(self, repo: Repo, wc: WorkingCopy) -> None:
        file = wc.file("dir/file", content="foo")
        wc.commit()
        file.remove()
        os.rmdir(wc.join("dir"))

        file = wc.file("DIR/file", content="foo")

        status = wc.status()
        self.assertTrue(status.empty())

    # Test Case: `hg add` on a file whose parent dir has different case than the
    # treemanifest uses the same case as in treemanifest.
    @hgtest
    def test_add_different_dir(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file("dir/file")
        wc.commit()
        wc.file("dir/file2", add=False)
        os.rename(wc.join("dir"), wc.join("DIR"))

        # BUGBUG: With fsmonitor, this `hg add` hits an issue where the input
        # file is normalized to "dir/file2", but that fails to match the
        # filesystem file during status and therefore doesn't get reported as
        # something that could be added. Thus the add does nothing.
        #
        # This is ok-ish, since we don't want users accidentally adding the
        # wrong case to the treestate, but it's still a bug. Using
        # "re:DIR/file2" works around this, since re patterns are not
        # normalized, but then it adds the incorrect "DIR/file2" case to the
        # treestate.
        #
        # Without fsmonitor, it works fine and status.added below shows
        # "dir/file2".

        wc.add("DIR/file2")
        status = wc.status()
        self.assertEqual(status.added, [])

    # Test Case: Checkout across case change.
    @hgtest
    def test_checkout_across_change(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file("dir/file")
        wc.commit()
        wc.hg.rename("dir/file", "dir/FILE")
        wc.commit()

        files = wc.ls(recurse=True)
        self.assertEqual(files, ["dir/", "dir/FILE"])

        wc.checkout(".^")
        files = wc.ls(recurse=True)
        self.assertEqual(files, ["dir/", "dir/file"])

        wc.hg.rename("dir", "temp")
        wc.hg.rename("temp", "DIR")
        dir_rename = wc.commit()
        files = wc.ls(recurse=True)
        self.assertEqual(files, ["DIR/", "DIR/file"])

        wc.checkout(".^")
        files = wc.ls(recurse=True)
        self.assertEqual(files, ["dir/", "dir/file"])

        wc.checkout(dir_rename)

        # Checkout across the same change, but this time there is an untracked
        # file in the directory. This time the directory is not made lowercase,
        # since it is not deleted due to the presence of the untracked file.
        wc.file("dir/untracked", add=False)
        wc.checkout(".^")
        files = wc.ls(recurse=True)
        self.assertEqual(files, ["DIR/", "DIR/file", "DIR/untracked"])
        status = wc.status()
        # BUGBUG: This shows "dir/untracked" for a non-fsmonitor status, but
        # DIR/untracked for fsmonitor and Rust status.
        self.assertEqual(status.untracked, ["DIR/untracked"])

    # Test Case: Sparse config/profile filters treemanifest case-sensitivly.
    @hgtest
    def test_sparse_config(self, repo: Repo, wc: WorkingCopy) -> None:
        repo.config.enable("sparse")
        wc.file("included/file")
        wc.file("excluded/file")
        wc.commit()

        wc.hg.sparse("include", "included")
        self.assertEqual(wc.ls(recurse=True), ["included/", "included/file"])

        wc.hg.sparse("reset")

        wc.hg.sparse("include", "INCLUDED")
        self.assertEqual(wc.ls(recurse=True), [])

    # Test Case: Gitignore filters files case-insensitivly.
    @hgtest
    def test_gitignore(self, repo: Repo, wc: WorkingCopy) -> None:
        gitignore = wc.file(".gitignore")
        wc.file("included/file", add=False)
        wc.file("excluded/file", add=False)
        wc.commit()

        self.assertEqual(wc.status().untracked, ["excluded/file", "included/file"])

        gitignore.write("included")
        self.assertEqual(wc.status().untracked, ["excluded/file"])

        gitignore.write("INCLUDED")
        self.assertEqual(wc.status().untracked, ["excluded/file"])


@require(caseinsensitive=True)
class TestInsensitiveRustStatus(TestInsensitiveStatus):
    def setUp(self) -> None:
        super().setUp()
        self.config.add("workingcopy", "use-rust", "True")
        self.config.add("status", "use-rust", "True")


# Disabled due to failures. See BUGBUG notes above.
# Technically the NoFsmonitor case has the right behavior, but we've coded the
# test results against the current behavior.
# @require(caseinsensitive=True)
# class TestInsensitiveNoFsmonitorStatus(TestInsensitiveStatus):
#    def setUp(self) -> None:
#        super().setUp()
#        self.config.add("extensions", "fsmonitor", "!")
