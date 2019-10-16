#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class RevertTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        repo.commit("Initial commit.\n")

    def test_make_local_change_and_attempt_revert(self) -> None:
        self.write_file("hello.txt", "hello")
        self.assert_status({"hello.txt": "M"})
        self.hg("revert", "--no-backup", "hello.txt")
        self.assert_status_empty()
        txt_contents = self.read_file("hello.txt")
        self.assertEqual("hola", txt_contents)
