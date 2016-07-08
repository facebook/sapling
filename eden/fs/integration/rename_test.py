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


class RenameTest:
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file('adir/file', 'foo!\n')
        self.repo.symlink('slink', 'hello')
        self.repo.commit('Initial commit.')

    def test_rename_errors(self):
        ''' Test some error cases '''
        with self.assertRaises(OSError) as context:
            os.rename(os.path.join(self.mount, 'not-exist'),
                      os.path.join(self.mount, 'also-not-exist'))
        self.assertEqual(errno.ENOENT, context.exception.errno,
                         msg='Renaming a bogus file -> ENOENT')

        # We don't yet support renaming dirs; check our behavior.
        filename = os.path.join(self.mount, 'adir')
        targetname = os.path.join(self.mount, 'a-new-target')

        with self.assertRaises(OSError) as context:
            os.rename(filename, targetname)
        self.assertEqual(errno.ENOSYS, context.exception.errno,
                         msg='Renaming dirs not supported')

    def test_rename_away_tree_entry(self):
        ''' Rename a tree entry away and back again '''
        # We should be able to rename files that are in the Tree
        hello = os.path.join(self.mount, 'hello')
        targetname = os.path.join(self.mount, 'a-new-target')
        os.rename(hello, targetname)

        with self.assertRaises(OSError) as context:
            os.lstat(hello)
        self.assertEqual(errno.ENOENT, context.exception.errno,
                         msg='no longer visible as old name')

        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['a-new-target', 'adir', 'slink'], entries)

        with open(targetname, 'r') as f:
            self.assertEqual('hola\n', f.read(),
                             msg='materialized correct data')

            # Now, while we hold this file open, check that a rename
            # leaves the handle connected to the file contents when
            # we rename it back to its old name.
            os.rename(targetname, hello)

            entries = sorted(os.listdir(self.mount))
            self.assertEqual(['adir', 'hello', 'slink'], entries)

            with open(hello, 'r+') as write_f:
                write_f.seek(0, os.SEEK_END)
                write_f.write('woot')

            f.seek(0)
            self.assertEqual('hola\nwoot', f.read())

    def test_rename_overlay_only(self):
        ''' Create a local/overlay only file and rename it '''
        # We should be able to rename files that are in the Tree
        from_name = os.path.join(self.mount, 'overlay-a')
        to_name = os.path.join(self.mount, 'overlay-b')

        with open(from_name, 'w') as f:
            f.write('overlay-a\n')

        os.rename(from_name, to_name)

        with self.assertRaises(OSError) as context:
            os.lstat(from_name)
        self.assertEqual(errno.ENOENT, context.exception.errno,
                         msg='no longer visible as old name')

        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['adir', 'hello', 'overlay-b', 'slink'], entries)

        with open(to_name, 'r') as f:
            self.assertEqual('overlay-a\n', f.read(),
                             msg='holds correct data')

            # Now, while we hold this file open, check that a rename
            # leaves the handle connected to the file contents when
            # we rename it back to its old name.
            os.rename(to_name, from_name)

            entries = sorted(os.listdir(self.mount))
            self.assertEqual(['adir', 'hello', 'overlay-a', 'slink'], entries)

            with open(from_name, 'r+') as write_f:
                write_f.seek(0, os.SEEK_END)
                write_f.write('woot')

            f.seek(0)
            self.assertEqual('overlay-a\nwoot', f.read())

    def test_rename_overlay_over_tree(self):
        ''' Make an overlay file and overwrite a tree entry with it '''

        from_name = os.path.join(self.mount, 'overlay-a')
        to_name = os.path.join(self.mount, 'hello')

        with open(from_name, 'w') as f:
            f.write('overlay-a\n')

        os.rename(from_name, to_name)

        with self.assertRaises(OSError) as context:
            os.lstat(from_name)
        self.assertEqual(errno.ENOENT, context.exception.errno,
                         msg='no longer visible as old name')

        entries = sorted(os.listdir(self.mount))
        self.assertEqual(['adir', 'hello', 'slink'], entries)

        with open(to_name, 'r') as f:
            self.assertEqual('overlay-a\n', f.read(),
                             msg='holds correct data')


class RenameTestGit(RenameTest, testcase.EdenGitTest):
    pass


class RenameTestHg(RenameTest, testcase.EdenHgTest):
    pass
