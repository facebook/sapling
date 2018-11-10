#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import pathlib
import subprocess
from pathlib import Path
from typing import Tuple

from .lib import overlay, repobase, testcase


FSCK_RETCODE_OK = 0
FSCK_RETCODE_SKIPPED = 1
FSCK_RETCODE_WARNINGS = 2
FSCK_RETCODE_ERRORS = 3


class FsckTest(testcase.EdenRepoTest):
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
        self.overlay = overlay.OverlayStore(self.eden, self.mount_path)

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

    def test_fsck_force(self) -> None:
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

    def test_fsck_empty_overlay_file(self) -> None:
        overlay_path = self.overlay.materialize_file(pathlib.Path("doc/foo.txt"))
        self.eden.run_cmd("unmount", self.mount)

        # Truncate the file to 0 length
        with overlay_path.open("wb"):
            pass

        returncode, fsck_out = self.run_fsck()
        self.assertRegex(
            fsck_out,
            r"invalid overlay file for materialized file .* \(doc/foo.txt\).*: "
            r"zero-sized overlay file",
        )
        self.assertRegex(fsck_out, r"\b1 errors")
        self.assertEqual(FSCK_RETCODE_ERRORS, returncode)

    def test_fsck_multiple_mounts(self) -> None:
        self.mount2 = Path(self.mounts_dir) / "second_mount"
        self.mount3 = Path(self.mounts_dir) / "third_mount"
        self.mount4 = Path(self.mounts_dir) / "fourth_mount"

        self.eden.clone(self.repo_name, self.mount2)
        self.eden.clone(self.repo_name, self.mount3)
        self.eden.clone(self.repo_name, self.mount4)

        # Unmount all but mount3
        self.eden.unmount(Path(self.mount))
        self.eden.unmount(self.mount2)
        self.eden.unmount(self.mount4)

        # Running fsck should check all but mount3
        returncode, fsck_out = self.run_fsck()
        self.assertIn(f"Checking {self.mount}", fsck_out)
        self.assertIn(f"Checking {self.mount2}", fsck_out)
        self.assertIn(f"Not checking {self.mount3}", fsck_out)
        self.assertIn(f"Checking {self.mount4}", fsck_out)
        self.assertEqual(FSCK_RETCODE_SKIPPED, returncode)

        # Running fsck with --force should check everything
        returncode, fsck_out = self.run_fsck("--force")
        self.assertIn(f"Checking {self.mount}", fsck_out)
        self.assertIn(f"Checking {self.mount2}", fsck_out)
        self.assertIn(f"Checking {self.mount3}", fsck_out)
        self.assertIn(f"Checking {self.mount4}", fsck_out)
        self.assertEqual(FSCK_RETCODE_OK, returncode)
