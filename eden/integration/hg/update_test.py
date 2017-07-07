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
    def edenfs_logging_settings(self):
        return {
            'eden.fs.inodes.TreeInode': 'DBG5',
            'eden.fs.inodes.CheckoutAction': 'DBG5',
        }

    def populate_backing_repo(self, repo):
        repo.write_file('hello.txt', 'hola')
        repo.write_file('.gitignore', 'ignoreme\n')
        repo.write_file('foo/.gitignore', '*.log\n')
        repo.write_file('foo/bar.txt', 'test\n')
        repo.write_file('foo/subdir/test.txt', 'test\n')
        self.commit1 = repo.commit('Initial commit.')

        repo.write_file('foo/.gitignore', '*.log\n/_*\n')
        self.commit2 = repo.commit('Update foo/.gitignore')

        repo.write_file('foo/bar.txt', 'updated in commit 3\n')
        self.commit3 = repo.commit('Update foo/.gitignore')

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
        self.assertEqual('test\n', self.read_file('foo/bar.txt'))

    def test_update_with_new_commits(self):
        '''
        Test running `hg update` to check out commits that were created after
        the edenfs daemon originally started.

        This makes sure edenfs can correctly import new commits that appear in
        the backing store repository.
        '''
        new_contents = 'New contents for bar.txt\n'
        self.backing_repo.write_file('foo/bar.txt', new_contents)
        new_commit = self.backing_repo.commit('Update foo/bar.txt')

        self.assert_status_empty()
        self.assertNotEqual(new_contents, self.read_file('foo/bar.txt'))

        self.repo.update(new_commit)
        self.assertEqual(new_contents, self.read_file('foo/bar.txt'))
        self.assert_status_empty()

    def test_reset(self):
        '''
        Test `hg reset`
        '''
        self.assert_status_empty()
        self.assertEqual('updated in commit 3\n', self.read_file('foo/bar.txt'))

        self.repo.reset(self.commit2, keep=True)
        self.assert_status({'foo/bar.txt': 'M'})
        self.assertEqual('updated in commit 3\n', self.read_file('foo/bar.txt'))

        self.repo.update(self.commit2, clean=True)
        self.assert_status_empty()
        self.assertEqual('test\n', self.read_file('foo/bar.txt'))

    def test_update_replace_untracked_dir(self):
        '''
        Create a local untracked directory, then run "hg update -C" to
        checkout a commit where this directory exists in source control.
        '''
        self.assert_status_empty()
        # Write some new files in the eden working directory
        self.mkdir('new_project')
        self.write_file('new_project/newcode.c', 'test\n')
        self.write_file('new_project/Makefile', 'all:\n\techo done!\n')
        self.write_file('new_project/.gitignore', '*.o\n')
        self.write_file('new_project/newcode.o', '\x00\x01\x02\x03\x04')

        # Add the same files to a commit in the backing repository
        self.backing_repo.write_file('new_project/newcode.c', 'test\n')
        self.backing_repo.write_file('new_project/Makefile',
                                     'all:\n\techo done!\n')
        self.backing_repo.write_file('new_project/.gitignore', '*.o\n')
        new_commit = self.backing_repo.commit('Add new_project')

        # Check the status before we update
        self.assert_status({
            'new_project/newcode.o': 'I',
            'new_project/newcode.c': '?',
            'new_project/Makefile': '?',
            'new_project/.gitignore': '?',
        })

        # Now run "hg update -C new_commit"
        self.repo.update(new_commit, clean=True)
        self.assert_status({
            'new_project/newcode.o': 'I',
        })
