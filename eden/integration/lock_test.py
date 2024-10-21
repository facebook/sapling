#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import subprocess

from .lib import testcase


@testcase.eden_repo_test
class LockTest(testcase.EdenRepoTest):
    enable_fault_injection: bool = True
    add_file_commit: str = ""
    update_file_commit: str = ""
    remove_file_commit: str = ""

    def populate_repo(self) -> None:
        self.maxDiff = None
        self.repo.write_file("mint", "mint\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        self.repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        self.repo.symlink("slink", "mint")
        self.add_file_commit = self.repo.commit("Adds a new file.")
        self.repo.write_file("mint", "mintoo\n")
        self.update_file_commit = self.repo.commit("Updates the file.")
        self.repo.remove_files([self.repo.get_path("mint")])
        self.remove_file_commit = self.repo.commit("Remove newly added file.")
        self.repo.update(self.update_file_commit)

    def check_read_allowed(self, expected_text: str) -> None:
        # Checks that external programs are allowed to read the file
        with open(self.repo.get_path("mint"), "r") as f:
            self.assertEqual(f.read(), expected_text)

    def check_read_blocked(self) -> None:
        # Checks that external programs are not allowed to read the file
        with self.assertRaises("PermissionError", msg="Reading should be blocked"):
            with open(self.repo.get_path("mint"), "r") as f:  # noqa: F841
                f.read()

    def check_commit_edit_allowed(self) -> None:
        # Checks that external programs are allowed to
        # change the file
        self.repo.update(self.add_file_commit)
        with open(self.repo.get_path("mint"), "r") as f:
            self.assertEqual(f.read(), "mint\n")

    def check_commit_edit_blocked(self, errmsg=b"abort:") -> None:
        # Checks that external programs are not allowed to change the file
        # A process needs both read and write access to a file to edit it.
        with self.assertRaisesRegex(
            subprocess.CalledProcessError,
            errmsg + ".*",
            "Editing via changing commits should be blocked",
        ):
            self.repo.update(self.add_file_commit)

    def check_commit_remove_allowed(self) -> None:
        # Checks that external programs are allowed to remove the file
        self.repo.update(self.remove_file_commit)
        self.assertFalse(os.path.isfile(self.repo.get_path("mint")))

    def check_commit_remove_blocked(self, errmsg=b"abort:") -> None:
        # Checks that external programs are not allowed to remove the file
        # A process needs both delete and write access to a file to remove it.
        with self.assertRaisesRegex(
            subprocess.CalledProcessError,
            errmsg + ".*",
            "Removing via changing commits should be blocked",
        ):
            self.repo.update(self.remove_file_commit)

    def test_no_lock(self) -> None:
        self.check_read_allowed("mintoo\n")
        self.check_commit_edit_allowed()
        self.check_commit_remove_allowed()
        self.repo.update(self.update_file_commit)
        self.check_read_allowed("mintoo\n")
        self.repo.update(self.add_file_commit)
        self.check_read_allowed("mint\n")
