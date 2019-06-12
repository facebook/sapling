#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
import os
import subprocess

import __manifest__

from .lib import testcase


class StaleTest(testcase.HgRepoTestMixin, testcase.EdenRepoTest):
    """Test that various Eden CLI commands work even when invoked from a
    current working directory that is inside a stale FUSE mount point.
    """

    def setup_eden_test(self) -> None:
        super().setup_eden_test()  # type: ignore (pyre does not follow MRO correctly)

        # Change into the mount point directory.
        # We have to do this before unmounting the mount point--the chdir() call itself
        # will fail afterwards.  If we want to execute the eden CLI with a stale current
        # directory we have to CD into it before it goes stale.
        orig_cwd = os.getcwd()
        os.chdir(self.mount)
        self.addCleanup(os.chdir, orig_cwd)

        self.eden.stop_with_stale_mounts()
        self.addCleanup(self.cleanup_mount)

        # Sanity check that accessing the mount does fail with ESTALE now
        with self.assertRaises(OSError) as ctx:
            os.listdir(self.mount)
            self.fail("expected listdir to fail with ENOTCONN")
        self.assertEqual(ctx.exception.errno, errno.ENOTCONN)

    def populate_repo(self) -> None:
        self.repo.write_file("src/foo.txt", "foo\n")
        self.repo.commit("Initial commit.")

    def cleanup_mount(self) -> None:
        cmd = ["sudo", "/bin/umount", "-lf", self.mount]
        subprocess.call(cmd)

    def _expected_to_fail(self) -> bool:
        # xar files are able to start correctly even when the current working
        # directory is not working, but unfortunately lpar files aren't.
        # Some core python library code fails early on during import bootstrapping.
        return __manifest__.fbmake["par_style"] == "live"

    def test_list(self) -> None:
        cmd_result = self.eden.run_unchecked("list", stdout=subprocess.PIPE)
        if not self._expected_to_fail():
            self.assertIn(
                f"{self.mount} (not mounted)".encode("utf-8"), cmd_result.stdout
            )
            self.assertEqual(0, cmd_result.returncode)

    def test_doctor(self) -> None:
        cmd_result = self.eden.run_unchecked("doctor", "-n", stdout=subprocess.PIPE)
        if not self._expected_to_fail():
            # We don't check for the exact number of stale mounts:
            # even though we expect there to only be one, more than one may be reported
            # if /tmp is also bind mounted to another location.  `eden doctor` also
            # scans the entire system so it may report additional stale mounts if there
            # are other existing stale mounts on the system.
            self.assertIn(b"stale edenfs mount", cmd_result.stdout)
            self.assertIn(self.mount.encode("utf-8"), cmd_result.stdout)
            self.assertEqual(1, cmd_result.returncode)
