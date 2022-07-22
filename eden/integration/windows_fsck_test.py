#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import sys
import unittest
from typing import Dict, List, Optional

from facebook.eden.ttypes import GetScmStatusParams

from .lib import testcase


@testcase.eden_repo_test
class WindowsFsckTest(testcase.EdenRepoTest):
    """Windows fsck integration tests"""

    initial_commit: str = ""

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("subdir/bdir/file", "foo!\n")
        self.repo.write_file("subdir/cdir/file", "foo!\n")
        self.repo.write_file(".gitignore", "ignored/\n")
        self.initial_commit = self.repo.commit("Initial commit.")

    def select_storage_engine(self) -> str:
        return "sqlite"

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden.fs.inodes.treeoverlay": "DBG9"}

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        return {"overlay": ["enable_tree_overlay=true"]}

    def _eden_status(self, listIgnored: bool = False):
        with self.eden.get_thrift_client_legacy() as client:
            status = client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=self.mount.encode(),
                    commit=self.initial_commit.encode(),
                    listIgnored=listIgnored,
                )
            )
            return status.status.entries

    def _assertInStatus(self, *files) -> None:
        status = self._eden_status(listIgnored=True).keys()
        for filename in files:
            self.assertIn(filename, status)

    def _assertNotInStatus(self, *files) -> None:
        status = self._eden_status(listIgnored=True).keys()
        for filename in files:
            self.assertNotIn(filename, status)

    def test_detect_added_file_in_full_directory(self) -> None:
        """
        Create a new directory when EdenFS is running, then add files to it
        when EdenFS is not running.
        """
        foobar = self.mount_path / "foobar"
        foobar.mkdir()
        # `foobar` is a Full directory in this case
        self.eden.shutdown()
        # Create a file
        (foobar / "foo").write_text("foo!!")
        # Create a subdirectory
        (foobar / "barfoo").mkdir()
        (foobar / "barfoo" / "baz").write_text("baz")
        self.eden.start()

        self._assertInStatus(b"foobar/foo", b"foobar/barfoo/baz")

    def test_detect_added_files_in_ignored_full_directory(self) -> None:
        """Create a file in Full ignored directory when EdenFS is not running."""
        foobar = self.mount_path / "ignored" / "foobar"
        foobar.parent.mkdir()
        self.eden.shutdown()
        foobar.write_text("barfoo\n")
        self.eden.start()

        self._assertInStatus(b"ignored/foobar")

    def test_detect_removed_file_from_full_directory(self) -> None:
        """Remove a file in Full directory when EdenFS is not running."""
        foo = self.mount_path / "foobar" / "foo"
        foo.parent.mkdir()
        foo.write_text("hello!!")
        self.assertIn(b"foobar/foo", self._eden_status(listIgnored=True).keys())
        self._assertInStatus(b"foobar/foo")
        self.eden.shutdown()
        foo.unlink()
        self.eden.start()
        self._assertNotInStatus(b"foobar/foo")

    def test_fsck_not_readding_tombstone(self) -> None:
        """
        Negative case: after user removes an entry while EdenFS is running,
        ProjectedFS will place a special Tombstone marker in place of that
        entry, and it is only visible when EdenFS is not running.

        In this test, we make sure FSCK does not incorrectly re-add the
        Tombstone as if they are untracked files.
        """
        (self.mount_path / "hello").unlink()
        (self.mount_path / "adir" / "file").unlink()
        (self.mount_path / "adir").rmdir()
        self._assertInStatus(b"hello", b"adir/file")

        self.eden.shutdown()
        # Tombstone should be visible now
        self.assertTrue((self.mount_path / "hello").exists())

        self.eden.start()
        # We should still see these files
        self._assertInStatus(b"hello", b"adir/file")
        # Tombstone should be invisible now
        self.assertFalse((self.mount_path / "hello").exists())

    def test_fsck_not_removing_existing_entry_under_placehold(self) -> None:
        """
        Negative case: ProjectedFS will remove untouched entries under
        DirtyPlaceholder directories when EdenFS is not running. As a result,
        we should not consider these as deleted by the user.
        """
        # We have to do this test in a subdirectory as entries under root is
        # always visible.
        subdir = self.mount_path / "subdir"
        # Create a directory so the parent directory now becomes a DirtyPlaceholder
        (subdir / "foobar").mkdir()
        bdir = subdir / "bdir"
        # We can't directly check the existence of the directory as it will
        # materialize the directory to disk
        self.assertIn(bdir, list(subdir.iterdir()))
        self.eden.shutdown()
        # bdir should be invisible when EdenFS is running
        self.assertNotIn(bdir, list(subdir.iterdir()))
        self.eden.start()
        # bdir should be visible when EdenFS is running
        self.assertIn(bdir, list(subdir.iterdir()))
