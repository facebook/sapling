#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class CommitTest(EdenHgTestCase):
    commit1: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        repo.write_file("foo/bar.txt", "test\n")
        repo.write_file("foo/subdir/test.txt", "test\n")
        self.commit1 = repo.commit("Initial commit.\n")

    def test_commit_modification(self) -> None:
        """Test committing a modification to an existing file"""
        self.assert_status_empty()

        self.write_file("foo/bar.txt", "test version 2\n")
        self.assert_status({"foo/bar.txt": "M"})

        commit2 = self.repo.commit("Updated bar.txt\n")
        self.assertNotEqual(self.commit1, commit2)
        self.assert_status_empty()
        self.assertEqual("test version 2\n", self.read_file("foo/bar.txt"))
        self.assertEqual([self.commit1, commit2], self.repo.log())

    def test_commit_new_file(self) -> None:
        """Test committing a new file"""
        self.assert_status_empty()

        self.write_file("foo/new.txt", "new and improved\n")
        self.assert_status({"foo/new.txt": "?"})
        self.hg("add", "foo/new.txt")
        self.assert_status({"foo/new.txt": "A"})

        commit2 = self.repo.commit("Added new.txt\n")
        self.assertNotEqual(self.commit1, commit2)
        self.assert_status_empty()
        self.assertEqual("new and improved\n", self.read_file("foo/new.txt"))

    def test_commit_remove_file(self) -> None:
        """Test a commit that removes a file"""
        self.assert_status_empty()

        self.hg("rm", "foo/subdir/test.txt")
        self.assertFalse(os.path.exists(self.get_path("foo/subdir/test.txt")))
        self.assert_status({"foo/subdir/test.txt": "R"})

        commit2 = self.repo.commit("Removed test.txt\n")
        self.assertNotEqual(self.commit1, commit2)
        self.assert_status_empty()
        self.assertFalse(os.path.exists(self.get_path("foo/subdir/test.txt")))

    def test_amend(self) -> None:
        """Test amending a commit"""
        self.assert_status_empty()

        self.write_file("foo/bar.txt", "test version 2\n")
        self.write_file("foo/new.txt", "new and improved\n")
        self.hg("add", "foo/new.txt")
        self.assert_status({"foo/bar.txt": "M", "foo/new.txt": "A"})

        commit2 = self.repo.commit("Updated initial commit\n", amend=True)
        self.assertNotEqual(self.commit1, commit2)
        self.assert_status_empty()
        self.assertEqual("test version 2\n", self.read_file("foo/bar.txt"))
        self.assertEqual("new and improved\n", self.read_file("foo/new.txt"))
        self.assertEqual([commit2], self.repo.log())

    def test_commit_interactive_with_new_file(self) -> None:
        self.assert_status_empty()
        self.assert_dirstate_empty()

        self.write_file("foo/bar.txt", "test v2\n")
        self.write_file("foo/new.txt", "new and improved\n")
        self.hg("add", "foo/new.txt")
        self.write_file("hello.txt", "ohai")
        self.assert_status({"foo/bar.txt": "M", "foo/new.txt": "A", "hello.txt": "M"})

        commit_commands = (
            "y\n"  # examine foo/bar.txt
            "y\n"  # record changes from foo/bar.txt
            "y\n"  # examine foo/new.txt
            "y\n"  # record changes from foo/new.txt
            "n\n"  # examine hello.txt
        )
        self.repo.run_hg(
            "commit",
            "-i",
            "-m",
            "test commit with a new file",
            "--config",
            "ui.interactive=true",
            "--config",
            "ui.interface=text",
            input=commit_commands,
            stdout=None,
            stderr=None,
        )

        self.assert_status({"hello.txt": "M"})
        self.assert_dirstate_empty()

        self.hg("commit", "-m", "commit changes to hello.txt")
        self.assert_status_empty()
        self.assert_dirstate_empty()

    def test_commit_single_new_file(self) -> None:
        self.assert_status_empty()
        self.assert_dirstate_empty()

        self.write_file("hello.txt", "updated")
        self.write_file("foo/new.txt", "new and improved\n")
        self.hg("add", "foo/new.txt")
        self.assert_status({"foo/new.txt": "A", "hello.txt": "M"})

        self.repo.run_hg(
            "commit", "-m", "test", "foo/new.txt", stdout=None, stderr=None
        )
        self.assert_status({"hello.txt": "M"})
        self.assert_dirstate_empty()

    def test_commit_single_modified_file(self) -> None:
        self.assert_status_empty()
        self.assert_dirstate_empty()

        self.write_file("hello.txt", "updated")
        self.write_file("foo/bar.txt", "bar updated\n")
        self.assert_status({"foo/bar.txt": "M", "hello.txt": "M"})
        self.assert_dirstate_empty()

        self.repo.run_hg(
            "commit", "-m", "test", "foo/bar.txt", stdout=None, stderr=None
        )
        self.assert_status({"hello.txt": "M"})
        self.assert_dirstate_empty()

    def test_commit_subdirectory(self) -> None:
        self.assert_status_empty()
        self.assert_dirstate_empty()

        self.write_file("hello.txt", "updated")
        self.write_file("foo/new.txt", "new and improved\n")
        self.write_file("foo/bar.txt", "bar updated\n")
        self.hg("add", "foo/new.txt")
        self.assert_status({"foo/new.txt": "A", "foo/bar.txt": "M", "hello.txt": "M"})

        self.repo.run_hg("commit", "-m", "test", "foo", stdout=None, stderr=None)
        self.assert_status({"hello.txt": "M"})
        self.assert_dirstate_empty()
