#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import shutil
import subprocess
from pathlib import Path
from typing import Dict, List, Optional, Sequence, Set, Tuple, Union

from eden.fs.service.eden.thrift_types import (
    CheckoutConflict,
    CheckoutMode,
    CheckOutRevisionParams,
    ScmFileStatus,
    SyncBehavior,
    TreeInodeDebugInfo,
)

from .lib import prjfs_test, testcase


@testcase.eden_repo_test
class WindowsFsckTest(prjfs_test.PrjFSTestBase):
    """Windows fsck integration tests"""

    initial_commit: str = ""

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("prjfs", []).append("fsck-detect-renames = true")
        return result

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("subdir/bdir/file", "foo!\n")
        self.repo.write_file("subdir/cdir/file", "foo!\n")
        self.repo.write_file(".gitignore", "ignored/\n")
        self.initial_commit = self.repo.commit("Initial commit.")

    def get_initial_commit(self) -> str:
        return self.initial_commit

    def select_storage_engine(self) -> str:
        return "sqlite"

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden.fs.inodes.sqlitecatalog": "DBG9"}

    async def test_detect_added_file_in_full_directory(self) -> None:
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

        await self.assertInStatus(b"foobar/foo", b"foobar/barfoo/baz")

    async def test_detect_added_files_in_ignored_full_directory(self) -> None:
        """Create a file in Full ignored directory when EdenFS is not running."""
        foobar = self.mount_path / "ignored" / "foobar"
        foobar.parent.mkdir()
        self.eden.shutdown()
        foobar.write_text("barfoo\n")
        self.eden.start()

        await self.assertInStatus(b"ignored/foobar")

    async def test_detect_removed_file_from_placeholder_directory_while_running(
        self,
    ) -> None:
        """Remove a file in placeholder directory when EdenFS is running."""
        afile = self.mount_path / "adir" / "file"

        afile.unlink()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED},
        )

        self.eden.shutdown()
        self.eden.start()

        self.assertFalse(afile.exists())
        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED},
        )

    async def test_detect_removed_file_from_dirty_placeholder_directory_while_running(
        self,
    ) -> None:
        """Remove a file in dirty placeholder directory when EdenFS is running."""
        afile = self.mount_path / "adir" / "file"
        new_file = self.mount_path / "adir" / "a_new_file"

        new_file.touch()
        afile.unlink()

        self.assertEqual(
            await self.eden_status(),
            {
                b"adir/a_new_file": ScmFileStatus.ADDED,
                b"adir/file": ScmFileStatus.REMOVED,
            },
        )

        self.eden.shutdown()
        self.eden.start()

        self.assertFalse(afile.exists())
        self.assertEqual(
            await self.eden_status(),
            {
                b"adir/a_new_file": ScmFileStatus.ADDED,
                b"adir/file": ScmFileStatus.REMOVED,
            },
        )

    async def test_detect_removed_file_from_full_directory_while_running(self) -> None:
        """Remove a file in Full directory when EdenFS is running."""
        foo = self.mount_path / "foobar" / "foo"
        foo.parent.mkdir()
        foo.write_text("hello!!")
        await self.assertInStatus(b"foobar/foo")
        foo.unlink()
        self.eden.shutdown()
        self.eden.start()
        self.assertFalse(foo.exists())
        await self.assertNotInStatus(b"foobar/foo")

    async def test_detect_removed_file_from_full_dir_matches_scm_not_empty_while_running(
        self,
    ) -> None:
        """
        Remove a file in full directory that happens to match a source
        control tree and is not empty on disk when EdenFS is running.
        """

        # Materialize the file and its parent by removing and re-creating them.
        self.rm("adir/file")
        self.rmdir("adir")
        self.write_file("adir/file", "foo!\n")
        self.write_file("adir/file2", "foo!\n")

        self.assertEqual(await self.eden_status(), {b"adir/file2": ScmFileStatus.ADDED})

        afile = self.mount_path / "adir" / "file"
        afile.unlink()

        self.eden.shutdown()
        self.eden.start()

        self.assertFalse(afile.exists())
        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"adir/file2": ScmFileStatus.ADDED},
        )

    async def test_detect_removed_file_from_full_dir_matches_scm_empty_while_running(
        self,
    ) -> None:
        """
        Remove a file in full directory that happens to match a source
        control tree and is not empty on disk when EdenFS is running.
        """

        # Materialize the file and its parent by removing and re-creating them.
        self.rm("adir/file")
        self.rmdir("adir")
        self.write_file("adir/file", "foo!\n")

        self.assertEqual(await self.eden_status(), {})

        afile = self.mount_path / "adir" / "file"
        afile.unlink()

        self.eden.shutdown()
        self.eden.start()

        self.assertFalse(afile.exists())
        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED},
        )

    # this test is currently broken because we magically "bring the file back"
    async def test_detect_removed_file_from_dirty_placeholder_directory(self) -> None:
        """Remove a file in placeholder directory when EdenFS is not running."""
        afile = self.mount_path / "adir" / "file"
        afile2 = self.mount_path / "adir" / "file2"
        afile2.touch()
        with open(afile, "r") as f:
            f.read()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file2": ScmFileStatus.ADDED},
        )

        self.eden.shutdown()

        afile.unlink()
        self.eden.start()

        self.assertFalse(afile.exists())
        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"adir/file2": ScmFileStatus.ADDED},
        )

    # this test is currently broken because we magically "bring the file back"
    async def test_detect_removed_file_from_placeholder_directory(self) -> None:
        """Remove a file in dirty placeholder directory when EdenFS is not running."""
        adir = self.mount_path / "adir"
        afile = adir / "file"
        with open(afile, "r") as f:
            f.read()

        self.assertEqual(
            await self.eden_status(),
            {},
        )

        self.eden.shutdown()

        afile.unlink()
        self.eden.start()

        self.assertFalse(afile.exists())
        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED},
        )

    async def test_detect_removed_file_from_full_directory(self) -> None:
        """Remove a file in Full directory when EdenFS is not running."""
        foo = self.mount_path / "foobar" / "foo"
        foo.parent.mkdir()
        foo.write_text("hello!!")
        status_dict = await self.eden_status(listIgnored=True)
        self.assertIn(b"foobar/foo", status_dict.keys())
        await self.assertInStatus(b"foobar/foo")
        self.eden.shutdown()
        foo.unlink()
        self.eden.start()
        self.assertFalse(foo.exists())
        await self.assertNotInStatus(b"foobar/foo")

    async def test_detect_removed_file_from_full_directory_scm_exists(self) -> None:
        """
        Remove a file in full directory that happens to match a tree
        in source control when EdenFS is not running.
        """

        # Materialize the file and its parent by removing and re-creating them.
        self.rm("adir/file")
        self.rmdir("adir")
        self.write_file("adir/file", "foo!\n")

        self.assertEqual(await self.eden_status(), {})

        afile = self.mount_path / "adir" / "file"
        self.eden.shutdown()
        afile.unlink()
        self.eden.start()

        self.assertFalse(afile.exists())
        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED},
        )

    async def test_fsck_not_readding_tombstone(self) -> None:
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
        await self.assertInStatus(b"hello", b"adir/file")

        self.eden.shutdown()
        # Tombstone should be visible now
        self.assertTrue((self.mount_path / "hello").exists())

        self.eden.start()
        # We should still see these files
        await self.assertInStatus(b"hello", b"adir/file")
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

    async def test_fsck_dirty_dir_checking(self) -> None:
        self.assertFalse(await self.eden_status())

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
            await self.eden_status(),
            {
                b"subdir/foo.txt": 0,
            },
        )

    async def test_fsck_junctions(self) -> None:
        subprocess.run(
            f"cmd.exe /c mklink /J {self.mount_path}\\bdir {self.mount_path}\\adir",
            check=True,
        )

        self.eden.shutdown()
        self.eden.start()

        self.assertEqual(await self.eden_status(), {b"bdir": 0})

    async def test_fsck_casing(self) -> None:
        afile = self.mount_path / "adir" / "file"
        afile.rename(self.mount_path / "adir" / "File")

        self.eden.shutdown()
        self.eden.start()

        self.assertEqual(await self.eden_status(), {})

    async def test_fsck_rename_with_different_case_while_stopped(self) -> None:
        # Materialize the file and its parent by removing and re-creating them.
        self.rm("adir/file")
        self.rmdir("adir")
        self.write_file("adir/file", "foo!\n")

        self.assertEqual(await self.eden_status(), {})

        afile = self.mount_path / "adir" / "file"

        self.eden.shutdown()
        afile.rename(self.mount_path / "adir" / "File")
        self.eden.start()

        self.assertEqual(await self.eden_status(), {})

    async def test_fsck_rename(self) -> None:
        afile = self.mount_path / "adir" / "file"
        afile.rename(self.mount_path / "adir" / "file-1")

        self.eden.shutdown()
        self.eden.start()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"adir/file-1": ScmFileStatus.ADDED},
        )

    async def test_fsck_rename_hydrated(self) -> None:
        afile = self.mount_path / "adir" / "file"
        moved_file = self.mount_path / "adir" / "file-1"
        afile.rename(moved_file)
        moved_file.read_bytes()

        self.eden.shutdown()
        self.eden.start()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"adir/file-1": ScmFileStatus.ADDED},
        )

    async def test_fsck_rename_full(self) -> None:
        afile = self.mount_path / "adir" / "file"
        moved_file = self.mount_path / "adir" / "file-1"
        afile.rename(moved_file)
        moved_file.write_bytes(b"blah")

        self.eden.shutdown()
        self.eden.start()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"adir/file-1": ScmFileStatus.ADDED},
        )

    async def test_fsck_rename_while_stopped_materialized(self) -> None:
        # Materialize the file and its parent by removing and re-creating them.
        self.rm("adir/file")
        self.rmdir("adir")
        self.write_file("adir/file", "foo!\n")

        self.assertEqual(await self.eden_status(), {})

        afile = self.mount_path / "adir" / "file"

        self.eden.shutdown()
        afile.rename(self.mount_path / "adir" / "file-1")
        self.eden.start()

        result = subprocess.run(
            ["eden", "debug", "prjfs-state", str(afile)], capture_output=True
        )
        print(result)

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"adir/file-1": ScmFileStatus.ADDED},
        )

    async def test_fsck_miss_rename(self) -> None:
        adir = self.mount_path / "adir"
        afile = adir / "file"

        os.listdir(adir)

        await self.make_eden_drop_all_notifications()

        afile.rename(adir / "file-1")

        self.eden.shutdown()

        self.eden.start()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"adir/file-1": ScmFileStatus.ADDED},
        )

    async def test_fsck_miss_remove_and_replace_with_rename(self) -> None:
        hello = self.mount_path / "hello"
        afile = self.mount_path / "adir" / "file"

        await self.make_eden_drop_all_notifications()

        hello.unlink()
        afile.rename(hello)

        self.eden.shutdown()

        self.eden.start()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"hello": ScmFileStatus.MODIFIED},
        )

    async def test_fsck_miss_remove_and_replace_with_rename_loaded(self) -> None:
        hello = self.mount_path / "hello"
        afile = self.mount_path / "adir" / "file"

        with hello.open() as f:
            f.read()

        await self.make_eden_drop_all_notifications()

        hello.unlink()
        afile.rename(hello)

        self.eden.shutdown()

        self.eden.start()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"hello": ScmFileStatus.MODIFIED},
        )

    async def test_fsck_miss_remove_and_replace_with_rename_materialized(self) -> None:
        hello = self.mount_path / "hello"
        afile = self.mount_path / "adir" / "file"

        with hello.open("w") as f:
            f.write("")

        await self.make_eden_drop_all_notifications()

        hello.unlink()
        afile.rename(hello)

        self.eden.shutdown()

        self.eden.start()

        self.assertEqual(
            await self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED, b"hello": ScmFileStatus.MODIFIED},
        )

    async def test_fsck_miss_remove_dir_and_replace_with_rename(self) -> None:
        hello = self.mount_path / "hello"
        adir = self.mount_path / "adir"
        afile = adir / "file"

        os.listdir(adir)

        await self.make_eden_drop_all_notifications()

        afile.unlink()
        adir.rmdir()
        hello.rename(adir)

        self.eden.shutdown()

        self.eden.start()

        self.assertEqual(
            await self.eden_status(),
            {
                b"adir": ScmFileStatus.ADDED,
                b"adir/file": ScmFileStatus.REMOVED,
                b"hello": ScmFileStatus.REMOVED,
            },
        )

    async def _update_clean(self) -> Sequence[CheckoutConflict]:
        async with self.get_thrift_client() as client:
            conflicts = await client.checkOutRevision(
                mountPoint=self.mount.encode(),
                snapshotHash=self.initial_commit.encode(),
                checkoutMode=CheckoutMode.FORCE,
                params=CheckOutRevisionParams(),
            )
        return conflicts

    async def test_fsck_rename_with_different_case_and_modify_while_stopped(
        self,
    ) -> None:
        # Materialize the file and its parent by removing and re-creating them.
        self.rm("adir/file")
        self.rmdir("adir")
        self.write_file("adir/file", "foo!\n")

        afile = self.mount_path / "adir" / "file"

        self.eden.shutdown()
        afile.rename(self.mount_path / "adir" / "File")
        self.write_file("adir/File", "Bar\n")
        self.eden.start()

        self.assertEqual(await self.eden_status(), {b"adir/file": 1})

        # Make sure we can revert the change:
        await self._update_clean()
        self.assertEqual(await self.eden_status(), {})

    async def test_loaded_inodes_not_loaded_on_restart(self) -> None:
        """Verifies that a loaded inode not present on disk doesn't get loaded
        with a positive refcount on restart.
        """

        async def get_all_loaded_under(path: str) -> List[Tuple[Path, int]]:
            async with self.get_thrift_client() as client:
                all_loaded = await client.debugInodeStatus(
                    self.mount_path_bytes,
                    path.encode(),
                    0,
                    sync=SyncBehavior(),
                )

            ret: List[Tuple[Path, int]] = []
            for loaded in all_loaded:
                ret += [(Path(loaded.path.decode()), loaded.refcount)]

            return ret

        # This relies on debugInodeStatus to load the inode for the directory.
        loaded = await get_all_loaded_under("subdir/bdir")
        self.assertEqual(loaded, [(Path("subdir/bdir"), 0)])

        self.eden.shutdown()
        self.eden.start()

        loaded = await get_all_loaded_under("subdir/bdir")
        self.assertEqual(loaded, [(Path("subdir/bdir"), 0)])


