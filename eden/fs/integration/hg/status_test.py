#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import HgExtensionTestBase


class StatusTest(HgExtensionTestBase):
    def populate_repo(self):
        self.repo.write_file('hello.txt', 'hola')
        self.repo.commit('Initial commit.')

    def test_status(self):
        '''Test various `hg status` states in the root of an Eden mount.'''
        empty_status = self.status()
        self.assertEqual('', empty_status)

        self.touch('world.txt')
        untracked_status = self.status()
        self.assertEqual('? world.txt\n', untracked_status)

        self.hg('add', 'world.txt')
        added_status = self.status()
        self.assertEqual('A world.txt\n', added_status)

        self.rm('hello.txt')
        missing_status = self.status()
        self.assertEqual('A world.txt\n! hello.txt\n', missing_status)

        with open(self.get_path('hello.txt'), 'w') as f:
            f.write('new contents')
        modified_status = self.status()
        self.assertEqual('M hello.txt\nA world.txt\n', modified_status)

        self.hg('rm', '--force', 'hello.txt')
        removed_status = self.status()
        self.assertEqual('A world.txt\nR hello.txt\n', removed_status)
