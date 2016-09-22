#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
import os
import stat
from .lib import testcase
from facebook.eden import EdenService
from facebook.eden.ttypes import FileInformationOrError

@testcase.eden_repo_test
class MaterializedQueryTest:
    '''Check that materialization is represented correctly.'''

    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file('adir/file', 'foo!\n')
        self.repo.write_file('bdir/test.sh', '#!/bin/bash\necho test\n',
                             mode=0o755)
        self.repo.write_file('bdir/noexec.sh', '#!/bin/bash\necho test\n')
        self.repo.symlink('slink', 'hello')
        self.repo.commit('Initial commit.')

    def setUp(self):
        super().setUp()
        self.client = self.get_thrift_client()
        self.client.open()

    def tearDown(self):
        self.client.close()
        super().tearDown()

    def test_noEntries(self):
        items = self.client.getMaterializedEntries(self.mount)
        self.assertEqual({}, items.fileInfo)

        pos = self.client.getCurrentJournalPosition(self.mount)
        self.assertEqual(1, pos.sequenceNumber)
        self.assertNotEqual(0, pos.mountGeneration)

        changed = self.client.getFilesChangedSince(self.mount, pos)
        self.assertEqual(0, len(changed.paths))
        self.assertEqual(pos, changed.fromPosition)
        self.assertEqual(pos, changed.toPosition)

    def test_getFileInformation(self):
        """ verify that getFileInformation is consistent with the VFS """

        paths = ['', 'not-exist', 'hello', 'adir',
                 'adir/file', 'bdir/test.sh', 'slink']
        info_list = self.client.getFileInformation(self.mount, paths)
        self.assertEqual(len(paths), len(info_list))

        for idx, path in enumerate(paths):
            try:
                st = os.lstat(os.path.join(self.mount, path))
                self.assertEqual(FileInformationOrError.INFO, info_list[
                                 idx].getType(),
                                 msg='have non-error result for ' + path)
                info = info_list[idx].get_info()
                self.assertEqual(st.st_mode, info.mode,
                                 msg='mode matches for ' + path)
                self.assertEqual(st.st_size, info.size,
                                 msg='size matches for ' + path)
                self.assertEqual(st.st_mtime, info.mtime.seconds)
            except OSError as e:
                self.assertEqual(FileInformationOrError.ERROR, info_list[
                                 idx].getType(),
                                 msg='have error result for ' + path)
                err = info_list[idx].get_error()
                self.assertEqual(e.errno, err.errorCode,
                                 msg='error code matches for ' + path)

    def test_invalidProcessGeneration(self):
        # Get a candidate position
        pos = self.client.getCurrentJournalPosition(self.mount)

        # poke the generation to a value that will never manifest in practice
        pos.mountGeneration = 0

        with self.assertRaises(EdenService.EdenError) as context:
            self.client.getFilesChangedSince(self.mount, pos)
        self.assertEqual(errno.ERANGE, context.exception.errorCode,
                         msg='Must return ERANGE')

    def test_addFile(self):
        initial_pos = self.client.getCurrentJournalPosition(self.mount)
        self.assertEqual(1, initial_pos.sequenceNumber)

        name = os.path.join(self.mount, 'overlaid')
        with open(name, 'w+') as f:
            pos = self.client.getCurrentJournalPosition(self.mount)
            self.assertEqual(2, pos.sequenceNumber,
                             msg='creating a file bumps the journal')

            changed = self.client.getFilesChangedSince(self.mount, initial_pos)
            self.assertEqual(['overlaid'], changed.paths)
            self.assertEqual(initial_pos.sequenceNumber + 1,
                             changed.fromPosition.sequenceNumber,
                             msg='changes start AFTER initial_pos')

            f.write('NAME!\n')

        pos_after_overlaid = self.client.getCurrentJournalPosition(self.mount)
        self.assertEqual(3, pos_after_overlaid.sequenceNumber,
                         msg='writing bumps the journal')
        changed = self.client.getFilesChangedSince(self.mount, initial_pos)
        self.assertEqual(['overlaid'], changed.paths)
        self.assertEqual(initial_pos.sequenceNumber + 1,
                         changed.fromPosition.sequenceNumber,
                         msg='changes start AFTER initial_pos')

        info = self.client.getMaterializedEntries(self.mount)
        self.assertEqual(pos_after_overlaid, info.currentPosition,
                         msg='consistent with getCurrentJournalPosition')

        items = info.fileInfo
        self.assertEqual(2, len(items))

        self.assertTrue(stat.S_ISDIR(items[''].mode))

        self.assertTrue(stat.S_ISREG(items['overlaid'].mode))
        self.assertEqual(6, items['overlaid'].size)
        self.assertNotEqual(0, items['overlaid'].mtime.seconds)

        name = os.path.join(self.mount, 'adir', 'file')
        with open(name, 'a') as f:
            pos = self.client.getCurrentJournalPosition(self.mount)
            self.assertEqual(3, pos.sequenceNumber,
                             msg='journal still in same place for append')
            f.write('more stuff on the end\n')

        pos = self.client.getCurrentJournalPosition(self.mount)
        self.assertEqual(4, pos.sequenceNumber,
                         msg='appending bumps the journal')

        changed = self.client.getFilesChangedSince(
            self.mount, pos_after_overlaid)
        self.assertEqual(['adir/file'], changed.paths)
        self.assertEqual(pos_after_overlaid.sequenceNumber + 1,
                         changed.fromPosition.sequenceNumber,
                         msg='changes start AFTER pos_after_overlaid')

        info = self.client.getMaterializedEntries(self.mount)
        self.assertEqual(pos, info.currentPosition,
                         msg='consistent with getCurrentJournalPosition')
        items = info.fileInfo
        self.assertEqual(4, len(items))

        self.assertTrue(stat.S_ISDIR(items[''].mode))

        self.assertTrue(stat.S_ISREG(items['overlaid'].mode))
        self.assertEqual(6, items['overlaid'].size)
        self.assertNotEqual(0, items['overlaid'].mtime.seconds)

    def test_rename_overlay_dir(self):
        srcname = os.path.join(self.mount, 'overlay-1')
        targetname = os.path.join(self.mount, 'overlay-2')
        os.mkdir(srcname)

        info = self.client.getMaterializedEntries(self.mount)
        self.assertEqual(['', 'overlay-1'], sorted(info.fileInfo.keys()))

        os.rename(srcname, targetname)

        info = self.client.getMaterializedEntries(self.mount)
        self.assertEqual(['', 'overlay-2'], sorted(info.fileInfo.keys()))
