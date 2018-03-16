#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import resource
import sys

from .lib import testcase


@testcase.eden_repo_test
class TakeoverTest:
    def populate_repo(self):
        self.pagesize = resource.getpagesize()
        self.page1 = "1" * self.pagesize
        self.page2 = "2" * self.pagesize
        self.repo.write_file('tree/hello', self.page1 + self.page2)
        self.repo.write_file('tree/deleted', self.page1 + self.page2)
        self.repo.commit('Initial commit.')

    def select_storage_engine(self):
        ''' we need to persist data across restarts '''
        return 'sqlite'

    def edenfs_logging_settings(self):
        return {'eden.strace': 'DBG7', 'eden.fs.fuse': 'DBG7'}

    def test_takeover(self):
        hello = os.path.join(self.mount, 'tree/hello')
        deleted = os.path.join(self.mount, 'tree/deleted')
        deleted_local = os.path.join(self.mount, 'deleted-local')

        # To test our handling of unlinked inodes, in addition
        # to unlinking something that is in the manifest we
        # need to check that we handle the case of a local
        # file being deleted to make sure that we cover both
        # code paths for FileInode.
        with open(deleted_local, 'w') as dl:
            dl.write(self.page1)
            dl.write(self.page2)

        # We'd like to make sure that we do something reasonable
        # for directories that have been unlinked and that are
        # still referenced via a file descriptor.  Ideally we'd call
        # opendir() here and then readdir() it after we've performed
        # the graceful restart, but we can't directly call those
        # functions from python.  The approach used here is to
        # open a file descriptor to the directory and then try
        # to stat() it after the restart.  Since the directory
        # has to be empty in order to be unlinked, a readdir
        # from it wouldn't return any interesting results anyway.
        deleted_dir = os.path.join(self.mount, 'deleted-dir')
        os.mkdir(deleted_dir)
        deleted_dir_fd = os.open(deleted_dir, 0)
        os.rmdir(deleted_dir)

        with open(hello, 'r') as f, \
             open(deleted, 'r') as d, \
             open(deleted_local, 'r') as dl:
            # Read the first page only (rather than the whole file)
            # before we restart the process.
            # This is so that we can check that the kernel really
            # does call in to us for the second page and that we're
            # really servicing the read for the second page and that
            # it isn't just getting served from the kernel buffer cache
            self.assertEqual(self.page1, f.read(self.pagesize))

            # Let's make sure that unlinked inodes continue to
            # work appropriately too.  We've opened the file
            # handles and are holding them alive in `d` and `dl`,
            # so now let's unlink it from the filesystem
            os.unlink(deleted)
            os.unlink(deleted_local)

            print('=== beginning restart ===', file=sys.stderr)
            self.eden.graceful_restart()
            print('=== restart complete ===', file=sys.stderr)

            # Ensure that our file handle is still live across
            # the restart boundary
            f.seek(0)
            self.assertEqual(self.page1, f.read(self.pagesize))
            self.assertEqual(self.page2, f.read(self.pagesize))

            # We should be able to read from the `d` file handle
            # even though we deleted the file from the tree
            self.assertEqual(self.page1, d.read(self.pagesize))
            self.assertEqual(self.page2, d.read(self.pagesize))
            # Likewise for the `dl` file handle
            self.assertEqual(self.page1, dl.read(self.pagesize))
            self.assertEqual(self.page2, dl.read(self.pagesize))

        # Now check that the unlinked directory handle still seems
        # connected.  This is difficult to do directly in python;
        # the directory had to be empty in order to be removed
        # so even if we could read its directory entries there
        # wouldn't be anything to read.
        # Note that os.stat() will throw if the fd is deemed
        # bad either by the kernel or the eden instance,
        # so we're just calling it and discarding the return
        # value.
        os.stat(deleted_dir_fd)
        os.close(deleted_dir_fd)

        # Let's also test opening the same file up again,
        # just to make sure that that is still working after
        # the graceful restart.
        with open(hello, 'r') as f:
            self.assertEqual(self.page1, f.read(self.pagesize))
            self.assertEqual(self.page2, f.read(self.pagesize))

    def test_takeover_preserves_inode_numbers_for_open_nonmaterialized_files(self):
        hello = os.path.join(self.mount, 'tree/hello')

        fd = os.open(hello, os.O_RDONLY)
        try:
            inode_number = os.fstat(fd).st_ino

            self.eden.graceful_restart()

            self.assertEqual(inode_number, os.fstat(fd).st_ino)
        finally:
            os.close(fd)

        fd = os.open(hello, os.O_RDONLY)
        try:
            self.assertEqual(inode_number, os.fstat(fd).st_ino)
        finally:
            os.close(fd)
