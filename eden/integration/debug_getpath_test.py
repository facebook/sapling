#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib import testcase, edenclient
import os

# This is the name of the default repository created by EdenRepoTestBase.
repo_name = 'main'


@testcase.eden_repo_test
class DebugGetPathTest:
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file(os.path.join('dir', 'file'), 'blah\n')
        self.repo.commit('Initial commit.')

    def test_getpath_root_inode(self):
        '''
        Test that calling `eden debug getname 1` returns the path to the eden
        mount, and indicates that the inode is loaded.
        '''
        output = self.eden.run_cmd(
            'debug',
            'getpath',
            self.mount,
            '1')

        self.assertEqual('loaded ' + self.mount + '\n', output)

    def test_getpath_dot_eden_inode(self):
        '''
        Test that calling `eden debug getname 2` returns the path to the .eden
        directory, and indicates that the inode is loaded.
        '''
        output = self.eden.run_cmd(
            'debug',
            'getpath',
            self.mount,
            '2')

        self.assertEqual(
            'loaded ' + os.path.join(self.mount, ".eden") + '\n',
            output)

    def test_getpath_invalid_inode(self):
        '''
        Test that calling `eden debug getname 1234` raises an error since
        1234 is not a valid inode number
        '''
        with self.assertRaises(edenclient.EdenCommandError) as context:
            self.eden.run_cmd(
                'debug',
                'getpath',
                self.mount,
                '1234')
            self.assertIn('unknown inode number 1234', str(context.exception))
