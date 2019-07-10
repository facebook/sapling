#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import os
import stat
from typing import Dict

from facebook.eden import EdenService
from facebook.eden.ttypes import FileInformationOrError

from .lib import testcase


@testcase.eden_repo_test
class MaterializedQueryTest(testcase.EdenRepoTest):
    """Check that materialization is represented correctly."""

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        self.repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        self.repo.symlink("slink", "hello")
        self.repo.commit("Initial commit.")

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden.fs.fuse.RequestData": "DBG5"}

    def setUp(self) -> None:
        super().setUp()
        self.client = self.get_thrift_client()
        self.client.open()
        self.addCleanup(self.client.close)

    def test_noEntries(self) -> None:
        pos = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.assertNotEqual(0, pos.mountGeneration)

        changed = self.client.getFilesChangedSince(self.mount_path_bytes, pos)
        self.assertEqual(set(), set(changed.changedPaths))
        self.assertEqual(set(), set(changed.createdPaths))
        self.assertEqual(set(), set(changed.removedPaths))
        self.assertEqual(pos, changed.fromPosition)
        self.assertEqual(pos, changed.toPosition)

    def test_getFileInformation(self) -> None:
        """ verify that getFileInformation is consistent with the VFS """

        paths = [
            b"",
            b"not-exist",
            b"hello",
            b"adir",
            b"adir/file",
            b"bdir/test.sh",
            b"slink",
        ]
        info_list = self.client.getFileInformation(self.mount_path_bytes, paths)
        self.assertEqual(len(paths), len(info_list))

        for idx, path in enumerate(paths):
            try:
                st = os.lstat(os.path.join(self.mount, os.fsdecode(path)))
                self.assertEqual(
                    FileInformationOrError.INFO,
                    info_list[idx].getType(),
                    msg="have non-error result for " + repr(path),
                )
                info = info_list[idx].get_info()
                self.assertEqual(
                    st.st_mode, info.mode, msg="mode matches for " + repr(path)
                )
                self.assertEqual(
                    st.st_size, info.size, msg="size matches for " + repr(path)
                )
                self.assertEqual(int(st.st_mtime), info.mtime.seconds)
                if not stat.S_ISDIR(st.st_mode):
                    self.assertNotEqual(0, st.st_mtime)
                    self.assertNotEqual(0, st.st_ctime)
                    self.assertNotEqual(0, st.st_atime)
            except OSError as e:
                self.assertEqual(
                    FileInformationOrError.ERROR,
                    info_list[idx].getType(),
                    msg="have error result for " + repr(path),
                )
                err = info_list[idx].get_error()
                self.assertEqual(
                    e.errno, err.errorCode, msg="error code matches for " + repr(path)
                )

    def test_invalidProcessGeneration(self) -> None:
        # Get a candidate position
        pos = self.client.getCurrentJournalPosition(self.mount_path_bytes)

        # poke the generation to a value that will never manifest in practice
        pos.mountGeneration = 0

        with self.assertRaises(EdenService.EdenError) as context:
            self.client.getFilesChangedSince(self.mount_path_bytes, pos)
        self.assertEqual(
            errno.ERANGE, context.exception.errorCode, msg="Must return ERANGE"
        )

    def test_removeFile(self) -> None:
        initial_pos = self.client.getCurrentJournalPosition(self.mount_path_bytes)

        os.unlink(os.path.join(self.mount, "adir", "file"))
        changed = self.client.getFilesChangedSince(self.mount_path_bytes, initial_pos)
        self.assertEqual(set(), set(changed.createdPaths))
        self.assertEqual({b"adir/file"}, set(changed.changedPaths))
        self.assertEqual(set(), set(changed.removedPaths))

    def test_renameFile(self) -> None:
        initial_pos = self.client.getCurrentJournalPosition(self.mount_path_bytes)

        os.rename(os.path.join(self.mount, "hello"), os.path.join(self.mount, "bye"))
        changed = self.client.getFilesChangedSince(self.mount_path_bytes, initial_pos)
        self.assertEqual({b"bye"}, set(changed.createdPaths))
        self.assertEqual({b"hello"}, set(changed.changedPaths))
        self.assertEqual(set(), set(changed.removedPaths))

    def test_addFile(self) -> None:
        initial_pos = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        # Record the initial journal position after we finish setting up the checkout.
        initial_seq = initial_pos.sequenceNumber

        name = os.path.join(self.mount, "overlaid")
        with open(name, "w+") as f:
            pos = self.client.getCurrentJournalPosition(self.mount_path_bytes)
            self.assertEqual(
                initial_seq + 1,
                pos.sequenceNumber,
                msg="creating a file bumps the journal",
            )

            changed = self.client.getFilesChangedSince(
                self.mount_path_bytes, initial_pos
            )
            self.assertEqual({b"overlaid"}, set(changed.createdPaths))
            self.assertEqual(set(), set(changed.changedPaths))
            self.assertEqual(set(), set(changed.removedPaths))
            self.assertEqual(
                initial_pos.sequenceNumber + 1,
                changed.fromPosition.sequenceNumber,
                msg="changes start AFTER initial_pos",
            )

            f.write("NAME!\n")

        pos_after_overlaid = self.client.getCurrentJournalPosition(
            self.mount_path_bytes
        )
        self.assertEqual(
            initial_seq + 2,
            pos_after_overlaid.sequenceNumber,
            msg="writing bumps the journal",
        )
        changed = self.client.getFilesChangedSince(self.mount_path_bytes, initial_pos)
        self.assertEqual({b"overlaid"}, set(changed.createdPaths))
        self.assertEqual(set(), set(changed.changedPaths))
        self.assertEqual(set(), set(changed.removedPaths))
        self.assertEqual(
            initial_pos.sequenceNumber + 1,
            changed.fromPosition.sequenceNumber,
            msg="changes start AFTER initial_pos",
        )

        name = os.path.join(self.mount, "adir", "file")
        with open(name, "a") as f:
            pos = self.client.getCurrentJournalPosition(self.mount_path_bytes)
            self.assertEqual(
                initial_seq + 2,
                pos.sequenceNumber,
                msg="journal still in same place for append",
            )
            f.write("more stuff on the end\n")

        pos = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.assertEqual(
            initial_seq + 3, pos.sequenceNumber, msg="appending bumps the journal"
        )

        changed = self.client.getFilesChangedSince(
            self.mount_path_bytes, pos_after_overlaid
        )
        self.assertEqual({b"adir/file"}, set(changed.changedPaths))
        self.assertEqual(set(), set(changed.createdPaths))
        self.assertEqual(set(), set(changed.removedPaths))
        self.assertEqual(
            pos_after_overlaid.sequenceNumber + 1,
            changed.fromPosition.sequenceNumber,
            msg="changes start AFTER pos_after_overlaid",
        )
