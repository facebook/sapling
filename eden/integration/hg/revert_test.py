#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class RevertTest(EdenHgTestCase):
    initial_commit: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        self.initial_commit = repo.commit("Initial commit.\n")

    def test_make_local_change_and_attempt_revert(self) -> None:
        self.write_file("hello.txt", "hello")
        self.assert_status({"hello.txt": "M"})
        self.hg("revert", "--no-backup", "hello.txt")
        self.assert_status_empty()
        txt_contents = self.read_file("hello.txt")
        self.assertEqual("hola", txt_contents)

    def test_revert_during_merge_resolution_succeeds(self) -> None:
        self.write_file("hello.txt", "one")
        c1 = self.repo.commit("c1")

        self.repo.update(self.initial_commit)
        self.write_file("hello.txt", "two")
        c2 = self.repo.commit("c2")

        with self.assertRaises(hgrepo.HgError):
            self.hg("rebase", "-r", c2, "-d", c1)
        self.assert_unresolved(unresolved=["hello.txt"])
        self.hg("revert", "-r", self.initial_commit, "hello.txt")
