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
        repo.write_file('.gitignore', 'ignoreme\n')
        repo.write_file('foo/.gitignore', '*.log\n')
        repo.write_file('foo/bar.txt', 'test\n')
        repo.write_file('foo/subdir/test.txt', 'test\n')
        self.commit1 = repo.commit('Initial commit.')

        repo.write_file('foo/.gitignore', '*.log\n/_*\n')
        self.commit2 = repo.commit('Update foo/.gitignore')

    def test_update_clean_dot(self):
        '''Test using `hg update --clean .` to revert file modifications.'''
        self.assertEqual('', self.status())

        self.write_file('hello.txt', 'saluton')
        self.assertEqual('M hello.txt\n', self.status())

        self.repo.update('.', clean=True)
        self.assertEqual('hola', self.read_file('hello.txt'))
        self.assertEqual('', self.status())

    def test_update_with_gitignores(self):
        '''
        Test `hg update` with gitignore files.

        This exercises the normal checkout and ignore logic, but also exercises
        some additional interesting cases:  The `hg status` calls cause eden to
        create FileInode objects for the .gitignore files, even though they
        have never been requested via FUSE APIs.  When we update them via
        checkout, this triggers FUSE inode invalidation events.  We want to
        make sure the invalidation doesn't cause any errors even though the
        kernel didn't previously know that these inode objects existed.
        '''
        # Call `hg status`, which causes eden to internally create FileInode
        # objects for the .gitignore files.
        self.assertEqual('', self.status())

        self.write_file('foo/subdir/test.log', 'log data')
        self.write_file('foo/_data', 'data file')
        self.assertEqual('', self.status(),
                         msg='test.log and _data should be ignored')

        # Call `hg update` to move from commit2 to commit1, which will
        # change the contents of foo/.gitignore.  This will cause edenfs
        # to send an inode invalidation event to FUSE, but FUSE never knew
        # about this inode in the first place.  edenfs should ignore the
        # resulting ENOENT error in response to the invalidation request.
        self.repo.update(self.commit1)
        self.assertEqual('? foo/_data\n', self.status(),
                         msg='now only test.log should be ignored')
        self.assertEqual('*.log\n', self.read_file('foo/.gitignore'))
