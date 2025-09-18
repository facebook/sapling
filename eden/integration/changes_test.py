#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import errno
import os
import re
import sys

from eden.fs.service.eden.thrift_types import (
    Added,
    CommitTransition,
    DirectoryRenamed,
    Dtype,
    EdenError,
    EdenErrorType,
    LostChanges,
    LostChangesReason,
    Modified,
    Removed,
    Renamed,
    Replaced,
    StateEntered,
)

from .lib import testcase
from .lib.journal_test_base import JournalTestBase, WindowsJournalTestBase
from .lib.thrift_objects import (
    buildLargeChange,
    buildSmallChange,
    buildStateChange,
    getLargeChangeSafe,
)

if sys.platform == "win32":
    testBase = WindowsJournalTestBase
else:
    testBase = JournalTestBase


@testcase.eden_repo_test
class ChangesTestCommon(testBase):
    async def test_wrong_mount_generation(self):
        # The input mount generation should equal the current mount generation
        async with self.get_thrift_client() as client:
            oldPosition = await client.getCurrentJournalPosition(self.mount_path_bytes)
            self.eden.unmount(self.mount_path)
            self.eden.mount(self.mount_path)
            changes = await self.getChangesSinceV2(oldPosition)
            self.assertEqual(len(changes.changes), 1)
            largeChange = getLargeChangeSafe(changes.changes[0])
            self.assertIsNotNone(largeChange)
            self.assertEqual(
                largeChange.lostChanges.reason,
                LostChangesReason.EDENFS_REMOUNTED,
            )

    async def test_exclude_directory(self):
        expected_changes = []
        async with self.get_thrift_client() as client:
            oldPosition = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.add_folder_expect("ignored_dir")
            await self.add_folder_expect("ignored_dir2/nested_ignored_dir")
            expected_changes += await self.add_folder_expect("want_dir")
            # same name in subdir should not be ignored
            expected_changes += await self.add_folder_expect("want_dir/ignored_dir")
            await self.add_file_expect("ignored_dir/test_file", "contents", add=False)
            expected_changes += await self.add_file_expect(
                "want_dir/test_file", "contents", add=False
            )
            await self.add_file_expect(
                "ignored_dir2/nested_ignored_dir/test_file", "contents", add=False
            )
            expected_changes += await self.add_file_expect(
                "want_dir/ignored_dir/test_file", "contents", add=False
            )
            changes = await self.getChangesSinceV2(
                oldPosition,
                excluded_roots=["ignored_dir", "ignored_dir2/nested_ignored_dir"],
            )
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_include_directory(self):
        expected_changes = []
        async with self.get_thrift_client() as client:
            oldPosition = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("ignored_dir")
            await self.mkdir_async("ignored_dir2/nested_ignored_dir")
            expected_changes += await self.add_folder_expect("want_dir")
            expected_changes += await self.add_folder_expect("want_dir/ignored_dir")
            await self.add_file_expect("ignored_dir/test_file", "contents", add=False)
            expected_changes += await self.add_file_expect(
                "want_dir/test_file", "contents", add=False
            )
            await self.add_file_expect(
                "ignored_dir2/nested_ignored_dir/test_file", "contents", add=False
            )
            expected_changes += await self.add_file_expect(
                "want_dir/ignored_dir/test_file", "contents", add=False
            )
            changes = await self.getChangesSinceV2(
                oldPosition,
                included_roots=["want_dir"],
            )
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_include_exclude_directory(self):
        # if directory is both included and excluded, it should be ignored
        async with self.get_thrift_client() as client:
            oldPosition = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("include_exclude_dir")
            await self.mkdir_async("ignored_dir")
            await self.add_file_expect("ignored_dir/test_file", "contents", add=False)
            await self.add_file_expect(
                "include_exclude_dir/test_file", "contents", add=False
            )
            changes = await self.getChangesSinceV2(
                oldPosition,
                included_roots=["include_exclude_dir"],
                excluded_roots=["include_exclude_dir"],
            )
            self.assertEqual(changes.changes, [])

    async def test_include_file_suffix(self):
        # use removed files for cross-os compatibility
        await self.repo_write_file("test_file.ext1", "", add=False)
        await self.repo_write_file("test_file.ext2", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rm_async("test_file.ext1")
            await self.rm_async("test_file.ext2")
            changes = await self.getChangesSinceV2(
                position=position, included_suffixes=[".ext1"]
            )
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.REGULAR,
                    path=b"test_file.ext1",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_exclude_file_suffix(self):
        await self.repo_write_file("test_file.ext1", "", add=False)
        await self.repo_write_file("test_file.ext2", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rm_async("test_file.ext1")
            await self.rm_async("test_file.ext2")
            changes = await self.getChangesSinceV2(
                position=position, excluded_suffixes=[".ext1"]
            )
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.REGULAR,
                    path=b"test_file.ext2",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_incude_exclude_file_same_suffix(self):
        await self.repo_write_file("test_file.ext1", "", add=False)
        await self.repo_write_file("test_file.ext2", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rm_async("test_file.ext1")
            await self.rm_async("test_file.ext2")
            changes = await self.getChangesSinceV2(
                position=position,
                included_suffixes=[".ext1"],
                excluded_suffixes=[".ext1"],
            )
            self.assertEqual(changes.changes, [])

    async def test_incude_exclude_file_suffix(self):
        await self.repo_write_file("test_file.ext1", "", add=False)
        await self.repo_write_file("test_file.ext2", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rm_async("test_file.ext1")
            await self.rm_async("test_file.ext2")
            changes = await self.getChangesSinceV2(
                position=position,
                included_suffixes=[".ext1"],
                excluded_suffixes=[".ext2"],
            )
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.REGULAR,
                    path=b"test_file.ext1",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_modify_file(self):
        await self.repo_write_file("test_file", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("test_file", "contents", add=False)
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(Modified, Dtype.REGULAR, path=b"test_file"),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_modify_file_root(self):
        await self.repo_write_file("root/test_file", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "contents", add=False)
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(Modified, Dtype.REGULAR, path=b"test_file"),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_remove_file(self):
        await self.repo_write_file("test_file", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rm_async("test_file")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.REGULAR,
                    path=b"test_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_remove_file_root(self):
        await self.repo_write_file("root/test_file", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rm_async("root/test_file")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.REGULAR,
                    path=b"test_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_add_folder(self):
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("test_folder")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"test_folder",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_add_folder_root(self):
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("root/test_folder")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"test_folder",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_remove_folder(self):
        await self.mkdir_async("test_folder")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_rmdir("test_folder")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.DIR,
                    path=b"test_folder",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_remove_folder_root(self):
        await self.mkdir_async("root/test_folder/")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_rmdir("root/test_folder")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.DIR,
                    path=b"test_folder",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_commit_transition(self):
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("test_folder")
            await self.repo_write_file("test_folder/test_file", "contents", add=True)
            commit1 = self.eden_repo.commit("commit 1")
            changes1 = await self.getChangesSinceV2(position=position)
            expected_changes1 = [
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"test_folder",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"test_folder/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"test_folder/test_file",
                ),
                # For CommitTransition, from_bytes is the current hash and to_bytes is the previous hash
                buildLargeChange(
                    CommitTransition,
                    from_bytes=bytes.fromhex(self.commit0),
                    to_bytes=bytes.fromhex(commit1),
                ),
            ]
            self.assertTrue(self.check_changes(changes1.changes, expected_changes1))

            self.eden_repo.hg("goto", self.commit0)
            changes2 = await self.getChangesSinceV2(position=changes1.toPosition)
            expected_changes2 = [
                buildLargeChange(
                    CommitTransition,
                    from_bytes=bytes.fromhex(commit1),
                    to_bytes=bytes.fromhex(self.commit0),
                ),
            ]
            self.assertTrue(self.check_changes(changes2.changes, expected_changes2))
            # Check that the file was removed when going down a commit
            self.assertFalse(os.path.exists(self.get_path("/test_folder/test_file")))

    async def test_truncated_journal(self):
        # Tests that when the journal has been truncated, we get a lost changes notification
        # We expect the following
        # Changes before the truncation are reported normally when there is no truncation
        # When there is a truncation in between the start position and the current position,
        #   we only report that there has been a truncated journal. Neither changes before and
        #   within the window are reported.
        # Changes after the truncation are reported when the start position is after the truncation
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("not_seen_folder")
            await self.repo_write_file(
                "not_seen_folder/not_seen_file", "missing", add=True
            )
            changes0 = await self.getChangesSinceV2(position=position)
            self.eden.run_cmd("debug", "flush_journal", self.mount_path)
            position2 = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("test_folder")
            await self.repo_write_file("test_folder/test_file", "contents", add=True)
            changes = await self.getChangesSinceV2(position=position)
            changes2 = await self.getChangesSinceV2(position=changes.toPosition)
            changes3 = await self.getChangesSinceV2(position=position2)
            expected_changes0 = [
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"not_seen_folder",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"not_seen_folder/not_seen_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"not_seen_folder/not_seen_file",
                ),
            ]
            expected_changes = [
                buildLargeChange(
                    LostChanges,
                    lost_change_reason=LostChangesReason.JOURNAL_TRUNCATED,
                ),
            ]
            expected_changes3 = [
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"test_folder",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"test_folder/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"test_folder/test_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes0.changes, expected_changes0))
            self.assertTrue(self.check_changes(changes.changes, expected_changes))
            self.assertEqual(changes2.changes, [])
            self.assertTrue(self.check_changes(changes3.changes, expected_changes3))

    async def test_root_does_not_exist(self):
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            with self.assertRaises(EdenError) as ctx:
                await self.getChangesSinceV2(
                    position=position, root=b"this_path_does_not_exist"
                )

            mount_string = self.mount_path
            self.assertRegex(
                ctx.exception.message,
                'Invalid root path "this_path_does_not_exist" in mount .*'
                + re.escape(str(mount_string)),
            )
            self.assertEqual(ctx.exception.errorCode, errno.EINVAL)
            self.assertEqual(
                ctx.exception.errorType,
                EdenErrorType.ARGUMENT_ERROR,
            )

    async def test_root_exists(self):
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("test_folder")
            await self.repo_write_file("test_folder/test_file", "contents", add=True)
            await self.mkdir_async("root_folder")
            await self.repo_write_file("root_folder/test_file", "contents", add=True)
            await self.mkdir_async("root_folder/subfolder")
            changes1 = await self.getChangesSinceV2(position=position)
            expected_changes1 = [
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"test_folder",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"test_folder/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"test_folder/test_file",
                ),
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"root_folder",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root_folder/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"root_folder/test_file",
                ),
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"root_folder/subfolder",
                ),
            ]
            self.assertTrue(self.check_changes(changes1.changes, expected_changes1))

            changes2 = await self.getChangesSinceV2(
                position=position, root=b"root_folder"
            )
            # Only include files and folders inside the root, not including the root itself
            # Paths should be relative to the root
            expected_changes2 = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"test_file",
                ),
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"subfolder",
                ),
            ]
            self.assertTrue(self.check_changes(changes2.changes, expected_changes2))

    async def test_root_is_file(self):
        await self.repo_write_file("this_path_is_a_file", "contents", add=True)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            with self.assertRaises(EdenError) as ctx:
                await self.getChangesSinceV2(
                    position=position, root=b"this_path_is_a_file"
                )

            mount_string = self.mount_path
            self.assertRegex(
                ctx.exception.message,
                'Invalid root path "this_path_is_a_file" in mount .*'
                + re.escape(str(mount_string)),
            )
            self.assertEqual(ctx.exception.errorCode, errno.EINVAL)
            self.assertEqual(
                ctx.exception.errorType,
                EdenErrorType.ARGUMENT_ERROR,
            )

    async def test_rename_file_root_in_to_out(self):
        await self.repo_write_file("root/test_file", "A")
        self.mkdir("out/")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("root/test_file", "out/test_file")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.REGULAR,
                    path=b"test_file",
                )
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_file_root_out_to_in(self):
        await self.repo_write_file("out/test_file", "A")
        self.mkdir("root/")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("out/test_file", "root/test_file")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"test_file",
                )
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_root_not_included_in_result(self):
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.mkdir_async("root/")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = []
            self.assertEqual(changes.changes, expected_changes)


