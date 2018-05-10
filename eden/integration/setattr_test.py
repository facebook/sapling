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
import time

from .lib import testcase


@testcase.eden_repo_test
class SetAttrTest(testcase.EdenRepoTest):

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    # mtime should not get changed on permission changes
    def test_chmod(self) -> None:
        filename = os.path.join(self.mount, "hello")

        st = os.lstat(filename)
        os.chmod(filename, st.st_mode | stat.S_IROTH)
        new_st = os.lstat(filename)
        self.assertGreaterEqual(new_st.st_atime, st.st_atime)

        self.assertEqual(new_st.st_mtime, st.st_mtime)
        self.assertEqual(new_st.st_atime, st.st_atime)
        self.assertGreaterEqual(new_st.st_ctime, st.st_ctime)
        self.assertEqual(new_st.st_mode, st.st_mode | stat.S_IROTH)

    def test_chown(self) -> None:
        filename = os.path.join(self.mount, "hello")

        # Chown should fail with EPERM unless we are setting it
        # to the same current ownership.
        # If we're running as root, this is EACCES instead.
        st = os.lstat(filename)
        os.chown(filename, st.st_uid, st.st_gid)

        with self.assertRaises(OSError) as context:
            os.chown(filename, st.st_uid + 1, st.st_gid)
        self.assertEqual(
            errno.EACCES if os.geteuid() == 0 else errno.EPERM,
            context.exception.errno,
            msg="changing uid of a file should raise EPERM/EACCES",
        )

        with self.assertRaises(OSError) as context:
            os.chown(filename, st.st_uid, st.st_gid + 1)
        self.assertEqual(
            errno.EACCES if os.geteuid() == 0 else errno.EPERM,
            context.exception.errno,
            msg="changing gid of a file should raise EPERM/EACCES",
        )

    def test_truncate(self) -> None:
        filename = os.path.join(self.mount, "hello")
        st = os.lstat(filename)

        with open(filename, "r+") as f:
            f.truncate(0)
            self.assertEqual("", f.read())

        new_st = os.lstat(filename)
        self.assertEqual(new_st.st_size, 0)
        self.assertGreaterEqual(new_st.st_atime, st.st_atime)
        self.assertGreaterEqual(new_st.st_ctime, st.st_ctime)
        self.assertGreaterEqual(new_st.st_mtime, st.st_mtime)

    def test_utime(self) -> None:
        filename = os.path.join(self.mount, "hello")

        # Update the atime and mtime to a time 5 seconds in the past.
        #
        # We round to the nearest second to avoid timestamp granularity issues.
        # (Eden currently uses the underlying overlay filesystem to store the
        # timestamps, and it might not necessarily support high resolution
        # timestamps.)
        timestamp = int(time.time() - 5)
        os.utime(filename, (timestamp, timestamp))
        st = os.lstat(filename)

        self.assertEqual(st.st_atime, timestamp)
        self.assertEqual(st.st_mtime, timestamp)

    def test_touch(self) -> None:
        filename = os.path.join(self.mount, "hello")

        now = time.time()
        subprocess.check_call(["touch", filename])
        st = os.lstat(filename)

        self.assertGreaterEqual(st.st_atime, now)
        self.assertGreaterEqual(st.st_mtime, now)

        newfile = os.path.join(self.mount, "touched-new-file")
        now = time.time()
        subprocess.check_call(["touch", newfile])
        st = os.lstat(newfile)

        self.assertGreaterEqual(st.st_atime, now)
        self.assertGreaterEqual(st.st_mtime, now)

    def test_umask(self) -> None:
        original_umask = os.umask(0o177)
        try:
            filename = os.path.join(self.mount, "test1")
            with open(filename, "w") as f:
                f.write("garbage")
            self.assertEqual(os.stat(filename).st_mode & 0o777, 0o600)
            filename = os.path.join(self.mount, "test2")
            os.umask(0o777)
            with open(filename, "w") as f:
                f.write("garbage")
            self.assertEqual(os.stat(filename).st_mode & 0o777, 0o000)
        finally:
            os.umask(original_umask)

    def test_dir_addfile(self) -> None:
        dirname = os.path.join(self.mount, "test_dir")
        self.mkdir("test_dir")

        st = os.lstat(dirname)
        self.write_file("test_file", "test string")
        new_st = os.lstat(dirname)

        self.assertEqual(new_st.st_atime, st.st_atime)
        self.assertGreaterEqual(new_st.st_ctime, st.st_ctime)
        self.assertGreaterEqual(new_st.st_mtime, st.st_mtime)

    def test_dir_delfile(self) -> None:
        dirname = os.path.join(self.mount, "test_dir")
        self.mkdir("test_dir")
        self.write_file("test_file", "test string")
        st = os.lstat(dirname)

        self.rm("test_file")
        new_st = os.lstat(dirname)

        self.assertEqual(new_st.st_atime, st.st_atime)
        self.assertGreaterEqual(new_st.st_ctime, st.st_ctime)
        self.assertGreaterEqual(new_st.st_mtime, st.st_mtime)

    def test_dir_change_filecontents(self) -> None:
        dirname = os.path.join(self.mount, "test_dir")
        self.mkdir("test_dir")

        self.write_file("test_file", "test string")
        st = os.lstat(dirname)
        self.write_file("test_file", "test string 1")
        new_st = os.lstat(dirname)

        self.assertEqual(new_st.st_mtime, st.st_mtime)
        self.assertEqual(new_st.st_ctime, st.st_ctime)
        self.assertEqual(new_st.st_mtime, st.st_mtime)

    # Changing permisssions of directory should change
    # only ctime of the directory, but not mtime and atime.
    def test_dir_change_perm(self) -> None:
        dirname = os.path.join(self.mount, "test_dir")
        self.mkdir("test_dir")

        st = os.lstat(dirname)
        os.chmod(dirname, st.st_mode | stat.S_IROTH)
        new_st = os.lstat(dirname)

        self.assertEqual(new_st.st_mtime, st.st_mtime)
        self.assertEqual(new_st.st_atime, st.st_atime)
        self.assertGreaterEqual(new_st.st_ctime, st.st_ctime)

    # Read call on a file in Edenfs should modify the atime of the file.
    # Also, open call should not change the timeStamps of a file.
    def test_timestamp_openfiles(self) -> None:
        filename = os.path.join(self.mount, "hello")
        st = os.lstat(filename)
        with open(filename, "r") as f:
            new_st = os.lstat(filename)
            self.assertEqual(new_st.st_mtime, st.st_mtime)
            self.assertEqual(new_st.st_atime, st.st_atime)
            self.assertEqual(new_st.st_ctime, st.st_ctime)
            f.read()
            f.close()

        new_st = os.lstat(filename)
        self.assertEqual(new_st.st_mtime, st.st_mtime)
        self.assertGreater(new_st.st_atime, st.st_atime)
        self.assertEqual(new_st.st_ctime, st.st_ctime)
