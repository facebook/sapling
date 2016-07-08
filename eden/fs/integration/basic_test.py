#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
import os
import stat
from .lib import testcase


class BasicTest:
    '''Exercise some fundamental properties of the filesystem.

    Listing directories, checking stat information, asserting
    that the filesystem is reporting the basic information
    about the sample git repo and that it is correct are all
    things that are appropriate to include in this test case.
    '''
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file('adir/file', 'foo!\n')
        self.repo.symlink('slink', 'hello')
        self.repo.commit('Initial commit.')

    def test_fileList(self):
        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['adir', 'hello', 'slink'], entries)

        adir = os.path.join(self.mount, 'adir')
        st = os.lstat(adir)
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(st.st_uid, os.getuid())
        self.assertEqual(st.st_gid, os.getgid())

        hello = os.path.join(self.mount, 'hello')
        st = os.lstat(hello)
        self.assertTrue(stat.S_ISREG(st.st_mode))

        slink = os.path.join(self.mount, 'slink')
        st = os.lstat(slink)
        self.assertTrue(stat.S_ISLNK(st.st_mode))

    def test_symlinks(self):
        slink = os.path.join(self.mount, 'slink')
        self.assertEqual(os.readlink(slink), 'hello')

    def test_regular(self):
        hello = os.path.join(self.mount, 'hello')
        with open(hello, 'r') as f:
            self.assertEqual('hola\n', f.read())

    def test_dir(self):
        entries = sorted(os.listdir(os.path.join(self.mount, 'adir')))
        self.assertEqual(['file'], entries)

        filename = os.path.join(self.mount, 'adir', 'file')
        with open(filename, 'r') as f:
            self.assertEqual('foo!\n', f.read())

    def test_create(self):
        filename = os.path.join(self.mount, 'notinrepo')
        with open(filename, 'w') as f:
            f.write('created\n')

        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['adir', 'hello', 'notinrepo', 'slink'], entries)

        with open(filename, 'r') as f:
            self.assertEqual(f.read(), 'created\n')

        st = os.lstat(filename)
        self.assertEqual(st.st_size, 8)
        self.assertTrue(stat.S_ISREG(st.st_mode))

    def test_overwrite(self):
        hello = os.path.join(self.mount, 'hello')
        with open(hello, 'w') as f:
            f.write('replaced\n')

        st = os.lstat(hello)
        self.assertEqual(st.st_size, len('replaced\n'))

    def test_materialize(self):
        hello = os.path.join(self.mount, 'hello')
        # Opening for write should materialize the file with the same
        # contents that we expect
        with open(hello, 'r+') as f:
            self.assertEqual('hola\n', f.read())

        st = os.lstat(hello)
        self.assertEqual(st.st_size, len('hola\n'))

    def test_mkdir(self):
        # Can't create a directory inside a file that is in the store
        with self.assertRaises(OSError) as context:
            os.mkdir(os.path.join(self.mount, 'hello', 'world'))
        self.assertEqual(context.exception.errno, errno.ENOTDIR)

        # Can't create a directory when a file of that name already exists
        with self.assertRaises(OSError) as context:
            os.mkdir(os.path.join(self.mount, 'hello'))
        self.assertEqual(context.exception.errno, errno.EEXIST)

        # Can't create a directory when a directory of that name already exists
        with self.assertRaises(OSError) as context:
            os.mkdir(os.path.join(self.mount, 'adir'))
        self.assertEqual(context.exception.errno, errno.EEXIST)

        buckout = os.path.join(self.mount, 'buck-out')
        os.mkdir(buckout)
        st = os.lstat(buckout)
        self.assertTrue(stat.S_ISDIR(st.st_mode))

        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['adir', 'buck-out', 'hello', 'slink'], entries)

        # Prove that we can recursively build out a directory tree
        deep_name = os.path.join(buckout, 'foo', 'bar', 'baz')
        os.makedirs(deep_name)
        st = os.lstat(deep_name)
        self.assertTrue(stat.S_ISDIR(st.st_mode))

        # And that we can create a file in there too
        deep_file = os.path.join(deep_name, 'file')
        with open(deep_file, 'w') as f:
            f.write('w00t')
        st = os.lstat(deep_file)
        self.assertTrue(stat.S_ISREG(st.st_mode))

    def test_unmount(self):
        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['adir', 'hello', 'slink'], entries)

        self.assertTrue(self.eden.in_proc_mounts(self.mount))

        self.eden.unmount(self.mount)

        entries = sorted(os.listdir(self.mount))
        self.assertEqual([], entries)

        self.assertFalse(self.eden.in_proc_mounts(self.mount))

        self.eden.clone(self.repo_name, self.mount)

        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['adir', 'hello', 'slink'], entries)

        self.assertTrue(self.eden.in_proc_mounts(self.mount))


class BasicTestGit(BasicTest, testcase.EdenGitTest):
    pass


class BasicTestHg(BasicTest, testcase.EdenHgTest):
    pass
