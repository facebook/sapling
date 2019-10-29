#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import binascii
import os

from eden.integration.lib.hgrepo import HgRepository
from facebook.eden.ttypes import (
    EdenError,
    EdenErrorType,
    GetScmStatusParams,
    ScmFileStatus,
    ScmStatus,
)

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test("TreeOnly")
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
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

        with self.get_thrift_client() as client:
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

        with self.get_thrift_client() as client:
            # Add file to commit
            self.touch("new_tracked.txt")
            self.hg("add", "new_tracked.txt")

            # Commit the modifications
            self.repo.commit("committing changes")

            # Test calling getScmStatusV2() with a commit that is not the parent commit
            error_regex = (
                "error computing status: requested parent commit is "
                + "out-of-date: requested .*, but current parent commit is .*"
            )
            with self.assertRaisesRegex(EdenError, error_regex) as context:
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


# Define a separate TestCase class purely to test with different initial
# repository contents.
@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
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
