#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
from ..lib import hgrepo
from .lib.hg_extension_test_base import EdenHgTestCase, hg_test
from textwrap import dedent


@hg_test
class UpdateTest(EdenHgTestCase):
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
        self.assert_status_empty(
            check_ignored=False, msg='test.log and _data should be ignored'
        )
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
        self.assert_status(
            {
                'foo/_data': '?',
            }, check_ignored=False
        )
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
        self.backing_repo.write_file(
            'new_project/Makefile', 'all:\n\techo done!\n'
        )
        self.backing_repo.write_file('new_project/.gitignore', '*.o\n')
        new_commit = self.backing_repo.commit('Add new_project')

        # Check the status before we update
        self.assert_status(
            {
                'new_project/newcode.o': 'I',
                'new_project/newcode.c': '?',
                'new_project/Makefile': '?',
                'new_project/.gitignore': '?',
            }
        )

        # Now run "hg update -C new_commit"
        self.repo.update(new_commit, clean=True)
        self.assert_status({
            'new_project/newcode.o': 'I',
        })

    def test_update_with_merge_flag_and_conflict(self):
        self.write_file('foo/bar.txt', 'changing yet again\n')
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg('update', '.^', '--merge')
        self.assertIn(
            b'conflicts while merging foo/bar.txt! '
            b'(edit, then use \'hg resolve --mark\')', context.exception.stderr
        )
        self.assert_status({
            'foo/bar.txt': 'M',
        })
        expected_contents = dedent(
            '''\
        <<<<<<< working copy
        changing yet again
        =======
        test
        >>>>>>> destination
        '''
        )
        self.assertEqual(expected_contents, self.read_file('foo/bar.txt'))

    def test_update_with_untracked_file_that_is_tracked_in_destination(self):
        base_commit = self.repo.log()[-1]
        original_contents = 'Original contents.\n'
        self.write_file('some_new_file.txt', original_contents)
        self.hg('add', 'some_new_file.txt')
        commit = self.repo.commit('Commit a new file.')

        # Do an `hg prev` and re-create the new file with different contents.
        self.hg('update', base_commit)
        self.assert_status_empty()
        self.assertFalse(os.path.exists('some_new_file.txt'))
        modified_contents = 'Re-create the file with different contents.\n'
        self.write_file('some_new_file.txt', modified_contents)
        self.assert_status({
            'some_new_file.txt': '?',
        })

        # Verify `hg next` updates such that the original contents and commit
        # hash are restored. No conflicts should be reported.
        path_to_backup = '.hg/origbackups/some_new_file.txt'
        expected_backup_file = os.path.join(self.mount, path_to_backup)
        self.assertFalse(os.path.isfile(expected_backup_file))
        self.hg('update', commit)
        self.assertEqual(commit, self.repo.log()[-1])
        self.assertEqual(original_contents, self.read_file('some_new_file.txt'))
        self.assert_status_empty()

        # Verify the previous version of the file was backed up as expected.
        self.assertTrue(os.path.isfile(expected_backup_file))
        self.assertEqual(modified_contents, self.read_file(path_to_backup))

    def test_update_modified_file_to_removed_file_taking_other(self):
        self.write_file('some_new_file.txt', 'I am new!\n')
        self.hg('add', 'some_new_file.txt')
        self.repo.commit('Commit a new file.')
        self.write_file(
            'some_new_file.txt', 'Make some changes to that new file.\n'
        )

        self.hg('update', '.^', '--merge', '--tool', ':other')
        self.assertFalse(os.path.exists(self.get_path('some_new_file.txt')))
        self.assertFalse(
            os.path.isfile(
                os.path.join(self.mount, '.hg/origbackups/some_new_file.txt')
            ),
            msg='There should not be a backup file because '
            ':other was specified explicitly.'
        )

    def test_update_ignores_untracked_directory(self):
        head = self.repo.log()[-1]
        self.mkdir('foo/bar')
        self.write_file('foo/bar/a.txt', 'File in directory two levels deep.\n')
        self.write_file('foo/bar/b.txt', 'Another file.\n')
        self.hg('add', 'foo/bar/a.txt')
        self.assert_status({
            'foo/bar/a.txt': 'A',
            'foo/bar/b.txt': '?',
        })
        self.repo.commit('Commit only a.txt.')
        self.assert_status({
            'foo/bar/b.txt': '?',
        })
        self.hg('update', head)
        self.assert_status({
            'foo/bar/b.txt': '?',
        })
        self.assertFalse(os.path.exists(self.get_path('foo/bar/a.txt')))
        self.assertTrue(os.path.exists(self.get_path('foo/bar/b.txt')))
