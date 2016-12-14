#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import hashlib

from facebook.eden.ttypes import SHA1Result
from .lib import testcase


@testcase.eden_repo_test
class ThriftTest:
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file('adir/file', 'foo!\n')
        self.repo.symlink('slink', 'hello')
        self.repo.commit('Initial commit.')

    def setUp(self):
        super().setUp()
        self.client = self.get_thrift_client()
        self.client.open()

    def tearDown(self):
        self.client.close()
        super().tearDown()

    def test_list_mounts(self):
        mounts = self.client.listMounts()
        self.assertEqual(1, len(mounts))

        mount = mounts[0]
        self.assertEqual(self.mount, mount.mountPoint)
        # Currently, edenClientPath is not set.
        self.assertEqual('', mount.edenClientPath)

    def test_get_sha1(self):
        expected_sha1_for_hello = hashlib.sha1(b'hola\n').digest()
        result_for_hello = SHA1Result()
        result_for_hello.set_sha1(expected_sha1_for_hello)

        expected_sha1_for_adir_file = hashlib.sha1(b'foo!\n').digest()
        result_for_adir_file = SHA1Result()
        result_for_adir_file.set_sha1(expected_sha1_for_adir_file)

        self.assertEqual(
            [
                result_for_hello,
                result_for_adir_file,
            ], self.client.getSHA1(self.mount, ['hello', 'adir/file'])
        )

    def test_get_sha1_throws_for_empty_string(self):
        results = self.client.getSHA1(self.mount, [''])
        self.assertEqual(1, len(results))
        self.assert_error(results[0], 'path cannot be the empty string')

    def test_get_sha1_throws_for_directory(self):
        results = self.client.getSHA1(self.mount, ['adir'])
        self.assertEqual(1, len(results))
        self.assert_error(results[0], 'adir: Is a directory')

    def test_get_sha1_throws_for_non_existent_file(self):
        results = self.client.getSHA1(self.mount, ['i_do_not_exist'])
        self.assertEqual(1, len(results))
        self.assert_error(results[0],
                          'i_do_not_exist: No such file or directory')

    def test_get_sha1_throws_for_symlink(self):
        '''Fails because caller should resolve the symlink themselves.'''
        results = self.client.getSHA1(self.mount, ['slink'])
        self.assertEqual(1, len(results))
        self.assert_error(results[0],
                          'slink: file is a symlink: Invalid argument')

    def assert_error(self, sha1result, error_message):
        self.assertIsNotNone(sha1result, msg='Must pass a SHA1Result')
        self.assertEqual(
            SHA1Result.ERROR,
            sha1result.getType(),
            msg='SHA1Result must be an error'
        )
        error = sha1result.get_error()
        self.assertIsNotNone(error)
        self.assertEqual(error_message, error.message)
