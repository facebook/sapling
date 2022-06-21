# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.testlib.base import BaseTest, hgtest
from eden.testlib.repo import Repo
from eden.testlib.workingcopy import WorkingCopy


class TestDiffHashes(BaseTest):
    @hgtest
    def test_diff_hashes(self, repo: Repo, wc: WorkingCopy) -> None:

        self.assertEqual(
            repo.hg.diff("inexistent1", "inexistent2").stderr,
            "inexistent1: No such file or directory\ninexistent2: No such file or directory\n",
        )

        file = wc.file()

        commit1 = wc.commit()

        file.write("foobar")

        commit2 = wc.commit()

        # Verify commit hashes are not present when --quiet is used.
        self.assertNotIn(
            f"diff -r {commit1.hash[:12]} -r {commit2.hash[:12]} {file.path}",
            wc.hg.diff(
                quiet=True, rev=[commit1.hash, commit2.hash], no_git=True
            ).stdout,
        )
        self.assertNotIn(
            f"diff -r {commit1.hash} -r {commit2.hash} {file.path}",
            wc.hg.diff(
                quiet=True, rev=[commit1.hash, commit2.hash], no_git=True
            ).stdout,
        )

        # Verify that hashes are present when no flag is passed, and when --verbose is used
        self.assertIn(
            f"diff -r {commit1.hash[:12]} -r {commit2.hash[:12]} {file.path}",
            repo.hg.diff(rev=[commit1.hash, commit2.hash], no_git=True).stdout,
        )
        self.assertIn(
            f"diff -r {commit1.hash[:12]} -r {commit2.hash[:12]} {file.path}",
            repo.hg.diff(
                verbose=True, rev=[commit1.hash, commit2.hash], no_git=True
            ).stdout,
        )

        # Verify that full hashes are present when --debug is used
        self.assertIn(
            f"diff -r {commit1.hash} -r {commit2.hash} {file.path}",
            repo.hg.diff(
                debug=True, rev=[commit1.hash, commit2.hash], no_git=True
            ).stdout,
        )
