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
from .lib import testcase


@testcase.eden_repo_test
class DotEdenTest(testcase.EdenRepoTest):
    '''\
    Verify manipulating the .eden directory is disallowed.
    '''

    def populate_repo(self) -> None:
        self.repo.write_file('hello', 'hola\n')
        self.repo.commit('Initial commit.')
        self.dot_eden_path = os.path.join(self.mount, '.eden')

    def setUp(self):
        super().setUp()
        self.entries = os.listdir(self.dot_eden_path)
        self.assertNotEqual([], self.entries)

    def test_rm_existing_contents_fails(self) -> None:
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.unlink(os.path.join(self.dot_eden_path, entry))
            self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_mkdir_fails(self):
        with self.assertRaises(OSError) as cm:
            os.mkdir(os.path.join(self.dot_eden_path, 'subdir'))
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_rmdir_fails(self):
        with self.assertRaises(OSError) as cm:
            os.rmdir(os.path.join(self.dot_eden_path, 'subdir'))
        # It is no longer possible to create a directory inside .eden -
        # if it was, EPERM would be the right errno value.
        self.assertEqual(errno.ENOENT, cm.exception.errno)

    def test_create_file_fails(self):
        with self.assertRaises(OSError) as cm:
            os.open(os.path.join(self.dot_eden_path, 'file'),
                    os.O_CREAT | os.O_RDWR)
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_mknod_fails(self):
        with self.assertRaises(OSError) as cm:
            os.mknod(os.path.join(self.dot_eden_path, 'file'),
                     stat.S_IFREG | 0o600)
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_symlink_fails(self):
        with self.assertRaises(OSError) as cm:
            os.symlink('/', os.path.join(self.dot_eden_path, 'lnk'))
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_rename_in_fails(self):
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.rename(os.path.join(self.dot_eden_path, entry),
                          os.path.join(self.dot_eden_path, 'dst'))
            self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_rename_from_fails(self):
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.rename(os.path.join(self.dot_eden_path, entry),
                          os.path.join(self.mount, 'dst'))
            self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_rename_to_fails(self):
        with self.assertRaises(OSError) as cm:
            os.rename(os.path.join(self.mount, 'hello'),
                      os.path.join(self.dot_eden_path, 'dst'))
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_chown_fails(self):
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.chown(os.path.join(self.dot_eden_path, entry),
                         uid=os.getuid(),
                         gid=os.getgid(),
                         follow_symlinks=False)
            self.assertEqual(errno.EPERM, cm.exception.errno)

    # Linux does not allow setting permissions on a symlink.
    def xtest_chmod_fails(self):
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.chmod(os.path.join(self.dot_eden_path, entry), 0o543,
                         follow_symlinks=False)
            self.assertEqual(errno.EPERM, cm.exception.errno)

    # utime() has no effect on symlinks.
    def xtest_touch_fails(self):
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.utime(os.path.join(self.dot_eden_path, entry))
            self.assertEqual(errno.EPERM, cm.exception.errno)
