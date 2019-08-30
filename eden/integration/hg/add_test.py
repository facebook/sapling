#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[38]: `AddTest` does not implement all inherited abstract methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class AddTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("rootfile.txt", "")
        repo.write_file("dir1/a.txt", "original contents")
        repo.commit("Initial commit.")

    def test_add(self) -> None:
        self.touch("dir1/b.txt")
        self.mkdir("dir2")
        self.touch("dir2/c.txt")
        self.assert_status({"dir1/b.txt": "?", "dir2/c.txt": "?"})
        self.assert_dirstate_empty()

        # `hg add dir2` should ensure only things under dir2 are added.
        self.hg("add", "dir2")
        self.assert_status({"dir1/b.txt": "?", "dir2/c.txt": "A"})
        self.assert_dirstate({"dir2/c.txt": ("a", 0, "MERGE_BOTH")})

        # This is the equivalent of `hg forget dir1/a.txt`.
        self.hg("rm", "--force", "dir1/a.txt")
        self.write_file("dir1/a.txt", "original contents")
        self.touch("dir1/a.txt")
        self.assert_status({"dir1/a.txt": "R", "dir1/b.txt": "?", "dir2/c.txt": "A"})

        # Running `hg add .` should remove the removal marker from dir1/a.txt
        # because dir1/a.txt is still on disk.
        self.hg("add")
        self.assert_status({"dir1/b.txt": "A", "dir2/c.txt": "A"})

        self.hg("rm", "dir1/a.txt")
        self.write_file("dir1/a.txt", "different contents")
        # Running `hg add dir1` should remove the removal marker from
        # dir1/a.txt, but `hg status` should also reflect that it is modified.
        self.hg("add", "dir1")
        self.assert_status({"dir1/a.txt": "M", "dir1/b.txt": "A", "dir2/c.txt": "A"})

        self.hg("rm", "--force", "dir1/a.txt")
        # This should not add dir1/a.txt back because it is not on disk.
        self.hg("add", "dir1")
        self.assert_status({"dir1/a.txt": "R", "dir1/b.txt": "A", "dir2/c.txt": "A"})

    def test_add_nonexistent_directory(self) -> None:
        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.hg("add", "dir3")
        self.assertEqual(
            "dir3: No such file or directory\n",
            context.exception.stderr.decode("utf-8"),
        )
        self.assertEqual(1, context.exception.returncode)

    def test_try_replacing_directory_with_file(self) -> None:
        # `hg rm` the only file in a directory, which should also remove the
        # directory.
        self.hg("rm", "dir1/a.txt")
        self.assert_status({"dir1/a.txt": "R"})
        self.assertFalse(os.path.exists(self.get_path("dir1")))

        # Create an ordinary file with the same name as the directory that was
        # removed and `hg add` it.
        self.write_file("dir1", "Now I am an ordinary file.\n")
        self.assert_status({"dir1": "?", "dir1/a.txt": "R"})

        self.hg("add", "dir1")  # Currently, this throws an exception.
        self.assert_status({"dir1": "A", "dir1/a.txt": "R"})

    def test_add_file_that_would_normally_be_ignored(self) -> None:
        self.write_file("somefile.bak", "Backup file.\n")
        self.assert_status({"somefile.bak": "?"})
        self.write_file(".gitignore", "*.bak\n")

        self.assert_status({".gitignore": "?", "somefile.bak": "I"})
        self.assert_status({".gitignore": "?"}, check_ignored=False)

        self.hg("add", "somefile.bak")
        self.assert_status({".gitignore": "?", "somefile.bak": "A"})

        self.rm("somefile.bak")
        self.assert_status({".gitignore": "?", "somefile.bak": "!"})

        self.hg("forget", "somefile.bak")
        self.assert_status({".gitignore": "?"})

        # Also try if somefile.bak is a symlink
        os.symlink(b"symlink contents", os.path.join(self.mount, "somefile.bak"))
        self.assert_status({".gitignore": "?", "somefile.bak": "I"})
        self.hg("add", "somefile.bak")
        self.assert_status({".gitignore": "?", "somefile.bak": "A"})

    def test_add_ignored_directory_has_no_effect(self) -> None:
        self.write_file(".gitignore", "ignored_directory\n")
        self.hg("add", ".gitignore")
        self.repo.commit("Add .gitignore.\n")
        self.assert_status_empty()

        self.mkdir("ignored_directory")
        self.assert_status_empty()

        self.write_file("ignored_directory/one.txt", "1\n")
        self.write_file("ignored_directory/two.txt", "2\n")
        self.assert_status_empty(check_ignored=False)

        self.hg("add", "ignored_directory")
        self.assert_status_empty(
            check_ignored=False,
            msg="Even though the directory was explicitly passed to `hg add`, "
            "it should not be added.",
        )

        self.hg("add", "ignored_directory/two.txt")
        self.assert_status(
            {"ignored_directory/two.txt": "A"},
            check_ignored=False,
            msg="Explicitly adding a file in an ignored directory "
            "should take effect.",
        )

        self.hg("add", "ignored_directory")
        self.assert_status(
            {"ignored_directory/two.txt": "A"},
            check_ignored=False,
            msg="Even though one file in ignored_directory has been added, "
            "calling `hg add` on the directory "
            "should not add the other ignored file.",
        )

        self.rm("ignored_directory/two.txt")
        self.assert_status(
            {"ignored_directory/one.txt": "I", "ignored_directory/two.txt": "!"}
        )

        self.hg("forget", "ignored_directory/two.txt")
        self.assert_status({"ignored_directory/one.txt": "I"})

        self.write_file("ignored_directory/two.txt", "2\n")
        self.assert_status(
            {"ignored_directory/one.txt": "I", "ignored_directory/two.txt": "I"}
        )

    def test_debugdirstate(self) -> None:
        self.touch("dir1/b.txt")
        self.mkdir("dir2")
        self.touch("dir2/c.txt")
        self.assert_status({"dir1/b.txt": "?", "dir2/c.txt": "?"})

        self.assertEqual(
            self.repo.hg("debugdirstate"),
            "",
            'Filesystem changes with no "hg add" commands should not '
            "update the dirstate",
        )

        self.hg("add", "dir2")
        self.assertEqual(
            self.repo.hg("debugdirstate"),
            "a   0   MERGE_BOTH dir2/c.txt\n",
            'Calling "hg add" should update the dirstate',
        )

        self.rm("dir1/a.txt")
        self.hg("add", "dir2")
        self.assertEqual(
            self.repo.hg("debugdirstate"),
            "a   0   MERGE_BOTH dir2/c.txt\n",
            "Removing a file without forgetting it should not " "update the dirstate",
        )

        self.hg("forget", "dir1/a.txt")
        self.assertEqual(
            self.repo.hg("debugdirstate"),
            "r   0              dir1/a.txt\n" "a   0   MERGE_BOTH dir2/c.txt\n",
        )

    def test_rebuild_dirstate(self) -> None:
        self.touch("dir1/b.txt")
        self.mkdir("dir2")
        self.touch("dir2/c.txt")
        self.hg("add", "dir2")
        self.assert_dirstate({"dir2/c.txt": ("a", 0, "MERGE_BOTH")})
        self.assert_status({"dir1/b.txt": "?", "dir2/c.txt": "A"})

        self.repo.hg("debugrebuilddirstate")
        self.assert_dirstate_empty()
        self.assert_status({"dir1/b.txt": "?", "dir2/c.txt": "?"})
