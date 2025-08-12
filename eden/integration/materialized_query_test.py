#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import errno
import os
import stat
from typing import Dict

from eden.fs.service.eden.thrift_types import (
    EdenError,
    EdenErrorType,
    FileInformation,
    JournalPosition,
    SyncBehavior,
)

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

    async def test_noEntries(self) -> None:
        async with self.get_thrift_client() as client:
            pos = await client.getCurrentJournalPosition(self.mount_path_bytes)
            self.assertNotEqual(0, pos.mountGeneration)

            changed = await client.getFilesChangedSince(self.mount_path_bytes, pos)
            self.assertEqual(set(), set(changed.changedPaths))
            self.assertEqual(set(), set(changed.createdPaths))
            self.assertEqual(set(), set(changed.removedPaths))
            self.assertEqual(pos, changed.fromPosition)
            self.assertEqual(pos, changed.toPosition)

    async def test_getFileInformation(self) -> None:
        """verify that getFileInformation is consistent with the VFS"""

        paths = [
            b"",
            b"not-exist",
            b"hello",
            b"adir",
            b"adir/file",
            b"bdir/test.sh",
            b"slink",
        ]

        async with self.get_thrift_client() as client:
            sync_behavior = SyncBehavior()
            info_list_pre = await client.getFileInformation(
                self.mount_path_bytes, paths, sync_behavior
            )
        self.assertEqual(len(paths), len(info_list_pre))

        # Check that stat doesn't change over the materialization of a file.
        contents = {}
        stats = []
        for path in paths:
            statpath = path if path else b"."
            # skip the non-existent file
            if statpath == b"not-exist":
                continue

            realpath = os.path.join(self.mount, os.fsdecode(statpath))
            st = os.lstat(realpath)
            self.assertNotEqual(0, st.st_mode)
            stats.append(st)

            if stat.S_ISREG(st.st_mode):
                with open(realpath, "a+b") as f:
                    f.seek(0, 0)  # beginning
                    contents[realpath] = f.read()
                    f.seek(0, 2)  # end
                    f.write(b"AppendedData")

            st = os.lstat(realpath)
            self.assertNotEqual(0, st.st_mode)

            # Don't bother checking against the info_list_pre elements, as we assert their equality with
            # info_list_post below, and they are checked against the stat information.
        async with self.get_thrift_client() as client:
            sync_behavior = SyncBehavior()
            info_list_post = await client.getFileInformation(
                self.mount_path_bytes, paths, sync_behavior
            )
        self.assertEqual(len(paths), len(info_list_post))

        # Check the getFileInformation things from before/after load are the same
        statidx = -1
        for path, a, b in zip(paths, info_list_pre, info_list_post):
            self.assertEqual(
                a.get_type(),
                b.get_type(),
                msg="have same pre and post loads for " + repr(path),
            )

            if hasattr(a, "info") and a.info is not None:
                a = a.info
                b = b.info
                statidx += 1
                st = stats[statidx]
                self.assertEqual(
                    a.mode,
                    b.mode,
                    msg="have same FileInformation.mode for " + repr(path),
                )

                # The files were modified above, ensure b.mtime increased
                if os.name == "nt":
                    # mtime is zero on Windows
                    self.assertEqual(
                        a.mtime.seconds,
                        0,
                        msg="have zero mtime on windows for " + repr(path),
                    )
                    self.assertEqual(
                        a.mtime.nanoSeconds,
                        0,
                        msg="have zero mtime on windows for " + repr(path),
                    )
                    self.assertEqual(
                        a.mtime,
                        b.mtime,
                        msg="have same FileInformation.mtime for " + repr(path),
                    )
                elif stat.S_ISREG(st.st_mode):
                    self.assertTrue(
                        (a.mtime.seconds < b.mtime.seconds)
                        or (
                            (a.mtime.seconds == b.mtime.seconds)
                            and (a.mtime.nanoSeconds < b.mtime.nanoSeconds)
                        ),
                        msg=f"have s={a.mtime.seconds},ns={a.mtime.nanoSeconds}< FileInformation.mtime s={b.mtime.seconds},ns={b.mtime.nanoSeconds} after modification for "
                        + repr(path),
                    )
                else:
                    self.assertEqual(
                        a.mtime,
                        b.mtime,
                        msg="have same FileInformation.mtime for " + repr(path),
                    )

                # The second info came from the modified file, adjust the size
                asize = a.size
                if stat.S_ISREG(st.st_mode):
                    asize += 12
                self.assertEqual(
                    asize,
                    b.size,
                    msg="have same FileInformation.size for " + repr(path),
                )
            else:
                a = a.error
                b = b.error
                self.assertEqual(
                    a,
                    b,
                    msg="have same Error for " + repr(path),
                )

        # Reset the repo contents so the length/size checks below are correct
        # TODO: While the below works on HG, it's not portable to the other backing repos this is tested on.
        # self.repo.reset(self.repo.log()[0], keep=False)

        for path, info_or_error in zip(paths, info_list_post):
            try:
                st = os.lstat(
                    os.path.join(self.mount, os.fsdecode(path if path else b"."))
                )

                self.assertTrue(
                    isinstance(info_or_error.info, FileInformation),
                    msg="have non-error result for " + repr(path),
                )
                info = info_or_error.info
                # Windows's FileInode/TreeInode returns zero for mode
                if os.name == "nt":
                    self.assertEqual(
                        0,
                        info.mode,
                        msg="mode is zero on windows for " + repr(path),
                    )
                else:
                    self.assertEqual(
                        f"{st.st_mode:#o}",
                        f"{info.mode:#o}",
                        msg="mode matches for " + repr(path),
                    )
                if os.name == "nt" and stat.S_ISDIR(st.st_mode):
                    # TreeInode assumes directories are zero-size on Windows
                    self.assertEqual(
                        0,
                        info.size,
                        msg="size is zero on windows on directory for " + repr(path),
                    )
                else:
                    self.assertEqual(
                        st.st_size, info.size, msg="size matches for " + repr(path)
                    )
                if os.name == "nt":
                    self.assertEqual(
                        0,
                        info.mtime.seconds,
                        msg="mtime is zero on windows for " + repr(path),
                    )
                else:
                    self.assertEqual(
                        int(st.st_mtime),
                        info.mtime.seconds,
                        msg="mtime matches for " + repr(path),
                    )
                if not stat.S_ISDIR(st.st_mode):
                    self.assertNotEqual(0, st.st_mtime)
                    # pyre-fixme[16]: `stat_result` has no attribute `st_ctime`.
                    self.assertNotEqual(0, st.st_ctime)
                    self.assertNotEqual(0, st.st_atime)
            except OSError as e:
                self.assertTrue(
                    isinstance(info_or_error.error, EdenError),
                    msg="have error result for " + repr(path),
                )
                err = info_or_error.error
                self.assertEqual(
                    e.errno,
                    err.errorCode,
                    msg="error code matches for " + repr(path),
                )

    async def test_invalidProcessGeneration(self) -> None:
        async with self.get_thrift_client() as client:
            # Get a candidate position
            pos = await client.getCurrentJournalPosition(self.mount_path_bytes)

            # poke the generation to a value that will never manifest in practice
            invalid_pos = JournalPosition(
                mountGeneration=0, sequenceNumber=pos.sequenceNumber
            )

            with self.assertRaises(EdenError) as context:
                await client.getFilesChangedSince(self.mount_path_bytes, invalid_pos)
            self.assertEqual(
                errno.ERANGE, context.exception.errorCode, msg="Must return ERANGE"
            )
            self.assertEqual(
                EdenErrorType.MOUNT_GENERATION_CHANGED,
                context.exception.errorType,
            )

    async def test_removeFile(self) -> None:
        async with self.get_thrift_client() as client:
            initial_pos = await client.getCurrentJournalPosition(self.mount_path_bytes)

            os.unlink(os.path.join(self.mount, "adir", "file"))
            changed = await client.getFilesChangedSince(
                self.mount_path_bytes, initial_pos
            )
            self.assertEqual(set(), set(changed.createdPaths))
            self.assertEqual({b"adir/file"}, set(changed.changedPaths))
            self.assertEqual(set(), set(changed.removedPaths))

    async def test_renameFile(self) -> None:
        async with self.get_thrift_client() as client:
            initial_pos = await client.getCurrentJournalPosition(self.mount_path_bytes)

            os.rename(
                os.path.join(self.mount, "hello"), os.path.join(self.mount, "bye")
            )
            changed = await client.getFilesChangedSince(
                self.mount_path_bytes, initial_pos
            )
            self.assertEqual({b"bye"}, set(changed.createdPaths))
            self.assertEqual({b"hello"}, set(changed.changedPaths))
            self.assertEqual(set(), set(changed.removedPaths))

    async def test_addFile(self) -> None:
        async with self.get_thrift_client() as client:
            initial_pos = await client.getCurrentJournalPosition(self.mount_path_bytes)
            # Record the initial journal position after we finish setting up the checkout.
            initial_seq = initial_pos.sequenceNumber

            name = os.path.join(self.mount, "overlaid")
            with open(name, "w+") as f:
                pos = await client.getCurrentJournalPosition(self.mount_path_bytes)
                self.assertEqual(
                    initial_seq + 1,
                    pos.sequenceNumber,
                    msg="creating a file bumps the journal",
                )

                changed = await client.getFilesChangedSince(
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

            pos_after_overlaid = await client.getCurrentJournalPosition(
                self.mount_path_bytes
            )
            self.assertEqual(
                initial_seq + 2,
                pos_after_overlaid.sequenceNumber,
                msg="writing bumps the journal",
            )
            changed = await client.getFilesChangedSince(
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

            name = os.path.join(self.mount, "adir", "file")
            with open(name, "a") as f:
                pos = await client.getCurrentJournalPosition(self.mount_path_bytes)
                self.assertEqual(
                    initial_seq + 2,
                    pos.sequenceNumber,
                    msg="journal still in same place for append",
                )
                f.write("more stuff on the end\n")

            pos = await client.getCurrentJournalPosition(self.mount_path_bytes)
            self.assertEqual(
                initial_seq + 3, pos.sequenceNumber, msg="appending bumps the journal"
            )

            changed = await client.getFilesChangedSince(
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
