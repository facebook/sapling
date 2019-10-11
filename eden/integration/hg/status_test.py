#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from eden.integration.lib.hgrepo import HgRepository

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test("Treemanifest", "TreeOnly")
# pyre-fixme[38]: `StatusTest` does not implement all inherited abstract methods.
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
# pyre-fixme[38]: `StatusRevertTest` does not implement all inherited abstract methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class StatusRevertTest(EdenHgTestCase):
    # pyre-fixme[13]: Attribute `commit1` is never initialized.
    commit1: str
    # pyre-fixme[13]: Attribute `commit2` is never initialized.
    commit2: str
    # pyre-fixme[13]: Attribute `commit3` is never initialized.
    commit3: str
    # pyre-fixme[13]: Attribute `commit4` is never initialized.
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
