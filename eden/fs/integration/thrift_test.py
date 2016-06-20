#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import hashlib

from facebook.eden.ttypes import EdenError
from .lib import testcase


class ThriftTest(testcase.EdenTestCase):
    def test_list_mounts(self):
        eden = self.init_git_eden()
        client = eden.get_thrift_client()

        mounts = client.listMounts()
        self.assertEqual(1, len(mounts))

        mount = mounts[0]
        self.assertEqual(eden.mount_path, mount.mountPoint)
        # Currently, edenClientPath is not set.
        self.assertEqual('', mount.edenClientPath)

    def test_get_sha1(self):
        eden = self.init_git_eden()
        client = eden.get_thrift_client()

        expected_sha1_for_hello = hashlib.sha1(b'hola\n').digest()
        self.assertEqual(expected_sha1_for_hello,
                         client.getSHA1(eden.mount_path, 'hello'))

        expected_sha1_for_adir_file = hashlib.sha1(b'foo!\n').digest()
        self.assertEqual(expected_sha1_for_adir_file,
                         client.getSHA1(eden.mount_path, 'adir/file'))

    def test_get_sha1_throws_for_empty_string(self):
        eden = self.init_git_eden()
        client = eden.get_thrift_client()

        def try_empty_string_for_path():
            client.getSHA1(eden.mount_path, '')

        self.assertRaisesRegexp(EdenError, 'path cannot be the empty string',
                                try_empty_string_for_path)

    def test_get_sha1_throws_for_directory(self):
        eden = self.init_git_eden()
        client = eden.get_thrift_client()

        def try_directory():
            client.getSHA1(eden.mount_path, 'adir')

        self.assertRaisesRegexp(EdenError,
                                'Found a directory instead of a file: adir',
                                try_directory)

    def test_get_sha1_throws_for_non_existent_file(self):
        eden = self.init_git_eden()
        client = eden.get_thrift_client()

        def try_non_existent_file():
            client.getSHA1(eden.mount_path, 'i_do_not_exist')

        self.assertRaisesRegexp(EdenError,
                                'No such file or directory: i_do_not_exist',
                                try_non_existent_file)

    def test_get_sha1_throws_for_symlink(self):
        '''Fails because caller should resolve the symlink themselves.'''
        eden = self.init_git_eden()
        client = eden.get_thrift_client()

        def try_symlink():
            client.getSHA1(eden.mount_path, 'slink')

        self.assertRaisesRegexp(EdenError, 'Not an ordinary file: slink',
                                try_symlink)
