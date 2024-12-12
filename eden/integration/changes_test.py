#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import sys

from facebook.eden.ttypes import Dtype, LostChangesReason, SmallChangeNotification

from .lib import testcase
from .lib.journal_test_base import JournalTestBase, WindowsJournalTestBase
from .lib.thrift_objects import buildSmallChange, getLargeChangeSafe


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
