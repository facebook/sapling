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


@testcase.eden_repo_test
class DebugGetPathTest:
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.commit('Initial commit.')

    def test_getpath_root_inode(self):
        '''
        Test that calling `eden debug getpath 1` returns the path to the eden
        mount, and indicates that the inode is loaded.
        '''
        output = self.eden.run_cmd(
            'debug',
            'getpath',
            '1',
            cwd=self.mount)

        self.assertEqual('loaded ' + self.mount + '\n', output)

    def test_getpath_dot_eden_inode(self):
        '''
        Test that calling `eden debug getpath 2` returns the path to the .eden
        directory, and indicates that the inode is loaded.
        '''
        output = self.eden.run_cmd(
            'debug',
            'getpath',
            '2',
            cwd=self.mount)

        self.assertEqual(
            'loaded ' + os.path.join(self.mount, '.eden') + '\n',
            output)

    def test_getpath_invalid_inode(self):
        '''
        Test that calling `eden debug getpath 1234` raises an error since
        1234 is not a valid inode number
        '''
        with self.assertRaises(edenclient.EdenCommandError) as context:
            self.eden.run_cmd(
                'debug',
                'getpath',
                '1234',
                cwd=self.mount)
            self.assertIn('unknown inode number 1234',
                          context.exception.stderr.decode())

    def test_getpath_unloaded_inode(self):
        '''
        Test that calling `eden debug getpath` on an unloaded inode returns the
        correct path and indicates that it is unloaded
        '''
        # Create the file
        self.write_file(os.path.join('dir', 'file'), 'blah')
        # Get the inodeNumber
        stat = os.stat(os.path.join(self.mount, 'dir', 'file'))
        # Unload any inodes in directory dir
        self.eden.run_cmd(
            'debug',
            'unload',
            os.path.join(self.mount, 'dir'),
            '0')
        # Get the path for dir/file from its inodeNumber
        output = self.eden.run_cmd(
            'debug',
            'getpath',
            str(stat.st_ino),
            cwd=self.mount)

        self.assertEqual(
            'unloaded ' + os.path.join(self.mount, 'dir', 'file') + '\n',
            output)

    def test_getpath_unloaded_inode_rename_parent(self):
        '''
        Test that when an unloaded inode has one of its parents renamed,
        `eden debug getpath` returns the new path
        '''
        # Create the file
        self.write_file(os.path.join('foo', 'bar', 'test.txt'), 'blah')
        # Get the inodeNumber
        stat = os.stat(os.path.join(self.mount, 'foo', 'bar', 'test.txt'))
        # Unload inodes in foo/bar
        self.eden.run_cmd(
            'debug',
            'unload',
            os.path.join(self.mount, 'foo', 'bar'),
            '0')
        # Rename the foo directory
        os.rename(os.path.join(self.mount, 'foo'),
                  os.path.join(self.mount, 'newname'))
        # Get the new path for the file from its inodeNumber
        output = self.eden.run_cmd(
            'debug',
            'getpath',
            str(stat.st_ino),
            cwd=self.mount)

        self.assertEqual(
            'unloaded ' +
            os.path.join(self.mount, 'newname', 'bar', 'test.txt') + '\n',
            output)

    def test_getpath_unlinked_inode(self):
        '''
        Test that when an inode is unlinked, `eden debug getpath` indicates
        that it is unlinked
        '''
        # Create the file
        self.write_file(os.path.join('foo', 'bar', 'test.txt'), 'blah')
        # Keep an open file handle so that the inode doesn't become invalid
        f = open(os.path.join(self.mount, 'foo', 'bar', 'test.txt'))
        # Get the inodeNumber
        stat = os.stat(os.path.join(self.mount, 'foo', 'bar', 'test.txt'))
        # Unlink the file
        os.unlink(os.path.join(self.mount, 'foo', 'bar', 'test.txt'))
        output = self.eden.run_cmd(
            'debug',
            'getpath',
            str(stat.st_ino),
            cwd=self.mount)
        # Close the file handle
        f.close()

        self.assertEqual('loaded unlinked\n', output)
