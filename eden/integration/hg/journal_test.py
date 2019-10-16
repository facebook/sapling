#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, JournalEntry, hg_test


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class JournalTest(EdenHgTestCase):
    commit1: str

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
