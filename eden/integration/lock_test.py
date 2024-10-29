#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import subprocess
import sys

from .lib import testcase

if sys.platform == "win32":
    try:
        from .lib.ntapi import open_file_handle
    except ImportError:
        # TODO(T150221518): We should add the ntapi extension module to the
        # getdeps build, but for now we have to account for the possibility that
        # it may not be present.
        pass

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
        with open(self.repo.get_path(self.BASE_FILE_NAME), "r") as f:
            self.assertEqual(f.read(), expected_text)

    def check_read_blocked(self) -> None:
        # Checks that external programs are not allowed to read the file
        with self.assertRaises(PermissionError, msg="Reading should be blocked"):
            with open(self.repo.get_path(self.BASE_FILE_NAME), "r") as f:  # noqa: F841
                f.read()

    def check_commit_edit_allowed(self) -> None:
        # Checks that external programs are allowed to
        # change the file
        self.repo.update(self.add_file_commit)
        with open(self.repo.get_path(self.BASE_FILE_NAME), "r") as f:
            self.assertEqual(f.read(), self.BASE_FILE_CONTENTS)

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
        self.assertFalse(os.path.isfile(self.repo.get_path(self.BASE_FILE_NAME)))

    def check_commit_remove_blocked(self, errmsg=b"abort:") -> None:
        # Checks that external programs are not allowed to remove the file
        # A process needs both delete and write access to a file to remove it.
        with self.assertRaisesRegex(
            subprocess.CalledProcessError,
            errmsg + ".*",
            "Removing via changing commits should be blocked",
        ):
            self.repo.update(self.remove_file_commit)

    def _test_share_none(self, open_mode) -> None:
        # Need to hold open the file until the end of the test
        handle = open_file_handle(  # noqa: F841
            self.repo.get_path(self.BASE_FILE_NAME),
            open_mode,
            0,
        )

        self.check_read_blocked()
        self.check_commit_edit_blocked()
        self.check_commit_remove_blocked()

        # Handle is closed when it is deleted
        del handle

    def _test_share_read(self, open_mode) -> None:
        # Need to hold open the file until the end of the test
        handle = open_file_handle(  # noqa: F841
            self.repo.get_path(self.BASE_FILE_NAME),
            open_mode,
            FILE_SHARE_READ,
        )

        # Check that reading is allowed
        self.check_read_allowed(self.UPDATED_FILE_CONTENTS)
        self.check_commit_edit_blocked(b"abort: error writing files")

        # BUG ZONE
        # TODO: This should be blocked, but it isn't
        try:
            self.check_commit_remove_blocked(b"abort: error writing files")
        except RuntimeError as e:
            self.assertEqual(
                e.args[0], "Removing via changing commits should be blocked"
            )

        # BUG1 See that the commit has changed
        cmdoutput = self.repo.run_hg("sl")
        sl = cmdoutput.stdout.decode("utf-8").split("\n")
        self.assertEqual(sl[0], f"@  commit:      {self.remove_file_commit[:12]}")

        # BUG1 No changes to the file are reported
        cmdoutput = self.repo.run_hg("diff")
        sl = cmdoutput.stdout.decode("utf-8")
        self.assertEqual(sl, "")
        cmdoutput = self.repo.run_hg("status")
        sl = cmdoutput.stdout.decode("utf-8")
        self.assertEqual(sl, "")

        # BUG1 See that the file still exists
        self.assertTrue(os.path.isfile(self.repo.get_path(self.BASE_FILE_NAME)))

        # Handle is closed when it is deleted
        del handle

        # BUG1 See that file contents have not changed
        with open(self.repo.get_path(self.BASE_FILE_NAME), "r") as f:
            self.assertEqual(f.read(), self.UPDATED_FILE_CONTENTS)

    def _test_share_write(self, open_mode) -> None:
        # Need to hold open the file until the end of the test
        handle = open_file_handle(  # noqa: F841
            self.repo.get_path(self.BASE_FILE_NAME),
            open_mode,
            FILE_SHARE_WRITE,
        )

        self.check_read_blocked()
        self.check_commit_edit_blocked(
            b"abort: The process cannot access the file because it is being used by another process."
        )
        self.check_commit_remove_blocked(
            b"abort: The process cannot access the file because it is being used by another process."
        )

        # Handle is closed when it is deleted
        del handle

    def _test_share_delete(self, open_mode) -> None:
        # Need to hold open the file until the end of the test
        handle = open_file_handle(  # noqa: F841
            self.repo.get_path(self.BASE_FILE_NAME),
            open_mode,
            FILE_SHARE_DELETE,
        )

        self.check_read_blocked()
        self.check_commit_edit_blocked(
            b"abort: The process cannot access the file because it is being used by another process."
        )
        self.check_commit_remove_blocked(
            b"abort: The process cannot access the file because it is being used by another process."
        )

        # Handle is closed when it is deleted
        del handle

    def _test_share_read_write(self, open_mode) -> None:
        # Need to hold open the file until the end of the test
        handle = open_file_handle(  # noqa: F841
            self.repo.get_path(self.BASE_FILE_NAME),
            open_mode,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )

        self.check_read_allowed(self.UPDATED_FILE_CONTENTS)
        self.check_commit_edit_allowed()
        # Same bug as in _test_share_read
        try:
            self.check_commit_remove_blocked(
                b"abort: The process cannot access the file because it is being used by another process."
            )
        except RuntimeError:
            pass
        cmdoutput = self.repo.run_hg("sl")
        sl = cmdoutput.stdout.decode("utf-8").split("\n")
        self.assertEqual(sl[0], f"@  commit:      {self.remove_file_commit[:12]}")

        # BUG1 See that the file still exists
        self.assertTrue(os.path.isfile(self.repo.get_path(self.BASE_FILE_NAME)))

        # Handle is closed when it is deleted
        del handle

        # BUG1 See that file contents have not changed
        with open(self.repo.get_path(self.BASE_FILE_NAME), "r") as f:
            self.assertEqual(f.read(), self.BASE_FILE_CONTENTS)

    def _test_share_read_delete(self, open_mode) -> None:
        # Need to hold open the file until the end of the test
        handle = open_file_handle(  # noqa: F841
            self.repo.get_path(self.BASE_FILE_NAME),
            open_mode,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
        )

        self.check_read_allowed(self.UPDATED_FILE_CONTENTS)

        # BUG ZONE
        # I don't know if this is supposed to work or not
        # checkout passes and file is changed to the correct value
        # despite not having write access
        try:
            self.check_commit_edit_blocked(
                b"abort: The process cannot access the file because it is being used by another process."
            )
        except RuntimeError:
            pass
        cmdoutput = self.repo.run_hg("sl")
        sl = cmdoutput.stdout.decode("utf-8").split("\n")
        self.assertEqual(sl[10], f"@  commit:      {self.add_file_commit[:12]}")

        with open(self.repo.get_path(self.BASE_FILE_NAME), "r") as f:
            self.assertEqual(f.read(), self.BASE_FILE_CONTENTS)
        # END BUG ZONE

        self.check_commit_remove_allowed()
        cmdoutput = self.repo.run_hg("sl")
        sl = cmdoutput.stdout.decode("utf-8").split("\n")
        self.assertEqual(sl[0], f"@  commit:      {self.remove_file_commit[:12]}")
        self.assertFalse(os.path.isfile(self.repo.get_path(self.BASE_FILE_NAME)))

        # Handle is closed when it is deleted
        del handle

    def _test_share_write_delete(self, open_mode) -> None:
        # Need to hold open the file until the end of the test
        handle = open_file_handle(  # noqa: F841
            self.repo.get_path(self.BASE_FILE_NAME),
            open_mode,
            FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )

        self.check_read_blocked()
        self.check_commit_edit_blocked(
            b"abort: The process cannot access the file because it is being used by another process."
        )
        self.check_commit_remove_blocked(
            b"abort: The process cannot access the file because it is being used by another process."
        )

        # Handle is closed when it is deleted
        del handle

    def _test_share_read_write_delete(self, open_mode) -> None:
        # Need to hold open the file until the end of the test
        handle = open_file_handle(  # noqa: F841
            self.repo.get_path(self.BASE_FILE_NAME),
            open_mode,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )

        self.check_read_allowed(self.UPDATED_FILE_CONTENTS)
        self.check_commit_edit_allowed()
        self.check_commit_remove_allowed()

        # Handle is closed when it is deleted
        del handle

    def test_no_lock(self) -> None:
        self.check_read_allowed(self.UPDATED_FILE_CONTENTS)
        self.check_commit_edit_allowed()
        self.check_commit_remove_allowed()
        self.repo.update(self.update_file_commit)
        self.check_read_allowed(self.UPDATED_FILE_CONTENTS)
        self.repo.update(self.add_file_commit)
        self.check_read_allowed(self.BASE_FILE_CONTENTS)