@testcase.eden_repo_test
class ChangesTestNix(JournalTestBase):
    async def test_add_file(self):
        # When adding a file, it is technically written to so there's an additional modified operation
        changes = await self.setup_test_add_file()
        expected_changes = [
            buildSmallChange(Added, Dtype.REGULAR, path=b"test_file"),
            buildSmallChange(Modified, Dtype.REGULAR, path=b"test_file"),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_add_file_root(self):
        # When adding a file, it is technically written to so there's an additional modified operation
        changes = await self.setup_test_add_file_root(b"root")
        expected_changes = [
            buildSmallChange(Added, Dtype.REGULAR, path=b"test_file"),
            buildSmallChange(Modified, Dtype.REGULAR, path=b"test_file"),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_file(self):
        changes = await self.setup_test_rename_file()
        expected_changes = [
            buildSmallChange(
                Renamed,
                Dtype.REGULAR,
                from_path=b"test_file",
                to_path=b"best_file",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_file_root(self):
        await self.repo_write_file("root/test_file", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("root/test_file", "root/best_file")
            changes = await self.getChangesSinceV2(position=position, root=b"root")

            expected_changes = [
                buildSmallChange(
                    Renamed,
                    Dtype.REGULAR,
                    from_path=b"test_file",
                    to_path=b"best_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_replace_file(self):
        self.eden_repo.write_file("test_file", "test_contents", add=False)
        self.eden_repo.write_file("gone_file", "replaced_contents", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("test_file", "gone_file")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Replaced,
                    Dtype.REGULAR,
                    from_path=b"test_file",
                    to_path=b"gone_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_replace_file_root(self):
        self.eden_repo.write_file("root/test_file", "test_contents", add=False)
        self.eden_repo.write_file("root/gone_file", "replaced_contents", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("root/test_file", "root/gone_file")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Replaced,
                    Dtype.REGULAR,
                    from_path=b"test_file",
                    to_path=b"gone_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_replace_file_root_in_to_out(self):
        self.eden_repo.write_file("root/test_file", "test_contents", add=False)
        self.eden_repo.write_file("out/gone_file", "replaced_contents", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("root/test_file", "out/gone_file")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.REGULAR,
                    path=b"test_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_replace_file_root_out_to_in(self):
        self.eden_repo.write_file("out/test_file", "test_contents", add=False)
        self.eden_repo.write_file("root/gone_file", "replaced_contents", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("out/test_file", "root/gone_file")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"gone_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_replace_file_different_folder(self):
        self.eden_repo.write_file("source_folder/test_file", "test_contents", add=False)
        self.eden_repo.write_file("gone_file", "replaced_contents", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("source_folder/test_file", "gone_file")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Replaced,
                    Dtype.REGULAR,
                    from_path=b"source_folder/test_file",
                    to_path=b"gone_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))
            self.assertEqual("test_contents", self.read_file("gone_file"))

    async def test_rename_folder(self):
        self.mkdir("test_folder")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("test_folder", "best_folder")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildLargeChange(
                    DirectoryRenamed,
                    from_bytes=b"test_folder",
                    to_bytes=b"best_folder",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_folder_root(self):
        self.mkdir("root/test_folder")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("root/test_folder", "root/best_folder")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildLargeChange(
                    DirectoryRenamed,
                    from_bytes=b"test_folder",
                    to_bytes=b"best_folder",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_folder_root_in_to_out(self):
        self.mkdir("root/test_folder")
        self.mkdir("out/")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("root/test_folder", "out/test_folder")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.DIR,
                    path=b"test_folder",
                )
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_folder_root_out_to_in(self):
        self.mkdir("out/test_folder")
        self.mkdir("root/")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("out/test_folder", "root/test_folder")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"test_folder",
                )
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_replace_folder(self):
        self.eden_repo.mkdir("test_folder")
        self.eden_repo.mkdir("gone_folder")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async("test_folder", "gone_folder")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Replaced,
                    Dtype.DIR,
                    from_path=b"test_folder",
                    to_path=b"gone_folder",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_copy_file_different_folder(self):
        # Copying a file over a different file shows up as a "Modify"
        self.eden_repo.write_file("source_folder/test_file", "test_contents", add=False)
        self.eden_repo.write_file("gone_file", "replaced_contents", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            self.copy("source_folder/test_file", "gone_file")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"gone_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))
            self.assertEqual("test_contents", self.read_file("gone_file"))

    # Python's chmod/chown only work on nix systems
    async def test_modify_folder_chmod(self):
        self.mkdir("test_folder_chmod")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_chmod("test_folder_chmod", 0o777)
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Modified,
                    Dtype.DIR,
                    path=b"test_folder_chmod",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_modify_folder_chown(self):
        # Due to platform differences and root permission requirements,
        # this test doesn't run on Sandcastle or on Mac
        self.eden_repo.mkdir("test_folder_chown")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_chown("test_folder_chown")
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(
                    Modified,
                    Dtype.DIR,
                    path=b"test_folder_chown",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_include(self):
        # Tests if a folder is renamed from an included directory to an not included directory
        # and vice versa it shows up
        self.mkdir("included_folder")
        self.mkdir("not_included_folder")
        self.mkdir("not_included_folder2")
        await self.repo_write_file("included_folder/test_file", "contents", add=False)
        await self.repo_write_file(
            "not_included_folder/test_file2", "contents", add=False
        )
        await self.repo_write_file(
            "not_included_folder/test_file3", "contents", add=False
        )
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async(
                "included_folder/test_file", "not_included_folder/test_file"
            )
            await self.rename_async(
                "not_included_folder/test_file2", "included_folder/test_file2"
            )
            await self.rename_async(
                "not_included_folder/test_file3", "not_included_folder2/test_file3"
            )
            changes = await self.getChangesSinceV2(
                position=position, included_roots=["included_folder"]
            )
            # We expect changes involving included folders to be present and changes involving
            # not_included folders to be ignored if they are not renamed to an included folder
            expected_changes = [
                buildSmallChange(
                    Renamed,
                    Dtype.REGULAR,
                    from_path=b"included_folder/test_file",
                    to_path=b"not_included_folder/test_file",
                ),
                buildSmallChange(
                    Renamed,
                    Dtype.REGULAR,
                    from_path=b"not_included_folder/test_file2",
                    to_path=b"included_folder/test_file2",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_exclude(self):
        # Tests if a folder is renamed from an excluded directory to an not excluded directory
        # and vice versa it shows up

        self.mkdir("not_excluded_folder")
        self.mkdir("not_excluded_folder2")
        self.mkdir("excluded_folder")
        await self.repo_write_file(
            "not_excluded_folder/test_file", "contents", add=False
        )
        await self.repo_write_file("excluded_folder/test_file2", "contents", add=False)
        await self.repo_write_file(
            "not_excluded_folder/test_file3", "contents", add=False
        )
        await self.repo_write_file("excluded_folder/test_file4", "contents", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.rename_async(
                "not_excluded_folder/test_file", "excluded_folder/test_file"
            )
            await self.rename_async(
                "excluded_folder/test_file2", "not_excluded_folder/test_file2"
            )
            await self.rename_async(
                "not_excluded_folder/test_file3", "not_excluded_folder2/test_file3"
            )
            await self.rename_async(
                "excluded_folder/test_file4", "excluded_folder/test_file4"
            )
            changes = await self.getChangesSinceV2(
                position=position, excluded_roots=["excluded_folder"]
            )
            # We expect changes involving not_excluded folders to be present and changes involving
            # excluded folders to be ignored if they are not renamed to a not_excluded folder
            expected_changes = [
                buildSmallChange(
                    Renamed,
                    Dtype.REGULAR,
                    from_path=b"not_excluded_folder/test_file",
                    to_path=b"excluded_folder/test_file",
                ),
                buildSmallChange(
                    Renamed,
                    Dtype.REGULAR,
                    from_path=b"excluded_folder/test_file2",
                    to_path=b"not_excluded_folder/test_file2",
                ),
                buildSmallChange(
                    Renamed,
                    Dtype.REGULAR,
                    from_path=b"not_excluded_folder/test_file3",
                    to_path=b"not_excluded_folder2/test_file3",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_too_many_changes(self):
        self.mkdir("test_folder")
        expected_changes1 = []
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            # usually the max changes is 10k but for test speed reasons we set it to 100
            # Each file add creates 2 changes, one for the add and one for the modify
            for i in range(50):
                expected_changes1 += await self.add_file_expect(
                    f"test_folder/test_file{i}", f"{i}"
                )
            changes1 = await self.getChangesSinceV2(position=position)
            await self.repo_write_file("test_folder/last_file", "i")
            changes2 = await self.getChangesSinceV2(position=position)
            expected_changes2 = [
                buildLargeChange(
                    LostChanges,
                    lost_change_reason=LostChangesReason.TOO_MANY_CHANGES,
                ),
            ]
            self.assertTrue(len(expected_changes1) == 100)
            self.assertTrue(len(changes1.changes) == 100)
            self.assertTrue(len(changes2.changes) == 1)
            self.assertTrue(self.check_changes(changes1.changes, expected_changes1))
            self.assertTrue(self.check_changes(changes2.changes, expected_changes2))

    async def test_too_many_changes_filtering(self):
        self.mkdir("test_folder1")
        self.mkdir("test_folder2")
        expected_changes1 = []
        expected_changes2 = []
        expected_changes3 = []
        expected_changes4 = []
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)

            # usually the max changes is 10k but for test speed reasons we set it to 100
            # Each file add creates 2 changes, one for the add and one for the modify
            for i in range(25):
                expected_changes1 += await self.add_file_expect(
                    f"test_folder1/test_file{i}.suf1", f"{i}"
                )
            for i in range(25):
                expected_changes2 += await self.add_file_expect(
                    f"test_folder1/test_file{i}.suf2", f"{i}"
                )
            for i in range(25):
                expected_changes3 += await self.add_file_expect(
                    f"test_folder2/test_file{i}.suf3", f"{i}"
                )
            for i in range(25):
                expected_changes4 += await self.add_file_expect(
                    f"test_folder2/test_file{i}.suf4", f"{i}"
                )
            changes1 = await self.getChangesSinceV2(position=position)
            expected_changes_too_many = [
                buildLargeChange(
                    LostChanges,
                    lost_change_reason=LostChangesReason.TOO_MANY_CHANGES,
                ),
            ]
            self.assertTrue(
                self.check_changes(changes1.changes, expected_changes_too_many)
            )

            # Test filtering by includes
            changes2 = await self.getChangesSinceV2(
                position=position, included_roots=["test_folder1"]
            )
            self.assertTrue(
                self.check_changes(
                    changes2.changes, expected_changes1 + expected_changes2
                )
            )

            # Test filtering by excludes
            changes3 = await self.getChangesSinceV2(
                position=position, excluded_roots=["test_folder1"]
            )
            self.assertTrue(
                self.check_changes(
                    changes3.changes, expected_changes3 + expected_changes4
                )
            )

            # Test filtering by suffix
            changes4 = await self.getChangesSinceV2(
                position=position, included_suffixes=[".suf1", ".suf3"]
            )
            self.assertTrue(
                self.check_changes(
                    changes4.changes, expected_changes1 + expected_changes3
                )
            )

            changes5 = await self.getChangesSinceV2(
                position=position, excluded_suffixes=[".suf1", ".suf3"]
            )
            self.assertTrue(
                self.check_changes(
                    changes5.changes, expected_changes2 + expected_changes4
                )
            )

    async def test_include_vcs_roots_false(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            # .git and .hg are symlinked so test with .sl
            self.mkdir(".sl")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(".sl/vcs_file", "", add=False)

            changes = await self.getChangesSinceV2(
                position=position,
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
            ]

            self.assertTrue(self.check_changes_exact(changes.changes, expected_changes))

    async def test_include_vcs_roots_without_root(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            # .git and .hg are symlinked so test with .sl
            self.mkdir(".sl")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(".sl/vcs_file", "", add=False)

            changes = await self.getChangesSinceV2(
                position=position, includeVCSRoots=True
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b".sl/vcs_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b".sl/vcs_file",
                ),
            ]

            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_include_vcs_roots_with_root(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            # .git and .hg are symlinked so test with .sl
            self.mkdir(".sl")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(".sl/vcs_file", "", add=False)

            changes = await self.getChangesSinceV2(
                position=position, root="root", includeVCSRoots=True
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"test_file",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b".sl/vcs_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b".sl/vcs_file",
                ),
            ]

            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_include_state_changes_false(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            self.mkdir(".edenfs-notifications-state/state")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(
                ".edenfs-notifications-state/state/state.notify", "", add=False
            )

            changes = await self.getChangesSinceV2(
                position=position,
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
            ]

            self.assertTrue(self.check_changes_exact(changes.changes, expected_changes))

    async def test_include_states_without_root(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            self.mkdir(".edenfs-notifications-state")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(
                ".edenfs-notifications-state/state/state.notify", "", add=False
            )

            changes = await self.getChangesSinceV2(
                position=position, includeStateChanges=True
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildStateChange(
                    StateEntered,
                    "state",
                ),
            ]

            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_include_states_with_root(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            self.mkdir(".edenfs-notifications-state")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(
                ".edenfs-notifications-state/state/state.notify", "", add=False
            )

            changes = await self.getChangesSinceV2(
                position=position, includeStateChanges=True
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildSmallChange(
                    Modified,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildStateChange(
                    StateEntered,
                    "state",
                ),
            ]

            self.assertTrue(self.check_changes(changes.changes, expected_changes))


@testcase.eden_repo_test
class ChangesTestWin(WindowsJournalTestBase):
    async def test_add_file(self):
        # In windows, the file is created and then modified in projfs, then eden gets
        # a single ADDED notification
        changes = await self.setup_test_add_file()
        expected_changes = [buildSmallChange(Added, Dtype.REGULAR, path=b"test_file")]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_add_file_root(self):
        # In windows, the file is created and then modified in projfs, then eden gets
        # a single ADDED notification
        changes = await self.setup_test_add_file_root(b"root")
        expected_changes = [buildSmallChange(Added, Dtype.REGULAR, path=b"test_file")]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_file(self):
        changes = await self.setup_test_rename_file()
        expected_changes = [
            buildSmallChange(Removed, Dtype.REGULAR, path=b"test_file"),
            buildSmallChange(Added, Dtype.REGULAR, path=b"best_file"),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_file_root(self):
        await self.repo_write_file("root/test_file", "", add=False)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            self.rename("root/test_file", "root/best_file")
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(Removed, Dtype.REGULAR, path=b"test_file"),
                buildSmallChange(Added, Dtype.REGULAR, path=b"best_file"),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    # Files cannot be replaced in windows
    async def test_replace_file(self):
        await self.repo_write_file("test_file", "test_contents", add=False)
        await self.repo_write_file("gone_file", "replaced_contents", add=False)
        with self.assertRaises(FileExistsError):
            self.rename("test_file", "gone_file")

    # Renaming uncommitted folders in windows is an add and delete
    async def test_rename_folder(self):
        self.mkdir("test_folder")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            # ensure that the file change is synced to the new folder
            await self.syncProjFS(position)
            self.rename("test_folder", "best_folder")
            position2 = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.syncProjFS(position2)
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(Removed, Dtype.DIR, path=b"test_folder"),
                buildSmallChange(Added, Dtype.DIR, path=b"best_folder"),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_folder_root(self):
        self.mkdir("root/test_folder")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            # ensure that the file change is synced to the new folder
            await self.syncProjFS(position)
            self.rename("root/test_folder", "root/best_folder")
            position2 = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.syncProjFS(position2)
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(Removed, Dtype.DIR, path=b"test_folder"),
                buildSmallChange(Added, Dtype.DIR, path=b"best_folder"),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_folder_root_in_to_out(self):
        self.mkdir("root/test_folder")
        self.mkdir("out/")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            # ensure that the file change is synced to the new folder
            await self.syncProjFS(position)
            self.rename("root/test_folder", "out/test_folder")
            position2 = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.syncProjFS(position2)
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Removed,
                    Dtype.DIR,
                    path=b"test_folder",
                )
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_rename_folder_root_out_to_in(self):
        self.mkdir("out/test_folder")
        self.mkdir("root/")
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            # ensure that the file change is synced to the new folder
            await self.syncProjFS(position)
            self.rename("out/test_folder", "root/test_folder")
            position2 = await client.getCurrentJournalPosition(self.mount_path_bytes)
            # ensure that the file change is synced to the new folder
            await self.syncProjFS(position2)
            changes = await self.getChangesSinceV2(position=position, root=b"root")
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.DIR,
                    path=b"test_folder",
                )
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    # Renaming uncommitted folders with a file
    async def test_rename_folder_uncommitted_file(self):
        self.mkdir("test_folder")
        await self.repo_write_file("test_folder/test_file", "contents", add=True)
        async with self.get_thrift_client() as client:
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            self.rename("test_folder", "best_folder")
            position2 = await client.getCurrentJournalPosition(self.mount_path_bytes)
            # ensure that the file change is synced to the new folder
            await self.syncProjFS(position2)
            changes = await self.getChangesSinceV2(position=position)
            expected_changes = [
                buildSmallChange(Removed, Dtype.DIR, path=b"test_folder"),
                buildSmallChange(Added, Dtype.DIR, path=b"best_folder"),
                # No REMOVED for test_file, on ProjFS, there's no change reported
                # for subfolders and files if the parent folder gets moved
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"best_folder/test_file",
                ),
            ]
            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    # Renaming folders that have been checked out is not allowed
    async def test_rename_folder_committed_file(self):
        # Files created in setup.
        with self.assertRaises(OSError):
            self.rename(self.get_path("the_land"), self.get_path("deepest_blue"))

        # In windows, files that were checked out via checkout cannot be renamed
        self.mkdir("test_folder")
        await self.repo_write_file("test_folder/test_file", "contents", add=True)
        self.eden_repo.hg()
        commit1 = self.eden_repo.commit("commit 1")
        self.eden_repo.hg("goto", self.commit0)
        self.eden_repo.hg("goto", commit1)

        with self.assertRaises(OSError):
            self.rename(self.get_path("test_folder"), self.get_path("best_folder"))

    async def test_include_vcs_roots_false(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            # .git and .hg are symlinked so test with .sl
            self.mkdir(".sl")
            start_position = await client.getCurrentJournalPosition(
                self.mount_path_bytes
            )
            await self.syncProjFS(start_position)
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(".sl/vcs_file", "", add=False)
            position2 = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.syncProjFS(position2)
            changes = await self.getChangesSinceV2(
                position=position,
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
            ]

            self.assertTrue(self.check_changes_exact(changes.changes, expected_changes))

    async def test_include_vcs_roots_without_root(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            # .git and .hg are symlinked so test with .sl
            self.mkdir(".sl")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(".sl/vcs_file", "", add=False)

            changes = await self.getChangesSinceV2(
                position=position, includeVCSRoots=True
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b".sl/vcs_file",
                ),
            ]

            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_include_vcs_roots_with_root(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            # .git and .hg are symlinked so test with .sl
            self.mkdir(".sl")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(".sl/vcs_file", "", add=False)

            changes = await self.getChangesSinceV2(
                position=position, root="root", includeVCSRoots=True
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"test_file",
                ),
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b".sl/vcs_file",
                ),
            ]

            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_include_state_changes_false(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            self.mkdir(".edenfs-notifications-state/state")
            start_position = await client.getCurrentJournalPosition(
                self.mount_path_bytes
            )
            await self.syncProjFS(start_position)
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(
                ".edenfs-notifications-state/state/state.notify", "", add=False
            )

            changes = await self.getChangesSinceV2(
                position=position,
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
            ]

            self.assertTrue(self.check_changes_exact(changes.changes, expected_changes))

    async def test_include_states_without_root(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            self.mkdir(".edenfs-notifications-state")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(
                ".edenfs-notifications-state/state/state.notify", "", add=False
            )

            changes = await self.getChangesSinceV2(
                position=position, includeStateChanges=True
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildStateChange(
                    StateEntered,
                    "state",
                ),
            ]

            self.assertTrue(self.check_changes(changes.changes, expected_changes))

    async def test_include_states_with_root(self):
        async with self.get_thrift_client() as client:
            self.mkdir("root")
            self.mkdir(".edenfs-notifications-state")
            position = await client.getCurrentJournalPosition(self.mount_path_bytes)
            await self.repo_write_file("root/test_file", "", add=False)
            await self.repo_write_file(
                ".edenfs-notifications-state/state/state.notify", "", add=False
            )

            changes = await self.getChangesSinceV2(
                position=position, includeStateChanges=True
            )
            expected_changes = [
                buildSmallChange(
                    Added,
                    Dtype.REGULAR,
                    path=b"root/test_file",
                ),
                buildStateChange(
                    StateEntered,
                    "state",
                ),
            ]

            self.assertTrue(self.check_changes(changes.changes, expected_changes))
