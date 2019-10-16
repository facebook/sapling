#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class RmTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("apple", "")
        repo.write_file("banana", "")
        repo.commit("first commit")

    def test_rm_file(self) -> None:
        self.hg("rm", "apple")
        self.assert_status({"apple": "R"})
        self.assertFalse(os.path.isfile(self.get_path("apple")))
        self.assertTrue(os.path.isfile(self.get_path("banana")))

    def test_rm_modified_file(self) -> None:
        self.write_file("apple", "new contents")

        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("rm", "apple")
        expected_msg = (
            "not removing apple: " "file is modified (use -f to force removal)"
        )
        self.assertIn(expected_msg, str(context.exception))
        self.assert_status({"apple": "M"})

        self.hg("rm", "--force", "apple")
        self.assert_status({"apple": "R"})
        self.assertFalse(os.path.isfile(self.get_path("apple")))
        self.assertTrue(os.path.isfile(self.get_path("banana")))

    def test_rm_modified_file_permissions(self) -> None:
        os.chmod(self.get_path("apple"), 0o755)

        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("rm", "apple")
        expected_msg = (
            "not removing apple: " "file is modified (use -f to force removal)"
        )
        self.assertIn(expected_msg, str(context.exception))
        self.assert_status({"apple": "M"})

        self.hg("rm", "--force", "apple")
        self.assert_status({"apple": "R"})
        self.assertFalse(os.path.isfile(self.get_path("apple")))
        self.assertTrue(os.path.isfile(self.get_path("banana")))

    def test_rm_directory(self) -> None:
        self.mkdir("dir")
        self.touch("dir/1")
        self.touch("dir/2")
        self.touch("dir/3")
        self.hg("add")
        self.repo.commit("second commit")

        self.hg("rm", "dir")
        self.assert_status({"dir/1": "R", "dir/2": "R", "dir/3": "R"})
        self.assertFalse(os.path.exists(self.get_path("dir")))

    def test_rm_directory_with_modification(self) -> None:
        self.mkdir("dir")
        self.touch("dir/1")
        self.touch("dir/2")
        self.touch("dir/3")
        self.hg("add")
        self.repo.commit("second commit")

        self.write_file("dir/2", "new contents")
        self.assert_status({"dir/2": "M"})

        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("rm", "dir")
        expected_msg = (
            "not removing dir/2: " "file is modified (use -f to force removal)"
        )
        self.assertIn(expected_msg, str(context.exception))
        self.assert_status({"dir/1": "R", "dir/2": "M", "dir/3": "R"})
        self.assertFalse(os.path.exists(self.get_path("dir/1")))
        self.assertTrue(os.path.exists(self.get_path("dir/2")))
        self.assertFalse(os.path.exists(self.get_path("dir/3")))
