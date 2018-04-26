#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from facebook.eden.ttypes import EdenError
from .lib import testcase


@testcase.eden_repo_test
class GlobTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file('README', 'docs\n')
        self.repo.write_file('adir/file', 'foo!\n')
        self.repo.write_file('bdir/file', 'bar!\n')
        self.repo.symlink('slink', 'hello')
        self.commit1 = self.repo.commit('Initial commit.')

        self.repo.write_file('bdir/file', 'bar?\n')
        self.repo.write_file('cdir/subdir/new.txt', 'and improved')
        self.repo.remove_file('README')
        self.commit2 = self.repo.commit('Commit 2.')

    def setUp(self) -> None:
        super().setUp()
        self.client = self.get_thrift_client()
        self.client.open()
        self.addCleanup(self.client.close)

    def test_glob(self) -> None:
        self.assertEqual(
            ['adir/file'], self.client.glob(self.mount, ['a*/file'])
        )
        self.assertCountEqual(
            ['adir/file', 'bdir/file'],
            self.client.glob(self.mount, ['**/file'])
        )
        self.assertEqual(
            ['adir/file'], self.client.glob(self.mount, ['adir/*'])
        )
        self.assertCountEqual(
            ['adir/file', 'bdir/file'],
            self.client.glob(self.mount, ['adir/*', '**/file']),
            msg='De-duplicate results from multiple globs'
        )
        self.assertEqual(['hello'], self.client.glob(self.mount, ['hello']))
        self.assertEqual(
            [],
            self.client.glob(self.mount, ['hell']),
            msg="No accidental substring match"
        )
        self.assertEqual(['hello'], self.client.glob(self.mount, ['hel*']))
        self.assertEqual(['adir'], self.client.glob(self.mount, ['ad*']))
        self.assertEqual(
            ['adir/file'], self.client.glob(self.mount, ['adir/**/*'])
        )
        self.assertEqual(
            ['adir/file'], self.client.glob(self.mount, ['adir/**'])
        )

        with self.assertRaises(EdenError) as ctx:
            self.client.glob(self.mount, ['adir['])
        self.assertIn('unterminated bracket sequence', str(ctx.exception))
