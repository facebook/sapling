#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib import testcase
import os
import stat
import subprocess


class SedTest(testcase.EdenTestCase):
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.commit('Initial commit.')

    def test_sed(self):
        filename = os.path.join(self.mount, 'sedme')

        with open(filename, 'w') as f:
            f.write('foo\n')

        before_st = os.lstat(filename)
        self.assertTrue(stat.S_ISREG(before_st.st_mode))

        subprocess.check_call(['sed', '-i', '-e', 's/foo/bar/', filename])

        after_st = os.lstat(filename)
        self.assertNotEqual(after_st.st_ino, before_st.st_ino,
                            msg='renamed file has a new inode number')
        self.assertEqual(4, after_st.st_size)
        with open(filename, 'r') as f:
            file_st = os.fstat(f.fileno())
            self.assertEqual(after_st.st_ino, file_st.st_ino)
            self.assertEqual('bar\n', f.read())


class SedTestGit(SedTest, testcase.EdenGitTest):
    pass


class SedTestHg(SedTest, testcase.EdenHgTest):
    pass
