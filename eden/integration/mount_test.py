#!/usr/bin/env python3
#
# Copyright (c) 2018-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
from typing import Set

from .lib import testcase


@testcase.eden_repo_test
class MountTest(testcase.EdenRepoTest):
    expected_mount_entries: Set[str]

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        self.repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        self.repo.symlink("slink", "hello")
        self.repo.commit("Initial commit.")

        self.expected_mount_entries = {".eden", "adir", "bdir", "hello", "slink"}

    def test_remove_unmounted_checkout(self) -> None:
        # Clone a second checkout mount point
        mount2 = os.path.join(self.mounts_dir, "mount2")
        self.eden.clone(self.repo_name, mount2)
        self.assertEqual(
            {self.mount: self.eden.CLIENT_ACTIVE, mount2: self.eden.CLIENT_ACTIVE},
            self.eden.list_cmd(),
        )

        # Now unmount it
        self.eden.run_cmd("unmount", mount2)
        self.assertEqual(
            {self.mount: self.eden.CLIENT_ACTIVE, mount2: self.eden.CLIENT_INACTIVE},
            self.eden.list_cmd(),
        )
        # The Eden README telling users what to do if their mount point is not mounted
        # should be present in the original mount point directory.
        self.assertTrue(os.path.exists(os.path.join(mount2, "README_EDEN.txt")))

        # Now use "eden remove" to destroy mount2
        self.eden.remove(mount2)
        self.assertEqual({self.mount: self.eden.CLIENT_ACTIVE}, self.eden.list_cmd())
        self.assertFalse(os.path.exists(mount2))

    def test_unmount_remount(self) -> None:
        # write a file into the overlay to test that it is still visible
        # when we remount.
        filename = os.path.join(self.mount, "overlayonly")
        with open(filename, "w") as f:
            f.write("foo!\n")

        entries = set(os.listdir(self.mount))
        self.assertEqual(self.expected_mount_entries | {"overlayonly"}, entries)
        self.assertTrue(self.eden.in_proc_mounts(self.mount))

        # do a normal user-facing unmount, preserving state
        self.eden.run_cmd("unmount", self.mount)

        self.assertFalse(self.eden.in_proc_mounts(self.mount))
        entries = set(os.listdir(self.mount))
        self.assertEqual({"README_EDEN.txt"}, entries)

        # Now remount it with the mount command
        self.eden.run_cmd("mount", self.mount)

        self.assertTrue(self.eden.in_proc_mounts(self.mount))
        entries = set(os.listdir(self.mount))
        self.assertEqual(self.expected_mount_entries | {"overlayonly"}, entries)

        with open(filename, "r") as f:
            self.assertEqual("foo!\n", f.read(), msg="overlay file is correct")

    def test_double_unmount(self) -> None:
        # Test calling "unmount" twice.  The second should fail, but edenfs
        # should still work normally afterwards
        self.eden.run_cmd("unmount", self.mount)
        self.eden.run_unchecked("unmount", self.mount)

        # Now remount it with the mount command
        self.eden.run_cmd("mount", self.mount)

        self.assertTrue(self.eden.in_proc_mounts(self.mount))
        entries = sorted(os.listdir(self.mount))
        self.assertEqual([".eden", "adir", "bdir", "hello", "slink"], entries)
