#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import subprocess

from .lib import testcase


GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000

FILE_SHARE_DELETE = 0x00000004
FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002


@testcase.eden_repo_test
class LockTest(testcase.EdenRepoTest):
    enable_fault_injection: bool = True
    add_file_commit: str = ""
    update_file_commit: str = ""
    remove_file_commit: str = ""
    BASE_FILE_NAME = "mint"
    BASE_FILE_CONTENTS = "mint\n"
    UPDATED_FILE_CONTENTS = "mintoo\n"

    def populate_repo(self) -> None:
        self.maxDiff = None
        self.repo.write_file(self.BASE_FILE_NAME, self.BASE_FILE_CONTENTS)
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        self.repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        self.repo.symlink("slink", self.BASE_FILE_NAME)
        self.add_file_commit = self.repo.commit("Adds a new file.")
        self.repo.write_file(self.BASE_FILE_NAME, self.UPDATED_FILE_CONTENTS)
        self.update_file_commit = self.repo.commit("Updates the file.")
        self.repo.remove_files([self.repo.get_path(self.BASE_FILE_NAME)])
        self.remove_file_commit = self.repo.commit("Remove newly added file.")
        self.repo.update(self.update_file_commit)

    def check_read_allowed(self, expected_text: str) -> None:
        # Checks that external programs are allowed to read the file
        with open(self.eden_repo.get_path(self.BASE_FILE_NAME), "r") as f:
            self.assertEqual(f.read(), expected_text)

    def check_read_blocked(self) -> None:
        # Checks that external programs are not allowed to read the file
        with self.assertRaises(PermissionError, msg="Reading should be blocked"):
            with open(self.eden_repo.get_path(self.BASE_FILE_NAME), "r") as f:  # noqa: F841
                f.read()

    def check_commit_edit_allowed(self) -> None:
        # Checks that external programs are allowed to
        # change the file
        self.eden_repo.update(self.add_file_commit)
        with open(self.eden_repo.get_path(self.BASE_FILE_NAME), "r") as f:
            self.assertEqual(f.read(), self.BASE_FILE_CONTENTS)

    def check_commit_edit_blocked(self, errmsg=".*abort:.*") -> None:
        # Checks that external programs are not allowed to change the file
        # A process needs both read and write access to a file to edit it.
        with self.assertRaisesRegex(
            subprocess.CalledProcessError,
            errmsg,
            msg="Editing via changing commits should be blocked",
        ):
            self.eden_repo.update(self.add_file_commit)

    def check_commit_remove_allowed(self) -> None:
        # Checks that external programs are allowed to remove the file
        self.eden_repo.update(self.remove_file_commit)
        self.assertFalse(os.path.isfile(self.eden_repo.get_path(self.BASE_FILE_NAME)))

    def check_commit_remove_blocked(self, errmsg=".*abort:.*") -> None:
        # Checks that external programs are not allowed to remove the file
        # A process needs both delete and write access to a file to remove it.
        with self.assertRaisesRegex(
            subprocess.CalledProcessError,
            errmsg,
            msg="Removing via changing commits should be blocked",
        ):
            self.eden_repo.update(self.remove_file_commit)

    def test_no_lock(self) -> None:
        self.check_read_allowed(self.UPDATED_FILE_CONTENTS)
        self.check_commit_edit_allowed()
        self.check_commit_remove_allowed()
        self.eden_repo.update(self.update_file_commit)
        self.check_read_allowed(self.UPDATED_FILE_CONTENTS)
        self.eden_repo.update(self.add_file_commit)
        self.check_read_allowed(self.BASE_FILE_CONTENTS)
