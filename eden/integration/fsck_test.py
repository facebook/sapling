#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import pathlib
import subprocess
import unittest
from pathlib import Path
from typing import Tuple

from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from .lib import edenclient, overlay as overlay_mod, repobase, testcase


FSCK_RETCODE_OK = 0
FSCK_RETCODE_SKIPPED = 1
FSCK_RETCODE_WARNINGS = 2
FSCK_RETCODE_ERRORS = 3


class FsckTest(testcase.EdenRepoTest):
    overlay: overlay_mod.OverlayStore

    def populate_repo(self) -> None:
        self.repo.write_file("README.md", "tbd\n")
        self.repo.write_file("proj/src/main.c", "int main() { return 0; }\n")
        self.repo.write_file("proj/src/lib.c", "void foo() {}\n")
        self.repo.write_file("proj/src/include/lib.h", "#pragma once\nvoid foo();\n")
        self.repo.write_file(
            "proj/test/test.sh", "#!/bin/bash\necho test\n", mode=0o755
        )
        self.repo.write_file("doc/foo.txt", "foo\n")
        self.repo.write_file("doc/bar.txt", "bar\n")
        self.repo.symlink("proj/doc", "../doc")
        self.repo.commit("Initial commit.")

    def create_repo(self, name: str) -> repobase.Repository:
        return self.create_hg_repo("main")

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        self.overlay = overlay_mod.OverlayStore(self.eden, self.mount_path)

    def run_fsck(self, *args: str) -> Tuple[int, str]:
        """Run `eden fsck [args]` and return a tuple of the return code and
        the combined stdout and stderr.

        The command output will be decoded as UTF-8 and returned as a string.
        """
        cmd_result = self.eden.run_unchecked(
            "fsck", *args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
        )
        fsck_out = cmd_result.stdout.decode("utf-8", errors="replace")
        return (cmd_result.returncode, fsck_out)

    def test_fsck_force_and_check_only(self) -> None:
        """Test the behavior of the --force and --check-only fsck flags."""
        foo_overlay_path = self.overlay.materialize_file(pathlib.Path("doc/foo.txt"))

        # Running fsck with the mount still mounted should fail
        returncode, fsck_out = self.run_fsck(self.mount)
        self.assertIn(f"Not checking {self.mount}", fsck_out)
        self.assertEqual(FSCK_RETCODE_SKIPPED, returncode)

        # Running fsck with --force should override that
        returncode, fsck_out = self.run_fsck(self.mount, "--force")
        self.assertIn(f"warning: could not obtain lock", fsck_out)
        self.assertIn(f"scanning anyway due to --force", fsck_out)
        self.assertIn(f"Checking {self.mount}", fsck_out)
        self.assertEqual(FSCK_RETCODE_OK, returncode)

        # fsck should perform the check normally without --force
        # if the mount is not mounted
        self.eden.run_cmd("unmount", self.mount)
        returncode, fsck_out = self.run_fsck(self.mount)
        self.assertIn(f"Checking {self.mount}", fsck_out)
        self.assertIn("No issues found", fsck_out)
        self.assertEqual(FSCK_RETCODE_OK, returncode)

        # Truncate the overlay file for doc/foo.txt to 0 length
        with foo_overlay_path.open("wb"):
            pass

        # Running fsck with --check-only should report the error but not try to fix it.
        returncode, fsck_out = self.run_fsck("--check-only")
        self.assertIn(f"Checking {self.mount}", fsck_out)
        self.assertRegex(
            fsck_out,
            r"invalid overlay file for materialized file .* \(doc/foo.txt\).*: "
            r"zero-sized overlay file",
        )
        self.assertRegex(fsck_out, r"\b1 errors")
        self.assertRegex(fsck_out, "Not fixing errors: --check-only was specified")
        self.assertEqual(FSCK_RETCODE_ERRORS, returncode)

        # Running fsck with no arguments should attempt to fix the errors
        returncode, fsck_out = self.run_fsck()
        self.assertRegex(
            fsck_out,
            r"invalid overlay file for materialized file .* \(doc/foo.txt\).*: "
            r"zero-sized overlay file",
        )
        self.assertRegex(fsck_out, r"\b1 errors")
        self.assertRegex(fsck_out, "Beginning repairs")
        self.assertRegex(
            fsck_out, "replacing corrupt file inode 'doc/foo.txt' with an empty file"
        )
        self.assertRegex(fsck_out, "Fixed 1 of 1 issues")
        self.assertEqual(FSCK_RETCODE_ERRORS, returncode)

        # There should be no more errors if we run fsck again
        returncode, fsck_out = self.run_fsck()
        self.assertIn(f"Checking {self.mount}", fsck_out)
        self.assertIn("No issues found", fsck_out)
        self.assertEqual(FSCK_RETCODE_OK, returncode)

    def test_fsck_multiple_mounts(self) -> None:
        mount2 = Path(self.mounts_dir) / "second_mount"
        mount3 = Path(self.mounts_dir) / "third_mount"
        mount4 = Path(self.mounts_dir) / "fourth_mount"

        self.eden.clone(self.repo_name, mount2)
        self.eden.clone(self.repo_name, mount3)
        self.eden.clone(self.repo_name, mount4)

        # Unmount all but mount3
        self.eden.unmount(Path(self.mount))
        self.eden.unmount(mount2)
        self.eden.unmount(mount4)

        # Running fsck should check all but mount3
        returncode, fsck_out = self.run_fsck()
        self.assertIn(f"Checking {self.mount}", fsck_out)
        self.assertIn(f"Checking {mount2}", fsck_out)
        self.assertIn(f"Not checking {mount3}", fsck_out)
        self.assertIn(f"Checking {mount4}", fsck_out)
        self.assertEqual(FSCK_RETCODE_SKIPPED, returncode)

        # Running fsck with --force should check everything
        returncode, fsck_out = self.run_fsck("--force")
        self.assertIn(f"Checking {self.mount}", fsck_out)
        self.assertIn(f"Checking {mount2}", fsck_out)
        self.assertIn(f"Checking {mount3}", fsck_out)
        self.assertIn(f"Checking {mount4}", fsck_out)
        self.assertEqual(FSCK_RETCODE_OK, returncode)


class FsckTestNoEdenfs(unittest.TestCase, TemporaryDirectoryMixin):
    def test_fsck_no_checkouts(self) -> None:
        tmp_dir = self.make_temporary_directory()
        eden = edenclient.EdenFS(Path(tmp_dir))
        cmd_result = eden.run_unchecked(
            "fsck",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            encoding="utf-8",
            errors="replace",
        )
        self.assertIn(
            "No Eden checkouts are configured.  Nothing to check.", cmd_result.stderr
        )
        self.assertEqual("", cmd_result.stdout)
        self.assertEqual(0, cmd_result.returncode)
