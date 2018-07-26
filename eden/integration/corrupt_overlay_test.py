#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import logging
import os
import pathlib
import stat

import eden.integration.lib.overlay as overlay_mod
from eden.integration.lib import testcase


class CorruptOverlayTest(testcase.HgRepoTestMixin, testcase.EdenRepoTest):
    """Test file operations when Eden's overlay is corrupted."""

    def setUp(self) -> None:
        super().setUp()
        self.overlay = overlay_mod.OverlayStore(self.eden, self.mount_path)

    def populate_repo(self) -> None:
        self.repo.write_file("src/committed_file", "committed_file content")
        self.repo.write_file("readme.txt", "readme content")
        self.repo.commit("Initial commit.")

    def test_unmount_succeeds(self) -> None:
        # Materialized some files then corrupt their overlay state
        tracked_overlay_file_path = self.overlay.materialize_file("src/committed_file")
        untracked_overlay_file_path = self.overlay.materialize_file("src/new_file")

        self.eden.unmount(self.mount_path)
        os.truncate(tracked_overlay_file_path, 0)
        os.unlink(untracked_overlay_file_path)

        self.eden.mount(self.mount_path)
        self.eden.unmount(self.mount_path)

    def test_unlink_deletes_corrupted_files(self) -> None:
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

        for path in (tracked_path, untracked_path, readme_path):
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
