#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib import testcase
import errno
import os
import subprocess
import tempfile


# This is the name of the default repository created by EdenRepoTestBase.
repo_name = 'main'


@testcase.eden_repo_test
class CloneTest:
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.commit('Initial commit.')

    def test_clone_to_non_existent_directory(self):
        tmp = self._new_tmp_dir()
        non_existent_dir = os.path.join(tmp, 'foo/bar/baz')

        self.eden.run_cmd('clone', repo_name, non_existent_dir)
        self.assertTrue(os.path.isfile(os.path.join(non_existent_dir, 'hello')),
                        msg='clone should succeed in non-existent directory')

    def test_clone_to_existing_empty_directory(self):
        tmp = self._new_tmp_dir()
        empty_dir = os.path.join(tmp, 'foo/bar/baz')
        os.makedirs(empty_dir)

        self.eden.run_cmd('clone', repo_name, empty_dir)
        self.assertTrue(os.path.isfile(os.path.join(empty_dir, 'hello')),
                        msg='clone should succeed in empty directory')

    def test_clone_to_non_empty_directory_fails(self):
        tmp = self._new_tmp_dir()
        non_empty_dir = os.path.join(tmp, 'foo/bar/baz')
        os.makedirs(non_empty_dir)
        with open(os.path.join(non_empty_dir, 'example.txt'), 'w') as f:
            f.write('I am not empty.\n')

        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden.run_cmd('clone', repo_name, non_empty_dir)
        stderr = context.exception.stderr.decode('utf-8')
        self.assertIn(os.strerror(errno.ENOTEMPTY), stderr,
                      msg='clone into non-empty dir should raise ENOTEMPTY')

    def test_clone_to_file_fails(self):
        tmp = self._new_tmp_dir()
        non_empty_dir = os.path.join(tmp, 'foo/bar/baz')
        os.makedirs(non_empty_dir)
        file_in_directory = os.path.join(non_empty_dir, 'example.txt')
        with open(file_in_directory, 'w') as f:
            f.write('I am not empty.\n')

        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden.run_cmd('clone', repo_name, file_in_directory)
        stderr = context.exception.stderr.decode('utf-8')
        self.assertIn(os.strerror(errno.ENOTDIR), stderr,
                      msg='clone into file should raise ENOTDIR')

    def test_clone_to_non_existent_directory_that_is_under_a_file_fails(self):
        tmp = self._new_tmp_dir()
        non_existent_dir = os.path.join(tmp, 'foo/bar/baz')
        with open(os.path.join(tmp, 'foo'), 'w') as f:
            f.write('I am not empty.\n')

        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden.run_cmd('clone', repo_name, non_existent_dir)
        stderr = context.exception.stderr.decode('utf-8')
        self.assertIn(os.strerror(errno.ENOTDIR), stderr,
                      msg='When the directory cannot be created because the '
                          'ancestor is a parent, clone should raise ENOTDIR')

    def _new_tmp_dir(self):
        return tempfile.mkdtemp(dir=self.tmp_dir)
