#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import errno
import os
import sys

from facebook.eden.ttypes import (
    Dtype,
    EdenError,
    EdenErrorType,
    LargeChangeNotification,
    LostChangesReason,
    SmallChangeNotification,
)

from .lib import testcase
from .lib.journal_test_base import JournalTestBase, WindowsJournalTestBase
from .lib.thrift_objects import buildLargeChange, buildSmallChange, getLargeChangeSafe


if sys.platform == "win32":
    testBase = WindowsJournalTestBase
else:
    testBase = JournalTestBase


@testcase.eden_repo_test
class ChangesTestCommon(testBase):
    def test_wrong_mount_generation(self):
        # The input mount generation should equal the current mount generation
        oldPosition = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.eden.unmount(self.mount_path)
        self.eden.mount(self.mount_path)
        changes = self.getChangesSinceV2(oldPosition)
        self.assertEqual(len(changes.changes), 1)
        largeChange = getLargeChangeSafe(changes.changes[0])
        self.assertIsNotNone(largeChange)
        self.assertEqual(
            largeChange.get_lostChanges().reason,
            LostChangesReason.EDENFS_REMOUNTED,
        )

    def test_exclude_directory(self):
        expected_changes = []
        oldPosition = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.add_folder_expect("ignored_dir")
        self.add_folder_expect("ignored_dir2/nested_ignored_dir")
        expected_changes += self.add_folder_expect("want_dir")
        # same name in subdir should not be ignored
        expected_changes += self.add_folder_expect("want_dir/ignored_dir")
        self.add_file_expect("ignored_dir/test_file", "contents", add=False)
        expected_changes += self.add_file_expect(
            "want_dir/test_file", "contents", add=False
        )
        self.add_file_expect(
            "ignored_dir2/nested_ignored_dir/test_file", "contents", add=False
        )
        expected_changes += self.add_file_expect(
            "want_dir/ignored_dir/test_file", "contents", add=False
        )
        changes = self.getChangesSinceV2(
            oldPosition,
            excluded_roots=["ignored_dir", "ignored_dir2/nested_ignored_dir"],
        )
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_include_directory(self):
        expected_changes = []
        oldPosition = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.mkdir("ignored_dir")
        self.mkdir("ignored_dir2/nested_ignored_dir")
        expected_changes += self.add_folder_expect("want_dir")
        expected_changes += self.add_folder_expect("want_dir/ignored_dir")
        self.add_file_expect("ignored_dir/test_file", "contents", add=False)
        expected_changes += self.add_file_expect(
            "want_dir/test_file", "contents", add=False
        )
        self.add_file_expect(
            "ignored_dir2/nested_ignored_dir/test_file", "contents", add=False
        )
        expected_changes += self.add_file_expect(
            "want_dir/ignored_dir/test_file", "contents", add=False
        )
        changes = self.getChangesSinceV2(
            oldPosition,
            included_roots=["want_dir"],
        )
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_include_exclude_directory(self):
        # if directory is both included and excluded, it should be ignored
        oldPosition = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.mkdir("include_exclude_dir")
        self.mkdir("ignored_dir")
        self.add_file_expect("ignored_dir/test_file", "contents", add=False)
        self.add_file_expect("include_exclude_dir/test_file", "contents", add=False)
        changes = self.getChangesSinceV2(
            oldPosition,
            included_roots=["include_exclude_dir"],
            excluded_roots=["include_exclude_dir"],
        )
        self.assertEqual(changes.changes, [])

    def test_include_file_suffix(self):
        # use removed files for cross-os compatibility
        self.repo_write_file("test_file.ext1", "", add=False)
        self.repo_write_file("test_file.ext2", "", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rm("test_file.ext1")
        self.rm("test_file.ext2")
        changes = self.getChangesSinceV2(position=position, included_suffixes=[".ext1"])
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REMOVED,
                Dtype.REGULAR,
                path=b"test_file.ext1",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_exclude_file_suffix(self):
        self.repo_write_file("test_file.ext1", "", add=False)
        self.repo_write_file("test_file.ext2", "", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rm("test_file.ext1")
        self.rm("test_file.ext2")
        changes = self.getChangesSinceV2(position=position, excluded_suffixes=[".ext1"])
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REMOVED,
                Dtype.REGULAR,
                path=b"test_file.ext2",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_incude_exclude_file_same_suffix(self):
        self.repo_write_file("test_file.ext1", "", add=False)
        self.repo_write_file("test_file.ext2", "", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rm("test_file.ext1")
        self.rm("test_file.ext2")
        changes = self.getChangesSinceV2(
            position=position, included_suffixes=[".ext1"], excluded_suffixes=[".ext1"]
        )
        self.assertEqual(changes.changes, [])

    def test_incude_exclude_file_suffix(self):
        self.repo_write_file("test_file.ext1", "", add=False)
        self.repo_write_file("test_file.ext2", "", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rm("test_file.ext1")
        self.rm("test_file.ext2")
        changes = self.getChangesSinceV2(
            position=position, included_suffixes=[".ext1"], excluded_suffixes=[".ext2"]
        )
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REMOVED,
                Dtype.REGULAR,
                path=b"test_file.ext1",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_modify_file(self):
        self.repo_write_file("test_file", "", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.repo_write_file("test_file", "contents", add=False)
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.MODIFIED, Dtype.REGULAR, path=b"test_file"
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_remove_file(self):
        self.repo_write_file("test_file", "", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rm("test_file")
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REMOVED,
                Dtype.REGULAR,
                path=b"test_file",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_add_folder(self):
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.mkdir("test_folder")
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.DIR,
                path=b"test_folder",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_remove_folder(self):
        self.mkdir("test_folder")
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.repo_rmdir("test_folder")
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REMOVED,
                Dtype.DIR,
                path=b"test_folder",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_commit_transition(self):
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.mkdir("test_folder")
        self.repo_write_file("test_folder/test_file", "contents", add=True)
        commit1 = self.eden_repo.commit("commit 1")
        changes1 = self.getChangesSinceV2(position=position)
        expected_changes1 = [
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.DIR,
                path=b"test_folder",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.REGULAR,
                path=b"test_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.REGULAR,
                path=b"test_folder/test_file",
            ),
            # For CommitTransition, from_bytes is the current hash and to_bytes is the previous hash
            buildLargeChange(
                LargeChangeNotification.COMMITTRANSITION,
                from_bytes=bytes.fromhex(self.commit0),
                to_bytes=bytes.fromhex(commit1),
            ),
        ]
        self.assertTrue(self.check_changes(changes1.changes, expected_changes1))

        self.eden_repo.hg("goto", self.commit0)
        changes2 = self.getChangesSinceV2(position=changes1.toPosition)
        expected_changes2 = [
            buildLargeChange(
                LargeChangeNotification.COMMITTRANSITION,
                from_bytes=bytes.fromhex(commit1),
                to_bytes=bytes.fromhex(self.commit0),
            ),
        ]
        self.assertTrue(self.check_changes(changes2.changes, expected_changes2))
        # Check that the file was removed when going down a commit
        self.assertFalse(os.path.exists(self.get_path("/test_folder/test_file")))

    def test_truncated_journal(self):
        # Tests that when the journal has been truncated, we get a lost changes notification
        # We expect the following
        # Changes before the truncation are reported normally when there is no truncation
        # When there is a truncation in between the start position and the current position,
        #   we only report that there has been a truncated journal. Neither changes before and
        #   within the window are reported.
        # Changes after the truncation are reported when the start position is after the truncation
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.mkdir("not_seen_folder")
        self.repo_write_file("not_seen_folder/not_seen_file", "missing", add=True)
        changes0 = self.getChangesSinceV2(position=position)
        self.eden.run_cmd("debug", "flush_journal", self.mount_path)
        position2 = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.mkdir("test_folder")
        self.repo_write_file("test_folder/test_file", "contents", add=True)
        changes = self.getChangesSinceV2(position=position)
        changes2 = self.getChangesSinceV2(position=changes.toPosition)
        changes3 = self.getChangesSinceV2(position=position2)
        expected_changes0 = [
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.DIR,
                path=b"not_seen_folder",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.REGULAR,
                path=b"not_seen_folder/not_seen_file",
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.REGULAR,
                path=b"not_seen_folder/not_seen_file",
            ),
        ]
        expected_changes = [
            buildLargeChange(
                LargeChangeNotification.LOSTCHANGES,
                lost_change_reason=LostChangesReason.JOURNAL_TRUNCATED,
            ),
        ]
        expected_changes3 = [
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.DIR,
                path=b"test_folder",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.REGULAR,
                path=b"test_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.REGULAR,
                path=b"test_folder/test_file",
            ),
        ]
        self.assertTrue(self.check_changes(changes0.changes, expected_changes0))
        self.assertTrue(self.check_changes(changes.changes, expected_changes))
        self.assertEqual(changes2.changes, [])
        self.assertTrue(self.check_changes(changes3.changes, expected_changes3))

    def test_root_does_not_exist(self):
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        with self.assertRaises(EdenError) as ctx:
            self.getChangesSinceV2(position=position, root="this_path_does_not_exist")

        self.assertEqual(
            ctx.exception.message,
            f'Invalid root path "this_path_does_not_exist" in mount {self.mount_path}',
        )
        self.assertEqual(ctx.exception.errorCode, errno.EINVAL)
        self.assertEqual(
            ctx.exception.errorType,
            EdenErrorType.ARGUMENT_ERROR,
        )

    def test_root_exists(self):
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.mkdir("test_folder")
        self.repo_write_file("test_folder/test_file", "contents", add=True)
        self.mkdir("root_folder")
        self.repo_write_file("root_folder/test_file", "contents", add=True)
        changes1 = self.getChangesSinceV2(position=position)
        expected_changes1 = [
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.DIR,
                path=b"test_folder",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.REGULAR,
                path=b"test_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.REGULAR,
                path=b"test_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.DIR,
                path=b"root_folder",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.REGULAR,
                path=b"root_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.REGULAR,
                path=b"root_folder/test_file",
            ),
        ]
        self.assertTrue(self.check_changes(changes1.changes, expected_changes1))

        changes2 = self.getChangesSinceV2(position=position, root="root_folder")
        # Currently actually checking for the root is not implemented. Remove the entries
        # under test_folder once that is implemented
        expected_changes2 = [
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.DIR,
                path=b"test_folder",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.REGULAR,
                path=b"test_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.REGULAR,
                path=b"test_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.DIR,
                path=b"root_folder",
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.REGULAR,
                path=b"root_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.REGULAR,
                path=b"root_folder/test_file",
            ),
        ]
        self.assertTrue(self.check_changes(changes2.changes, expected_changes2))

    def test_root_is_file(self):
        self.repo_write_file("this_path_is_a_file", "contents", add=True)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        with self.assertRaises(EdenError) as ctx:
            self.getChangesSinceV2(position=position, root="this_path_is_a_file")

        self.assertEqual(
            ctx.exception.message,
            f'Invalid root path "this_path_is_a_file" in mount {self.mount_path}',
        )
        self.assertEqual(ctx.exception.errorCode, errno.EINVAL)
        self.assertEqual(
            ctx.exception.errorType,
            EdenErrorType.ARGUMENT_ERROR,
        )


# The following tests have different results based on platform


@testcase.eden_repo_test
class ChangesTestNix(JournalTestBase):
    def setUp(self) -> None:
        if sys.platform == "win32":
            self.skipTest("Non-Windows test")
        return super().setUp()

    def test_add_file(self):
        # When adding a file, it is technically written to so there's an additional modified operation
        changes = self.setup_test_add_file()
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.ADDED, Dtype.REGULAR, path=b"test_file"
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED, Dtype.REGULAR, path=b"test_file"
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_rename_file(self):
        changes = self.setup_test_rename_file()
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.RENAMED,
                Dtype.REGULAR,
                from_path=b"test_file",
                to_path=b"best_file",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_replace_file(self):
        self.eden_repo.write_file("test_file", "test_contents", add=False)
        self.eden_repo.write_file("gone_file", "replaced_contents", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rename("test_file", "gone_file")
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REPLACED,
                Dtype.REGULAR,
                from_path=b"test_file",
                to_path=b"gone_file",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    # Python's chmod/chown only work on nix systems
    def test_modify_folder_chmod(self):
        self.mkdir("test_folder_chmod")
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.repo_chmod("test_folder_chmod", 0o777)
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.DIR,
                path=b"test_folder_chmod",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_modify_folder_chown(self):
        # Due to platform differences and root permission requirements,
        # this test doesn't run on Sandcastle
        self.eden_repo.mkdir("test_folder_chown")
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.repo_chown("test_folder_chown")
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.MODIFIED,
                Dtype.DIR,
                path=b"test_folder_chown",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_rename_folder(self):
        self.mkdir("test_folder")
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rename("test_folder", "best_folder")
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildLargeChange(
                LargeChangeNotification.DIRECTORYRENAMED,
                from_bytes=b"test_folder",
                to_bytes=b"best_folder",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_rename_include(self):
        # Tests if a folder is renamed from an included directory to an not included directory
        # and vice versa it shows up
        self.mkdir("included_folder")
        self.mkdir("not_included_folder")
        self.mkdir("not_included_folder2")
        self.repo_write_file("included_folder/test_file", "contents", add=False)
        self.repo_write_file("not_included_folder/test_file2", "contents", add=False)
        self.repo_write_file("not_included_folder/test_file3", "contents", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rename("included_folder/test_file", "not_included_folder/test_file")
        self.rename("not_included_folder/test_file2", "included_folder/test_file2")
        self.rename("not_included_folder/test_file3", "not_included_folder2/test_file3")
        changes = self.getChangesSinceV2(
            position=position, included_roots=["included_folder"]
        )
        # We expect changes involving included folders to be present and changes involving
        # not_included folders to be ignored if they are not renamed to an included folder
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.RENAMED,
                Dtype.REGULAR,
                from_path=b"included_folder/test_file",
                to_path=b"not_included_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.RENAMED,
                Dtype.REGULAR,
                from_path=b"not_included_folder/test_file2",
                to_path=b"included_folder/test_file2",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_rename_exclude(self):
        # Tests if a folder is renamed from an excluded directory to an not excluded directory
        # and vice versa it shows up

        self.mkdir("not_excluded_folder")
        self.mkdir("not_excluded_folder2")
        self.mkdir("excluded_folder")
        self.repo_write_file("not_excluded_folder/test_file", "contents", add=False)
        self.repo_write_file("excluded_folder/test_file2", "contents", add=False)
        self.repo_write_file("not_excluded_folder/test_file3", "contents", add=False)
        self.repo_write_file("excluded_folder/test_file4", "contents", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rename("not_excluded_folder/test_file", "excluded_folder/test_file")
        self.rename("excluded_folder/test_file2", "not_excluded_folder/test_file2")
        self.rename("not_excluded_folder/test_file3", "not_excluded_folder2/test_file3")
        self.rename("excluded_folder/test_file4", "excluded_folder/test_file4")
        changes = self.getChangesSinceV2(
            position=position, excluded_roots=["excluded_folder"]
        )
        # We expect changes involving not_excluded folders to be present and changes involving
        # excluded folders to be ignored if they are not renamed to a not_excluded folder
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.RENAMED,
                Dtype.REGULAR,
                from_path=b"not_excluded_folder/test_file",
                to_path=b"excluded_folder/test_file",
            ),
            buildSmallChange(
                SmallChangeNotification.RENAMED,
                Dtype.REGULAR,
                from_path=b"excluded_folder/test_file2",
                to_path=b"not_excluded_folder/test_file2",
            ),
            buildSmallChange(
                SmallChangeNotification.RENAMED,
                Dtype.REGULAR,
                from_path=b"not_excluded_folder/test_file3",
                to_path=b"not_excluded_folder2/test_file3",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_too_many_changes(self):
        self.mkdir("test_folder")
        expected_changes1 = []
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        # usually the max changes is 10k but for test speed reasons we set it to 100
        # Each file add creates 2 changes, one for the add and one for the modify
        for i in range(50):
            expected_changes1 += self.add_file_expect(
                f"test_folder/test_file{i}", f"{i}"
            )
        changes1 = self.getChangesSinceV2(position=position)
        self.repo_write_file("test_folder/last_file", "i")
        changes2 = self.getChangesSinceV2(position=position)
        expected_changes2 = [
            buildLargeChange(
                LargeChangeNotification.LOSTCHANGES,
                lost_change_reason=LostChangesReason.TOO_MANY_CHANGES,
            ),
        ]
        self.assertTrue(len(expected_changes1) == 100)
        self.assertTrue(len(changes1.changes) == 100)
        self.assertTrue(len(changes2.changes) == 1)
        self.assertTrue(self.check_changes(changes1.changes, expected_changes1))
        self.assertTrue(self.check_changes(changes2.changes, expected_changes2))

    def test_too_many_changes_filtering(self):
        self.mkdir("test_folder1")
        self.mkdir("test_folder2")
        expected_changes1 = []
        expected_changes2 = []
        expected_changes3 = []
        expected_changes4 = []
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)

        # usually the max changes is 10k but for test speed reasons we set it to 100
        # Each file add creates 2 changes, one for the add and one for the modify
        for i in range(25):
            expected_changes1 += self.add_file_expect(
                f"test_folder1/test_file{i}.suf1", f"{i}"
            )
        for i in range(25):
            expected_changes2 += self.add_file_expect(
                f"test_folder1/test_file{i}.suf2", f"{i}"
            )
        for i in range(25):
            expected_changes3 += self.add_file_expect(
                f"test_folder2/test_file{i}.suf3", f"{i}"
            )
        for i in range(25):
            expected_changes4 += self.add_file_expect(
                f"test_folder2/test_file{i}.suf4", f"{i}"
            )
        changes1 = self.getChangesSinceV2(position=position)
        expected_changes_too_many = [
            buildLargeChange(
                LargeChangeNotification.LOSTCHANGES,
                lost_change_reason=LostChangesReason.TOO_MANY_CHANGES,
            ),
        ]
        self.assertTrue(self.check_changes(changes1.changes, expected_changes_too_many))

        # Test filtering by includes
        changes2 = self.getChangesSinceV2(
            position=position, included_roots=["test_folder1"]
        )
        self.assertTrue(
            self.check_changes(changes2.changes, expected_changes1 + expected_changes2)
        )

        # Test filtering by excludes
        changes3 = self.getChangesSinceV2(
            position=position, excluded_roots=["test_folder1"]
        )
        self.assertTrue(
            self.check_changes(changes3.changes, expected_changes3 + expected_changes4)
        )

        # Test filtering by suffix
        changes4 = self.getChangesSinceV2(
            position=position, included_suffixes=[".suf1", ".suf3"]
        )
        self.assertTrue(
            self.check_changes(changes4.changes, expected_changes1 + expected_changes3)
        )

        changes5 = self.getChangesSinceV2(
            position=position, excluded_suffixes=[".suf1", ".suf3"]
        )
        self.assertTrue(
            self.check_changes(changes5.changes, expected_changes2 + expected_changes4)
        )


@testcase.eden_repo_test
class ChangesTestWin(WindowsJournalTestBase):
    def setUp(self) -> None:
        if sys.platform != "win32":
            self.skipTest("Windows only test")
        return super().setUp()

    def test_add_file(self):
        # In windows, the file is created and then modified in projfs, then eden gets
        # a single ADDED notification
        changes = self.setup_test_add_file()
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.ADDED, Dtype.REGULAR, path=b"test_file"
            )
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    def test_rename_file(self):
        changes = self.setup_test_rename_file()
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.RENAMED,
                Dtype.REGULAR,
                from_path=b"test_file",
                to_path=b"best_file",
            ),
        ]
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REMOVED, Dtype.REGULAR, path=b"test_file"
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED, Dtype.REGULAR, path=b"best_file"
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    # Files cannot be replaced in windows
    def test_replace_file(self):
        self.repo_write_file("test_file", "test_contents", add=False)
        self.repo_write_file("gone_file", "replaced_contents", add=False)
        with self.assertRaises(FileExistsError):
            self.rename("test_file", "gone_file")

    # Renaming uncommitted folders in windows is an add and delete
    def test_rename_folder(self):
        self.mkdir("test_folder")
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rename("test_folder", "best_folder")
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REMOVED, Dtype.DIR, path=b"test_folder"
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED, Dtype.DIR, path=b"best_folder"
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    # Renaming uncommitted folders with a file
    def test_rename_folder_uncommitted_file(self):
        self.mkdir("test_folder")
        self.repo_write_file("test_folder/test_file", "contents", add=True)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rename("test_folder", "best_folder")
        position2 = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        # ensure that the file change is synced to the new folder
        self.syncProjFS(position2)
        changes = self.getChangesSinceV2(position=position)
        expected_changes = [
            buildSmallChange(
                SmallChangeNotification.REMOVED, Dtype.DIR, path=b"test_folder"
            ),
            buildSmallChange(
                SmallChangeNotification.ADDED, Dtype.DIR, path=b"best_folder"
            ),
            # No REMOVED for test_file, on ProjFS, there's no change reported
            # for subfolders and files if the parent folder gets moved
            buildSmallChange(
                SmallChangeNotification.ADDED,
                Dtype.REGULAR,
                path=b"best_folder/test_file",
            ),
        ]
        self.assertTrue(self.check_changes(changes.changes, expected_changes))

    # Renaming folders that have been checked out is not allowed
    def test_rename_folder_committed_file(self):
        # Files created in setup.
        with self.assertRaises(OSError):
            self.rename(self.get_path("the_land"), self.get_path("deepest_blue"))

        # In windows, files that were checked out via checkout cannot be renamed
        self.mkdir("test_folder")
        self.repo_write_file("test_folder/test_file", "contents", add=True)
        self.eden_repo.hg()
        commit1 = self.eden_repo.commit("commit 1")
        self.eden_repo.hg("goto", self.commit0)
        self.eden_repo.hg("goto", commit1)

        with self.assertRaises(OSError):
            self.rename(self.get_path("test_folder"), self.get_path("best_folder"))
