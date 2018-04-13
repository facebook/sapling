#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class StatusTest(EdenHgTestCase):
    def populate_backing_repo(self, repo):
        repo.write_file('hello.txt', 'hola')
        repo.commit('Initial commit.')

    def test_status(self):
        '''Test various `hg status` states in the root of an Eden mount.'''
        self.assert_status_empty()

        self.touch('world.txt')
        self.assert_status({'world.txt': '?'})

        self.hg('add', 'world.txt')
        self.assert_status({'world.txt': 'A'})

        self.rm('hello.txt')
        self.assert_status({
            'hello.txt': '!',
            'world.txt': 'A',
        })

        with open(self.get_path('hello.txt'), 'w') as f:
            f.write('new contents')
        self.assert_status({
            'hello.txt': 'M',
            'world.txt': 'A',
        })

        self.hg('forget', 'hello.txt')
        self.assert_status({
            'hello.txt': 'R',
            'world.txt': 'A',
        })
        self.assertEqual('new contents', self.read_file('hello.txt'))

        self.hg('rm', 'hello.txt')
        self.assert_status({
            'hello.txt': 'R',
            'world.txt': 'A',
        })
        # If the file is already forgotten, `hg rm` does not remove it from
        # disk.
        self.assertEqual('new contents', self.read_file('hello.txt'))

        self.hg('add', 'hello.txt')
        self.assert_status({
            'hello.txt': 'M',
            'world.txt': 'A',
        })
        self.assertEqual('new contents', self.read_file('hello.txt'))

        self.hg('rm', '--force', 'hello.txt')
        self.assert_status({
            'hello.txt': 'R',
            'world.txt': 'A',
        })
        self.assertFalse(os.path.exists(self.get_path('hello.txt')))
