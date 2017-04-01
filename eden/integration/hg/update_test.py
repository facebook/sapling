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
        self.assert_status_empty()

        self.write_file('hello.txt', 'saluton')
        self.assert_status({'hello.txt': 'M'})

        self.repo.update('.', clean=True)
        self.assertEqual('hola', self.read_file('hello.txt'))
        self.assert_status_empty()

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
        self.assert_status_empty()

        self.write_file('foo/subdir/test.log', 'log data')
        self.write_file('foo/_data', 'data file')
        self.assert_status_empty(check_ignored=False,
                                 msg='test.log and _data should be ignored')
        self.assert_status({
            'foo/subdir/test.log': 'I',
            'foo/_data': 'I',
        })

        # Call `hg update` to move from commit2 to commit1, which will
        # change the contents of foo/.gitignore.  This will cause edenfs
        # to send an inode invalidation event to FUSE, but FUSE never knew
        # about this inode in the first place.  edenfs should ignore the
        # resulting ENOENT error in response to the invalidation request.
        self.repo.update(self.commit1)
        self.assert_status({
            'foo/_data': '?',
        }, check_ignored=False)
        self.assert_status({
            'foo/subdir/test.log': 'I',
            'foo/_data': '?',
        })
        self.assertEqual('*.log\n', self.read_file('foo/.gitignore'))

    def test_update_with_new_commits(self):
        '''
        Test running `hg update` to check out commits that were created after
        the edenfs daemon originally started.

        This makes sure edenfs can correctly import new commits that appear in
        the backing store repository.
        '''
        new_contents = 'changed in commit 3\n'
        self.backing_repo.write_file('foo/bar.txt', new_contents)
        new_commit = self.backing_repo.commit('Update foo/bar.txt')

        self.assert_status_empty()

        self.repo.update(new_commit)
        self.assertEqual(new_contents, self.read_file('foo/bar.txt'))
        self.assert_status_empty()
