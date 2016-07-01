#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib import testcase
import errno
import os
import stat
import subprocess
import time


class SetAttrTest(testcase.EdenTestCase):
    def test_chmod(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')

        st = os.lstat(filename)
        os.chmod(filename, st.st_mode | stat.S_IROTH)
        new_st = os.lstat(filename)
        self.assertGreaterEqual(new_st.st_atime, st.st_atime)
        self.assertGreaterEqual(new_st.st_mtime, st.st_mtime)
        self.assertEqual(new_st.st_mode, st.st_mode | stat.S_IROTH)

    def test_chown(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')

        # Chown should fail with EACCESS unless we are setting it
        # to the same current ownership
        st = os.lstat(filename)
        os.chown(filename, st.st_uid, st.st_gid)

        with self.assertRaises(OSError) as context:
            os.chown(filename, st.st_uid + 1, st.st_gid)
        self.assertEqual(errno.EACCES, context.exception.errno,
                         msg="changing uid of a file should raise EACCESS")

        with self.assertRaises(OSError) as context:
            os.chown(filename, st.st_uid, st.st_gid + 1)
        self.assertEqual(errno.EACCES, context.exception.errno,
                         msg="changing gid of a file should raise EACCESS")

    def test_truncate(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')

        with open(filename, 'r+') as f:
            f.truncate(0)
            self.assertEqual('', f.read())

        st = os.lstat(filename)
        self.assertEqual(st.st_size, 0)

    def test_utime(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')

        now = time.time()
        os.utime(filename)
        st = os.lstat(filename)

        self.assertGreaterEqual(st.st_atime, now)
        self.assertGreaterEqual(st.st_mtime, now)

    def test_touch(self):
        eden = self.init_git_eden()
        filename = os.path.join(eden.mount_path, 'hello')

        now = time.time()
        subprocess.check_call(['touch', filename])
        st = os.lstat(filename)

        self.assertGreaterEqual(st.st_atime, now)
        self.assertGreaterEqual(st.st_mtime, now)

        newfile = os.path.join(eden.mount_path, 'touched-new-file')
        now = time.time()
        subprocess.check_call(['touch', newfile])
        st = os.lstat(newfile)

        self.assertGreaterEqual(st.st_atime, now)
        self.assertGreaterEqual(st.st_mtime, now)