MATERIALIZED = True
UNMATERIALIZED = False
FILE_MODE = 32768
DIR_MODE = 16384


@testcase.eden_repo_test
class WindowsRebuildOverlayTest(testcase.EdenRepoTest):
    """Windows fsck integration tests"""

    initial_commit: str = ""

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden.fs.inodes.sqlitecatalog.WindowsFsck": "DBG9"}

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

    async def _eden_inode_info(self) -> Sequence[TreeInodeDebugInfo]:
        async with self.get_thrift_client() as client:
            return await client.debugInodeStatus(
                mountPoint=self.mount.encode(),
                path=b"",
                # Force it to return all inodes, not just loaded ones.
                flags=1,
                sync=SyncBehavior(),
            )

    async def get_inodes(self) -> Set[Tuple[str, int, bool, bytes]]:
        inodes = await self._eden_inode_info()
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

    async def stop_eden(self) -> Set[Tuple[str, int, bool, bytes]]:
        preInodes = await self.get_inodes()
        self.eden.shutdown()
        self.assertTrue(preInodes)
        return preInodes

    async def start_eden(self) -> Set[Tuple[str, int, bool, bytes]]:
        self.eden.start()
        postInodes = await self.get_inodes()
        return postInodes

    async def rebuild_overlay(
        self, from_backup=False
    ) -> Dict[str, Tuple[Union[bool, bytes, int, str], ...]]:
        preInodes = await self.stop_eden()

        if from_backup:
            self.restore_overlay()
        else:
            # Clear the overlay
            os.unlink(
                os.path.join(
                    self.eden_dir, "clients", self.repo_name, "local", "treestore.db"
                )
            )

        postInodes = await self.start_eden()
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

    async def test_rebuild_entire_overlay(self) -> None:
        # Test a not yet loaded overlay
        await self.rebuild_overlay()

        # Test an empty overlay
        self.repo.update("null")
        self.repo.update(self.initial_commit)
        await self.rebuild_overlay()

        # Test a partially materialized overlay
        self.read_file("subdir/bdir/file2")
        await self.rebuild_overlay()

        # Test with non-tracked changes
        self.write_file("subdir/bdir/untracked", "asdf")
        await self.rebuild_overlay()

    async def test_rebuild_partial_overlay(self) -> None:
        # Clear then partially load the overlay
        self.repo.update("null")
        self.repo.update(self.initial_commit)
        self.read_file("subdir/bdir/file2")

        initialInodes = await self.get_inodes()
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

        inodes = await self.rebuild_overlay(from_backup=True)
        self.assertEqual(inodes["file1"][1], UNMATERIALIZED)
        self.assertTrue(".gitignore" not in inodes)
        self.assertEqual(inodes["untracked"][1], MATERIALIZED)
        self.assertEqual(inodes["subdir"][1], MATERIALIZED)
        self.assertEqual(inodes["cdir"][1], UNMATERIALIZED)
        self.assertEqual(inodes["hello"][:2], (DIR_MODE, MATERIALIZED))
        self.assertEqual(inodes["innerfile"][:2], (FILE_MODE, MATERIALIZED))
        self.assertEqual(inodes["otherdir"][:2], (DIR_MODE, MATERIALIZED))
