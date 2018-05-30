#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, JournalEntry, hg_test


@hg_test
class JournalTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        repo.write_file("foo/bar.txt", "test\n")
        repo.write_file("foo/subdir/test.txt", "test\n")
        self.commit1 = repo.commit("Initial commit.")

    def test_journal(self) -> None:
        self.assert_journal_empty()

        # Create a new commit
        self.assert_status_empty()
        self.write_file("foo/bar.txt", "test version 2\n")
        self.assert_status({"foo/bar.txt": "M"})
        commit2 = self.repo.commit("Updated bar.txt\n")

        # Check that the journal was updated
        self.assert_journal(
            JournalEntry(name=".", old=self.commit1, new=commit2, command="^commit")
        )

        # Amend the commit
        self.write_file("foo/bar.txt", "v3\nother stuff\n")
        commit3 = self.repo.commit("Updated bar.txt\n", amend=True)

        # Check out commit1, then commit3 again
        self.repo.update(self.commit1)
        self.repo.update(commit3)

        # Check the journal
        self.assert_journal(
            JournalEntry(name=".", old=self.commit1, new=commit2, command="^commit"),
            JournalEntry(
                name=".", old=commit2, new=commit3, command="^commit.* --amend"
            ),
            JournalEntry(name=".", old=commit3, new=self.commit1, command="^update"),
            JournalEntry(name=".", old=self.commit1, new=commit3, command="^update"),
        )
