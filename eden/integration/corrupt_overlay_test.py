#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging
import os
import pathlib
import stat
from typing import List

import eden.integration.lib.overlay as overlay_mod
from eden.integration.lib import testcase


class CorruptOverlayTest(testcase.HgRepoTestMixin, testcase.EdenRepoTest):
    """Test file operations when Eden's overlay is corrupted."""

    def setUp(self) -> None:
        super().setUp()  # type: ignore (pyre does not follow python MRO properly)
        self.overlay = overlay_mod.OverlayStore(self.eden, self.mount_path)

    def populate_repo(self) -> None:
        self.repo.write_file("src/committed_file", "committed_file content")
        self.repo.write_file("readme.txt", "readme content")
        self.repo.commit("Initial commit.")

    def _corrupt_files(self) -> List[pathlib.Path]:
        """Corrupt some files inside the mount.
        Returns relative paths to these files inside the mount.
        """
        # Corrupt 3 separate files.  2 are tracked by mercurial, one is not.
        # We will corrupt 2 of them by truncating the overlay file, and one by
        # completely removing the overlay file.  (In practice an unclean reboot often
        # leaves overlay files that exist but have 0 length.)
        tracked_path = pathlib.Path("src/committed_file")
        untracked_path = pathlib.Path("src/new_file")
        readme_path = pathlib.Path("readme.txt")

        tracked_overlay_file_path = self.overlay.materialize_file(tracked_path)
        untracked_overlay_file_path = self.overlay.materialize_file(untracked_path)
        readme_overlay_file_path = self.overlay.materialize_file(readme_path)

        self.eden.unmount(self.mount_path)
        os.truncate(tracked_overlay_file_path, 0)
        os.unlink(untracked_overlay_file_path)
        os.truncate(readme_overlay_file_path, 0)
        self.eden.mount(self.mount_path)

        return [tracked_path, untracked_path, readme_path]

    def test_unmount_succeeds(self) -> None:
        corrupted_paths = self._corrupt_files()

        # Access the files to make sure that edenfs loads them.
        # The stat calls should succeed, but reading them would fail.
        for path in corrupted_paths:
            os.lstat(str(self.mount_path / path))

        # Make sure that eden can successfully unmount the mount point
        # Previously we had a bug where the inode unloading code would throw an
        # exception if it failed to update the overlay state for some inodes.
        self.eden.unmount(self.mount_path)

    def test_unlink_deletes_corrupted_files(self) -> None:
        corrupted_paths = self._corrupt_files()
        for path in corrupted_paths:
            logging.info(f"stat()ing and unlinking {path}")
            full_path = self.mount_path / path
            s = os.lstat(str(full_path))
            self.assertTrue(stat.S_ISREG, s.st_mode)
            self.assertEqual(0, s.st_mode & 0o7777)
            self.assertEqual(0, s.st_size)
            full_path.unlink()
            self.assertFalse(
                full_path.exists(), f"{full_path} should not exist after being deleted"
            )

    def test_mount_possible_after_corrupt_directory_and_cached_next_inode_number(
        self
    ) -> None:
        test_dir_path = self.mount_path / "test_dir"
        test_dir_path.mkdir()
        test_dir_overlay_file_path = self.overlay.materialize_dir(test_dir_path)

        self.eden.unmount(self.mount_path)
        os.truncate(test_dir_overlay_file_path, 0)
        self.overlay.delete_cached_next_inode_number()

        self.eden.mount(self.mount_path)

    def test_eden_list_does_not_return_corrupt_mounts(self) -> None:
        self.eden.shutdown()

        # Truncate the overlay info file so edenfs will
        # not be able to open the overlay
        os.truncate(self.overlay.get_info_path(), 0)

        self.eden.start()
        self.assertEqual({str(self.mount): "NOT_RUNNING"}, self.eden.list_cmd_simple())
