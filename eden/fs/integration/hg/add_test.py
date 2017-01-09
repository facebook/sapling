#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import HgExtensionTestBase
import subprocess


class AddTest(HgExtensionTestBase):
    def populate_repo(self):
        self.repo.write_file('rootfile.txt', '')
        self.repo.write_file('dir1/a.txt', 'original contents')
        self.repo.commit('Initial commit.')

    def test_add(self):
        self.touch('dir1/b.txt')
        self.mkdir('dir2')
        self.touch('dir2/c.txt')
        self.assertEqual('? dir1/b.txt\n? dir2/c.txt\n', self.status())

        # `hg add dir2` should ensure only things under dir2 are added.
        self.hg('add', 'dir2')
        self.assertEqual('A dir2/c.txt\n? dir1/b.txt\n', self.status())

        # This is the equivalent of `hg forget dir1/a.txt`.
        self.hg('rm', '--force', 'dir1/a.txt')
        self.write_file('dir1/a.txt', 'original contents')
        self.touch('dir1/a.txt')
        self.assertEqual('A dir2/c.txt\nR dir1/a.txt\n? dir1/b.txt\n',
                         self.status())

        # Running `hg add .` should remove the removal marker from dir1/a.txt
        # because dir1/a.txt is still on disk.
        self.hg('add')
        self.assertEqual('A dir1/b.txt\nA dir2/c.txt\n', self.status())

        self.hg('rm', 'dir1/a.txt')
        self.write_file('dir1/a.txt', 'different contents')
        # Running `hg add dir1` should remove the removal marker from
        # dir1/a.txt, but `hg status` should also reflect that it is modified.
        self.hg('add', 'dir1')
        self.assertEqual('M dir1/a.txt\nA dir1/b.txt\nA dir2/c.txt\n',
                         self.status())

        self.hg('rm', '--force', 'dir1/a.txt')
        # This should not add dir1/a.txt back because it is not on disk.
        self.hg('add', 'dir1')
        self.assertEqual('A dir1/b.txt\nA dir2/c.txt\nR dir1/a.txt\n',
                         self.status())

        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.hg('add', 'dir3')
        self.assertEqual('dir3: No such file or directory\n',
                         context.exception.output.decode('utf-8'))
        self.assertEqual(1, context.exception.returncode)
