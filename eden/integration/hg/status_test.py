#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import binascii
import os
import socket
import stat
import sys
from typing import Dict

from eden.integration.lib.hgrepo import HgRepository
from facebook.eden.ttypes import (
    EdenError,
    EdenErrorType,
    GetScmStatusParams,
    ScmFileStatus,
    ScmStatus,
)

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class StatusTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        repo.write_file("subdir/file.txt", "contents")
        repo.commit("Initial commit.")

    def test_status(self) -> None:
        """Test various `hg status` states in the root of an Eden mount."""
        self.assert_status_empty()

        self.touch("world.txt")
        self.assert_status({"world.txt": "?"})

        self.hg("add", "world.txt")
        self.assert_status({"world.txt": "A"})

        self.rm("hello.txt")
        self.assert_status({"hello.txt": "!", "world.txt": "A"})

        with open(self.get_path("hello.txt"), "w") as f:
            f.write("new contents")
        self.assert_status({"hello.txt": "M", "world.txt": "A"})

        self.hg("forget", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("rm", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        # If the file is already forgotten, `hg rm` does not remove it from
        # disk.
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("add", "hello.txt")
        self.assert_status({"hello.txt": "M", "world.txt": "A"})
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("rm", "--force", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        self.assertFalse(os.path.exists(self.get_path("hello.txt")))

    def thoroughly_get_scm_status(
        self, client, mountPoint, commit, listIgnored, expected_status
    ) -> None:
        status_from_get_scm_status = client.getScmStatus(
            mountPoint=bytes(mountPoint, encoding="utf-8"),
            commit=commit,
            listIgnored=False,
        )
        status_from_get_scm_status_v2 = client.getScmStatusV2(
            GetScmStatusParams(
                mountPoint=bytes(mountPoint, encoding="utf-8"),
                commit=commit,
                listIgnored=False,
            )
        ).status

        self.assertEqual(
            status_from_get_scm_status,
            status_from_get_scm_status_v2,
            "getScmStatus and getScmStatusV2 should agree",
        )

    def test_status_thrift_apis(self) -> None:
        """Test both the getScmStatusV2() and getScmStatus() thrift APIs."""
        # This confirms that both thrift APIs continue to work,
        # independently of the one currently used by hg.
        initial_commit_hex = self.repo.get_head_hash()
        initial_commit = binascii.unhexlify(initial_commit_hex)

        with self.get_thrift_client_legacy() as client:
            # Test with a clean status.
            expected_status = ScmStatus(entries={}, errors={})
            self.thoroughly_get_scm_status(
                client, self.mount, initial_commit, False, expected_status
            )

            # Modify the working directory and then test again
            self.repo.write_file("hello.txt", "saluton")
            self.touch("new_tracked.txt")
            self.hg("add", "new_tracked.txt")
            self.touch("untracked.txt")
            expected_entries = {
                b"hello.txt": ScmFileStatus.MODIFIED,
                b"new_tracked.txt": ScmFileStatus.ADDED,
                b"untracked.txt": ScmFileStatus.ADDED,
            }
            expected_status = ScmStatus(entries=expected_entries, errors={})
            self.thoroughly_get_scm_status(
                client, self.mount, initial_commit, False, expected_status
            )

            # Commit the modifications
            self.repo.commit("committing changes")

    def test_status_with_non_parent(self) -> None:
        # This confirms that an error is thrown if getScmStatusV2 is called
        # with a commit that is not the parent commit
        initial_commit_hex = self.repo.get_head_hash()
        initial_commit = binascii.unhexlify(initial_commit_hex)
        config = """\
["hg"]
enforce-parents = false
"""
        edenrc = os.path.join(self.home_dir, ".edenrc")

        with self.get_thrift_client_legacy() as client:
            # Add file to commit
            self.touch("new_tracked.txt")
            self.hg("add", "new_tracked.txt")

            # Commit the modifications
            self.repo.commit("committing changes")

            # Test calling getScmStatusV2() with a commit that is not the parent commit
            with self.assertRaises(EdenError) as context:
                client.getScmStatusV2(
                    GetScmStatusParams(
                        mountPoint=bytes(self.mount, encoding="utf-8"),
                        commit=initial_commit,
                        listIgnored=False,
                    )
                )
            self.assertEqual(
                EdenErrorType.OUT_OF_DATE_PARENT, context.exception.errorType
            )

            with open(edenrc, "w") as f:
                f.write(config)

            # Makes sure that EdenFS picks up our updated config,
            # since we wrote it out after EdenFS started.
            client.reloadConfig()

            try:
                client.getScmStatusV2(
                    GetScmStatusParams(
                        mountPoint=bytes(self.mount, encoding="utf-8"),
                        commit=initial_commit,
                        listIgnored=False,
                    )
                )
            except EdenError as ex:
                self.fail(
                    "getScmStatusV2 threw after setting enforce-parents to false with {}".format(
                        ex
                    )
                )

    def test_manual_revert(self) -> None:
        self.assert_status_empty()
        self.write_file("dir1/a.txt", "original contents\n")
        self.hg("add", "dir1/a.txt")
        self.repo.commit("create a.txt")
        self.assert_status_empty()

        self.write_file("dir1/a.txt", "updated contents\n")
        self.repo.commit("modify a.txt")
        self.assert_status_empty()

        self.write_file("dir1/a.txt", "original contents\n")
        self.repo.commit("revert a.txt")
        self.assert_status_empty()

    def test_truncation_upon_open_modifies_file(self) -> None:
        fd = os.open(os.path.join(self.mount, "subdir/file.txt"), os.O_TRUNC)
        try:
            self.assert_status({"subdir/file.txt": "M"})
        finally:
            os.close(fd)

    def test_truncation_after_open_modifies_file(self) -> None:
        fd = os.open(os.path.join(self.mount, "subdir/file.txt"), os.O_WRONLY)
        try:
            os.ftruncate(fd, 0)
            self.assert_status({"subdir/file.txt": "M"})
        finally:
            os.close(fd)

    def test_partial_truncation_after_open_modifies_file(self) -> None:
        fd = os.open(os.path.join(self.mount, "subdir/file.txt"), os.O_WRONLY)
        try:
            os.ftruncate(fd, 1)
            self.assert_status({"subdir/file.txt": "M"})
        finally:
            os.close(fd)

    def test_irrelevant_chmod_is_ignored_by_status(self) -> None:
        path = os.path.join(self.mount, "hello.txt")
        mode = os.lstat(path).st_mode
        mode |= stat.S_IXGRP
        os.chmod(path, mode)
        self.assert_status_empty()

    def test_rename_materialized(self) -> None:
        self.write_file("subdir1/file.txt", "contents")
        self.assert_status({"subdir1/file.txt": "?"})

        subdir1 = os.path.join(self.mount, "subdir1")
        subdir2 = os.path.join(self.mount, "subdir2")
        os.rename(subdir1, subdir2)
        self.assert_status({"subdir2/file.txt": "?"})

    def test_status_socket(self) -> None:
        if sys.platform == "win32":
            from eden.thrift.windows_thrift import WindowsSocketHandle  # @manual

            uds = WindowsSocketHandle()
        else:
            uds = socket.socket(family=socket.AF_UNIX)

        uds.bind(os.path.join(self.mount, "socket"))
        uds.close()
        self.assert_status({"socket": "?"})


@hg_test
# pyre-ignore[13]: T62487924
class StatusEdgeCaseTest(EdenHgTestCase):
    commit1: str
    commit2: str

    def populate_backing_repo(self, repo: HgRepository) -> None:
        repo.write_file("subdir/file.txt", "contents")
        self.commit1 = repo.commit("commit 1")
        repo.write_file("subdir/file.txt", "contents", mode=0o775)
        self.commit2 = repo.commit("commit 2")
        self.assertNotEqual(self.commit1, self.commit2)

    def select_storage_engine(self) -> str:
        """we need to persist data across restarts"""
        return "sqlite"

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {
            "eden.strace": "DBG7",
            "eden.fs.inodes.TreeInode": "DBG9",
        }

    @EdenHgTestCase.unix_only
    def test_file_created_with_relevant_mode_difference_and_then_fixed_is_ignored(
        self,
    ) -> None:
        self.repo.update(self.commit1)
        path = os.path.join(self.mount, "subdir", "file.txt")
        os.unlink(path)
        fd = os.open(path, os.O_CREAT | os.O_WRONLY, mode=0o775)
        try:
            os.write(fd, b"contents")
        finally:
            os.close(fd)

        self.assert_status({"subdir/file.txt": "M"})
        os.chmod(path, 0o664)
        self.assert_status_empty()
        self.repo.update(self.commit2)
        self.eden.restart()
        self.assert_status_empty()

    @EdenHgTestCase.unix_only
    def test_dematerialized_file_created_with_different_mode_is_unchanged(self) -> None:
        path = os.path.join(self.mount, "subdir", "file.txt")
        # save inode numbers and initial dtype
        os.lstat(path)
        # materialize and remove executable bit
        os.chmod(path, 0o664)
        self.assert_status({"subdir/file.txt": "M"})
        # make an untracked file so the checkout doesn't reallocate inodes
        os.close(os.open(os.path.join(self.mount, "subdir", "sibling"), os.O_CREAT))
        self.repo.update(self.commit1, merge=True)
        # put the old contents back
        os.unlink(os.path.join(self.mount, "subdir", "sibling"))
        self.assert_status_empty()
        self.eden.restart()
        os.chmod(os.path.join(self.mount, "subdir"), 0o664)
        self.assert_status_empty()


# Define a separate TestCase class purely to test with different initial
# repository contents.
@hg_test
# pyre-ignore[13]: T62487924
class StatusRevertTest(EdenHgTestCase):
    commit1: str
    commit2: str
    commit3: str
    commit4: str

    def populate_backing_repo(self, repo: HgRepository) -> None:
        repo.write_file("dir1/a.txt", "original contents of a\n")
        repo.write_file("dir1/b.txt", "b.txt\n")
        repo.write_file("dir1/c.txt", "c.txt\n")
        repo.write_file("dir2/x.txt", "x.txt\n")
        repo.write_file("dir2/y.txt", "y.txt\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("dir1/a.txt", "updated contents of a\n", add=False)
        self.commit2 = repo.commit("commit 2")

        repo.write_file("dir1/b.txt", "updated b\n", add=False)
        self.commit3 = repo.commit("commit 3")

        repo.write_file("dir1/a.txt", "original contents of a\n")
        self.commit4 = repo.commit("commit 4")

    def test_reverted_contents(self) -> None:
        self.assert_status_empty()
        # Read dir1/a.txt so it is loaded by edenfs
        self.read_file("dir1/a.txt")

        # Reset the state from commit4 to commit1 without actually doing a
        # checkout.  dir1/a.txt has the same contents in commit4 as in commit1,
        # but different blob hashes.
        self.hg("reset", "--keep", self.commit1)
        # Only dir1/b.txt should be reported as modified.
        # dir1/a.txt should not show up in the status output.
        self.assert_status({"dir1/b.txt": "M"})
