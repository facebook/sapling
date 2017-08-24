#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import hg_test


@hg_test
class StatusTest:
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

        self.hg('rm', '--force', 'hello.txt')
        self.assert_status({
            'hello.txt': 'R',
            'world.txt': 'A',
        })
