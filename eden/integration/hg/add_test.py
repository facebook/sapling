#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import hg_test
import os
import subprocess
import unittest


@hg_test
class AddTest:
    def populate_backing_repo(self, repo):
        repo.write_file('rootfile.txt', '')
        repo.write_file('dir1/a.txt', 'original contents')
        repo.commit('Initial commit.')

    def test_add(self):
        self.touch('dir1/b.txt')
        self.mkdir('dir2')
        self.touch('dir2/c.txt')
        self.assert_status({
            'dir1/b.txt': '?',
            'dir2/c.txt': '?',
        })

        # `hg add dir2` should ensure only things under dir2 are added.
        self.hg('add', 'dir2')
        self.assert_status({
            'dir1/b.txt': '?',
            'dir2/c.txt': 'A',
        })

        # This is the equivalent of `hg forget dir1/a.txt`.
        self.hg('rm', '--force', 'dir1/a.txt')
        self.write_file('dir1/a.txt', 'original contents')
        self.touch('dir1/a.txt')
        self.assert_status({
            'dir1/a.txt': 'R',
            'dir1/b.txt': '?',
            'dir2/c.txt': 'A',
        })

        # Running `hg add .` should remove the removal marker from dir1/a.txt
        # because dir1/a.txt is still on disk.
        self.hg('add')
        self.assert_status({
            'dir1/b.txt': 'A',
            'dir2/c.txt': 'A',
        })

        self.hg('rm', 'dir1/a.txt')
        self.write_file('dir1/a.txt', 'different contents')
        # Running `hg add dir1` should remove the removal marker from
        # dir1/a.txt, but `hg status` should also reflect that it is modified.
        self.hg('add', 'dir1')
        self.assert_status({
            'dir1/a.txt': 'M',
            'dir1/b.txt': 'A',
            'dir2/c.txt': 'A',
        })

        self.hg('rm', '--force', 'dir1/a.txt')
        # This should not add dir1/a.txt back because it is not on disk.
        self.hg('add', 'dir1')
        self.assert_status({
            'dir1/a.txt': 'R',
            'dir1/b.txt': 'A',
            'dir2/c.txt': 'A',
        })

    @unittest.skip('Need to add precondition checks that true Hg has.')
    def test_add_nonexistent_directory(self):
        # I believe this does not pass today because _eden_walk_helper does not
        # invoke the bad() method of the matcher.
        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.hg('add', 'dir3')
        self.assertEqual('dir3: No such file or directory\n',
                         context.exception.output.decode('utf-8'))
        self.assertEqual(1, context.exception.returncode)

    @unittest.skip('Try re-enabling as part of the great dirstate refactor.')
    def test_try_replacing_directory_with_file(self):
        # `hg rm` the only file in a directory, which should also remove the
        # directory.
        self.hg('rm', 'dir1/a.txt')
        self.assert_status({
            'dir1/a.txt': 'R',
        })
        self.assertFalse(os.path.exists(self.get_path('dir1')))

        # Create an ordinary file with the same name as the directory that was
        # removed and `hg add` it.
        self.write_file('dir1', 'Now I am an ordinary file.\n')
        self.assert_status({
            'dir1': '?',
            'dir1/a.txt': 'R',
        })

        self.hg('add', 'dir1')  # Currently, this throws an exception.
        self.assert_status({
            'dir1': 'A',
            'dir1/a.txt': 'R',
        })
