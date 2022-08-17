#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import shutil
import sys
import unittest
from typing import Dict, List, Optional, Set, Tuple, Union

from facebook.eden.ttypes import GetScmStatusParams, SyncBehavior, TreeInodeDebugInfo

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

    def test_fsck_dirty_dir_checking(self) -> None:
        self.assertFalse(self._eden_status())

        subdir = self.mount_path / "subdir"
        bdir = subdir / "bdir"
        filepath = subdir / "foo.txt"
        with open(filepath, "w+") as f:
            f.write("asdf")
        # Load subdir and bdirs contents so the fsck will into the dirty subdir
        # and the non-dirty bdir.
        list(subdir.iterdir())
        list(bdir.iterdir())

        # T129264761 involved restarting eden causing directories below dirty
        # directories to get deleted. Let's run status and verify that there
        # are no pending changes aside from the foo.txt we created.
        self.eden.shutdown()
        self.eden.start()

        self.assertEqual(
            self._eden_status(),
            {
                b"subdir/foo.txt": 0,
            },
        )


MATERIALIZED = True
UNMATERIALIZED = False
FILE_MODE = 32768
DIR_MODE = 16384


@testcase.eden_repo_test
class WindowsRebuildOverlayTest(testcase.EdenRepoTest):
    """Windows fsck integration tests"""

    initial_commit: str = ""

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden.fs.inodes.treeoverlay.TreeOverlayWindowsFsck": "DBG9"}

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        return {"fsck": ["use-thorough-fsck=true"]}

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file1", "foo!\n")
        self.repo.write_file("subdir/bdir/file2", "foo!\n")
        self.repo.write_file("subdir/cdir/file3", "foo!\n")
        self.repo.write_file(".gitignore", "ignored/\n")
        self.repo.write_file("otherdir/file4", "foo!/\n")
        self.initial_commit = self.repo.commit("Initial commit.")

    def select_storage_engine(self) -> str:
        return "sqlite"

    def _eden_inode_info(self) -> List[TreeInodeDebugInfo]:
        with self.eden.get_thrift_client_legacy() as client:
            return client.debugInodeStatus(
                mountPoint=self.mount.encode(),
                path=b"",
                # Force it to return all inodes, not just loaded ones.
                flags=1,
                sync=SyncBehavior(),
            )

    def get_inodes(self) -> Set[Tuple[str, int, bool, bytes]]:
        inodes = self._eden_inode_info()
        entries = set()
        ignored = {
            b".hg",
        }
        for inode in inodes:
            if inode.path in ignored:
                continue
            for entry in inode.entries:
                if entry.name in ignored:
                    continue
                entries.add(
                    (
                        entry.name.decode("utf8"),
                        entry.mode,
                        entry.materialized,
                        entry.hash,
                    )
                )
        return entries

    def stop_eden(self) -> Set[Tuple[str, int, bool, bytes]]:
        preInodes = self.get_inodes()
        self.eden.shutdown()
        self.assertTrue(preInodes)
        return preInodes

    def start_eden(self) -> Set[Tuple[str, int, bool, bytes]]:
        self.eden.start()
        postInodes = self.get_inodes()
        return postInodes

    def rebuild_overlay(
        self, from_backup=False
    ) -> Dict[str, Tuple[Union[bool, bytes, int, str], ...]]:
        preInodes = self.stop_eden()

        if from_backup:
            self.restore_overlay()
        else:
            # Clear the overlay
            os.unlink(
                os.path.join(
                    self.eden_dir, "clients", self.repo_name, "local", "treestore.db"
                )
            )

        postInodes = self.start_eden()
        if preInodes != postInodes:
            print("PreInodes: %s" % preInodes)
        self.assertEqual(preInodes, postInodes)

        # Make a dict for easy access to verify individual files.
        return {inode[0]: inode[1:] for inode in postInodes}

    def backup_overlay(self) -> None:
        path = os.path.join(
            self.eden_dir, "clients", self.repo_name, "local", "treestore.db"
        )
        backup = os.path.join(
            self.eden_dir, "clients", self.repo_name, "local", "treestore.db.bak"
        )
        shutil.copy(path, backup)

    def restore_overlay(self) -> None:
        path = os.path.join(
            self.eden_dir, "clients", self.repo_name, "local", "treestore.db"
        )
        backup = os.path.join(
            self.eden_dir, "clients", self.repo_name, "local", "treestore.db.bak"
        )
        shutil.copy(backup, path)

    def test_rebuild_entire_overlay(self) -> None:
        # Test a not yet loaded overlay
        self.rebuild_overlay()

        # Test an empty overlay
        self.repo.update("null")
        self.repo.update(self.initial_commit)
        self.rebuild_overlay()

        # Test a partially materialized overlay
        self.read_file("subdir/bdir/file2")
        self.rebuild_overlay()

        # Test with non-tracked changes
        self.write_file("subdir/bdir/untracked", "asdf")
        self.rebuild_overlay()

    def test_rebuild_partial_overlay(self) -> None:
        # Clear then partially load the overlay
        self.repo.update("null")
        self.repo.update(self.initial_commit)
        self.read_file("subdir/bdir/file2")

        initialInodes = self.get_inodes()
        initialInodes = {inode[0]: inode[1:] for inode in initialInodes}
        self.assertEqual(initialInodes["subdir"][1], UNMATERIALIZED)

        # Create a backup of the overlay. Later we'll restore this backup
        # to simulate a crash where the overlay was out of date since the
        # OverlayBuffer hadn't been flushed yet.
        self.backup_overlay()

        # Test loading a file, but not changing it (i.e. not materializing)
        self.assertEqual(self.read_file("adir/file1"), "foo!\n")

        # Test changing a file to a directory
        self.rm("hello")
        self.write_file("hello/innerfile", "asdf")

        # Test adding an untracked file
        self.write_file("subdir/untracked", "asdf")

        # Test deleting a file
        self.rm(".gitignore")

        # Test deleting a file in a directory
        self.rm("otherdir/file4")

        inodes = self.rebuild_overlay(from_backup=True)
        self.assertEqual(inodes["file1"][1], UNMATERIALIZED)
        self.assertTrue(".gitignore" not in inodes)
        self.assertEqual(inodes["untracked"][1], MATERIALIZED)
        self.assertEqual(inodes["subdir"][1], MATERIALIZED)
        self.assertEqual(inodes["cdir"][1], UNMATERIALIZED)
        self.assertEqual(inodes["hello"][:2], (DIR_MODE, MATERIALIZED))
        self.assertEqual(inodes["innerfile"][:2], (FILE_MODE, MATERIALIZED))
        self.assertEqual(inodes["otherdir"][:2], (DIR_MODE, MATERIALIZED))
