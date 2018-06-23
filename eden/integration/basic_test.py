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
import stat
import subprocess

from .lib import testcase


@testcase.eden_repo_test
class BasicTest(testcase.EdenRepoTest):
    """Exercise some fundamental properties of the filesystem.

    Listing directories, checking stat information, asserting
    that the filesystem is reporting the basic information
    about the sample git repo and that it is correct are all
    things that are appropriate to include in this test case.
    """

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        self.repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        self.repo.symlink("slink", "hello")
        self.repo.commit("Initial commit.")

        self.expected_mount_entries = {".eden", "adir", "bdir", "hello", "slink"}

    def test_version(self) -> None:
        output = self.eden.run_cmd("version", cwd=self.mount)
        lines = output.splitlines()

        # The first line reports info about the installed RPM version
        rpm_info = lines[0]
        self.assertTrue(rpm_info.startswith("Installed: "))
        if "Not Installed" in rpm_info:
            # This system does not have Eden installed as an RPM.
            pass
        else:
            parts = rpm_info[11:].split("-")
            self.assertEqual(len(parts[0]), 8)
            self.assertEqual(len(parts[1]), 6)
            self.assertTrue(parts[0].isdigit())
            self.assertTrue(parts[1].isdigit())

        # The second line reports info about the current running edenfs process
        running_info = lines[1]
        self.assertTrue(running_info.startswith("Running: "))

    def test_fileList(self) -> None:
        entries = set(os.listdir(self.mount))
        self.assertEqual(self.expected_mount_entries, entries)

        adir = os.path.join(self.mount, "adir")
        st = os.lstat(adir)
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(st.st_uid, os.getuid())
        self.assertEqual(st.st_gid, os.getgid())

        hello = os.path.join(self.mount, "hello")
        st = os.lstat(hello)
        self.assertTrue(stat.S_ISREG(st.st_mode))

        slink = os.path.join(self.mount, "slink")
        st = os.lstat(slink)
        self.assertTrue(stat.S_ISLNK(st.st_mode))

    def test_symlinks(self) -> None:
        slink = os.path.join(self.mount, "slink")
        self.assertEqual(os.readlink(slink), "hello")

    def test_regular(self) -> None:
        hello = os.path.join(self.mount, "hello")
        with open(hello, "r") as f:
            self.assertEqual("hola\n", f.read())

    def test_dir(self) -> None:
        entries = sorted(os.listdir(os.path.join(self.mount, "adir")))
        self.assertEqual(["file"], entries)

        filename = os.path.join(self.mount, "adir", "file")
        with open(filename, "r") as f:
            self.assertEqual("foo!\n", f.read())

    def test_create(self) -> None:
        filename = os.path.join(self.mount, "notinrepo")
        with open(filename, "w") as f:
            f.write("created\n")

        entries = set(os.listdir(self.mount))
        self.assertEqual(self.expected_mount_entries | {"notinrepo"}, entries)

        with open(filename, "r") as f:
            self.assertEqual(f.read(), "created\n")

        st = os.lstat(filename)
        self.assertEqual(st.st_size, 8)
        self.assertTrue(stat.S_ISREG(st.st_mode))

    def test_overwrite(self) -> None:
        hello = os.path.join(self.mount, "hello")
        with open(hello, "w") as f:
            f.write("replaced\n")

        st = os.lstat(hello)
        self.assertEqual(st.st_size, len("replaced\n"))

    def test_append(self) -> None:
        hello = os.path.join(self.mount, "bdir/test.sh")
        with open(hello, "a") as f:
            f.write("echo more commands\n")

        expected_data = "#!/bin/bash\necho test\necho more commands\n"
        st = os.lstat(hello)
        with open(hello, "r") as f:
            read_back = f.read()
        self.assertEqual(expected_data, read_back)
        self.assertEqual(len(expected_data), st.st_size)

    def test_materialize(self) -> None:
        hello = os.path.join(self.mount, "hello")
        # Opening for write should materialize the file with the same
        # contents that we expect
        with open(hello, "r+") as f:
            self.assertEqual("hola\n", f.read())

        st = os.lstat(hello)
        self.assertEqual(st.st_size, len("hola\n"))

    def test_mkdir(self) -> None:
        # Can't create a directory inside a file that is in the store
        with self.assertRaises(OSError) as context:
            os.mkdir(os.path.join(self.mount, "hello", "world"))
        self.assertEqual(context.exception.errno, errno.ENOTDIR)

        # Can't create a directory when a file of that name already exists
        with self.assertRaises(OSError) as context:
            os.mkdir(os.path.join(self.mount, "hello"))
        self.assertEqual(context.exception.errno, errno.EEXIST)

        # Can't create a directory when a directory of that name already exists
        with self.assertRaises(OSError) as context:
            os.mkdir(os.path.join(self.mount, "adir"))
        self.assertEqual(context.exception.errno, errno.EEXIST)

        buckout = os.path.join(self.mount, "buck-out")
        os.mkdir(buckout)
        st = os.lstat(buckout)
        self.assertTrue(stat.S_ISDIR(st.st_mode))

        entries = set(os.listdir(self.mount))
        self.assertEqual(self.expected_mount_entries | {"buck-out"}, entries)

        # Prove that we can recursively build out a directory tree
        deep_name = os.path.join(buckout, "foo", "bar", "baz")
        os.makedirs(deep_name)
        st = os.lstat(deep_name)
        self.assertTrue(stat.S_ISDIR(st.st_mode))

        # And that we can create a file in there too
        deep_file = os.path.join(deep_name, "file")
        with open(deep_file, "w") as f:
            f.write("w00t")
        st = os.lstat(deep_file)
        self.assertTrue(stat.S_ISREG(st.st_mode))

    def test_mkdir_umask(self):
        original_umask = os.umask(0o177)
        try:
            dirname = os.path.join(self.mount, "testd1")
            os.mkdir(dirname)
            self.assertEqual(0o600, os.lstat(dirname).st_mode & 0o777)
            dirname = os.path.join(self.mount, "testd2")
            os.umask(0o777)
            os.mkdir(dirname)
            self.assertEqual(0o000, os.lstat(dirname).st_mode & 0o777)
        finally:
            os.umask(original_umask)

    def test_access(self) -> None:
        def check_access(path: str, mode: int) -> bool:
            return os.access(os.path.join(self.mount, path), mode)

        self.assertTrue(check_access("hello", os.R_OK))
        self.assertTrue(check_access("hello", os.W_OK))
        self.assertFalse(check_access("hello", os.X_OK))

        self.assertTrue(check_access("bdir/test.sh", os.R_OK))
        self.assertTrue(check_access("bdir/test.sh", os.W_OK))
        self.assertTrue(check_access("bdir/test.sh", os.X_OK))

        self.assertTrue(check_access("bdir/noexec.sh", os.R_OK))
        self.assertTrue(check_access("bdir/noexec.sh", os.W_OK))
        self.assertFalse(check_access("bdir/noexec.sh", os.X_OK))

        cmd = [os.path.join(self.mount, "bdir/test.sh")]
        out = subprocess.check_output(cmd, stderr=subprocess.STDOUT)
        self.assertEqual(out, b"test\n")

        cmd = [os.path.join(self.mount, "bdir/noexec.sh")]
        with self.assertRaises(OSError) as context:
            out = subprocess.check_output(cmd, stderr=subprocess.STDOUT)
        self.assertEqual(
            errno.EACCES,
            context.exception.errno,
            msg="attempting to run noexec.sh should fail with " "EACCES",
        )

    def test_remove_invalid_paths(self) -> None:
        self.eden.run_unchecked("remove", "/tmp")
        self.eden.run_unchecked("remove", "/root")

    def test_remove_checkout(self) -> None:
        entries = set(os.listdir(self.mount))
        self.assertEqual(self.expected_mount_entries, entries)

        self.assertTrue(self.eden.in_proc_mounts(self.mount))

        self.eden.remove(self.mount)

        self.assertFalse(self.eden.in_proc_mounts(self.mount))
        self.assertFalse(os.path.exists(self.mount))

        self.eden.clone(self.repo_name, self.mount)

        entries = set(os.listdir(self.mount))
        self.assertEqual(self.expected_mount_entries, entries)

        self.assertTrue(self.eden.in_proc_mounts(self.mount))

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

        # Now use "eden remove" to destroy mount2
        self.eden.remove(mount2)
        self.assertEqual({self.mount: self.eden.CLIENT_ACTIVE}, self.eden.list_cmd())

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
        self.assertEqual(set(), entries)

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

    def test_statvfs(self) -> None:
        hello_path = os.path.join(self.mount, "hello")
        fs_info = os.statvfs(hello_path)
        self.assertGreaterEqual(fs_info.f_namemax, 255)
        self.assertGreaterEqual(fs_info.f_frsize, 4096)
        self.assertGreaterEqual(fs_info.f_bsize, 4096)

        self.assertGreaterEqual(os.pathconf(hello_path, "PC_NAME_MAX"), 255)
        self.assertGreaterEqual(os.pathconf(hello_path, "PC_PATH_MAX"), 4096)
        self.assertGreaterEqual(os.pathconf(hello_path, "PC_REC_XFER_ALIGN"), 4096)
        self.assertGreaterEqual(os.pathconf(hello_path, "PC_ALLOC_SIZE_MIN"), 4096)
        self.assertGreaterEqual(os.pathconf(hello_path, "PC_REC_MIN_XFER_SIZE"), 4096)
