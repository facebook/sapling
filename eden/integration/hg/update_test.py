#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import HgExtensionTestBase


class UpdateTest(HgExtensionTestBase):
    def populate_backing_repo(self, repo):
        repo.write_file('hello.txt', 'hola')
        repo.commit('Initial commit.')

    def test_update_clean_dot(self):
        '''Test using `hg update --clean .` to revert file modifications.'''
        empty_status = self.status()
        self.assertEqual('', empty_status)

        self.write_file('hello.txt', 'saluton')
        self.assertEqual('M hello.txt\n', self.status())

        self.repo.update('.', clean=True)
        self.assertEqual('hola', self.read_file('hello.txt'))
        self.assertEqual('', self.status())
