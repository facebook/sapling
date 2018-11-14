#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from ..lib import hgrepo
from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class GraftTest(EdenHgTestCase):
    commit1: str

    def populate_backing_repo(self, repo):
        repo.write_file("first.txt", "1")
        self.commit1 = repo.commit("Initial commit\n")

    def test_graft_conflict_free_commit(self):
        # Create a new head.
        self.write_file("second.txt", "2")
        self.repo.add_file("second.txt")
        commit2 = self.repo.commit("Second commit\n")

        # Create another new head.
        self.repo.update(self.commit1)
        self.assertFalse(os.path.exists(self.get_path("second.txt")))
        self.write_file("third.txt", "3")
        self.repo.add_file("third.txt")
        commit3 = self.repo.commit("Third commit\n")

        # Perform graft.
        self.hg("graft", commit2)

        # Verify we end up with the expected stack of commits.
        self.assert_status_empty()
        commits = self.repo.log()
        self.assertEqual(3, len(commits))
        self.assertEqual([self.commit1, commit3], commits[:2])
        self.assertTrue(os.path.exists(self.get_path("second.txt")))

    def test_graft_commit_with_conflict(self):
        # Create a new head.
        self.write_file("first.txt", "2")
        commit2 = self.repo.commit("Updated first.txt.\n")

        # Create another new head that modifies first.txt in a different way.
        self.repo.update(self.commit1)
        self.write_file("first.txt", "3")
        commit3 = self.repo.commit("Alternative update to first.txt.\n")

        # Attempt graft.
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("graft", commit2)
        self.assertIn(
            (
                "warning: 1 conflicts while merging first.txt!"
                " (edit, then use 'hg resolve --mark')\n"
                "  abort: unresolved conflicts, can't continue\n"
            ),
            str(context.exception),
        )

        # Resolve conflict with something completely different.
        self.write_file("first.txt", "23")
        self.hg("resolve", "--mark", self.get_path("first.txt"))
        self.hg("graft", "--continue")

        # Verify we end up with the expected stack of commits.
        self.assert_status_empty()
        commits = self.repo.log()
        self.assertEqual(3, len(commits))
        self.assertEqual([self.commit1, commit3], commits[:2])

    def test_graft_that_removes_a_file(self):
        # Create a new head that adds second.txt and removes first.txt.
        self.write_file("second.txt", "2")
        self.repo.add_file("second.txt")
        self.hg("rm", "first.txt")
        commit2 = self.repo.commit("Add second.txt and remove first.txt.")

        # Create a separate head that adds third.txt.
        self.repo.update(self.commit1)
        self.write_file("third.txt", "3")
        self.repo.add_file("third.txt")
        commit3 = self.repo.commit("Add third.txt.")

        # Perform the graft and verify we end up in the right state.
        self.hg("graft", commit2)
        self.assert_status_empty()
        commits = self.repo.log()
        self.assertEqual(3, len(commits))
        self.assertEqual([self.commit1, commit3], commits[:2])
        self.assertFalse(os.path.exists(self.get_path("first.txt")))
        self.assertTrue(os.path.exists(self.get_path("second.txt")))
        self.assertTrue(os.path.exists(self.get_path("third.txt")))
