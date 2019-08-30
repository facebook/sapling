#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from textwrap import dedent

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[38]: `CopyTest` does not implement all inherited abstract methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class CopyTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        repo.commit("Initial commit.\n")

    def test_copy_file_within_directory(self) -> None:
        self.hg("copy", "hello.txt", "goodbye.txt")
        self.assert_status({"goodbye.txt": "A"})
        extended_status = self.hg("status", "--copies")
        self.assertEqual(
            dedent(
                """\
        A goodbye.txt
          hello.txt
        """
            ),
            extended_status,
        )
        self.assert_copy_map({"goodbye.txt": "hello.txt"})

        self.repo.commit("Commit copied file.\n")
        self.assert_status_empty()
        self.assert_copy_map({})

    def test_copy_file_then_revert_it(self) -> None:
        self.hg("copy", "hello.txt", "goodbye.txt")
        self.assert_status({"goodbye.txt": "A"})
        self.assert_copy_map({"goodbye.txt": "hello.txt"})

        self.hg("revert", "--no-backup", "--all")
        self.assert_status({"goodbye.txt": "?"})
        self.assert_copy_map({})

        self.hg("add", "goodbye.txt")
        extended_status = self.hg("status", "--copies")
        self.assertEqual(
            dedent(
                """\
        A goodbye.txt
        """
            ),
            extended_status,
        )
