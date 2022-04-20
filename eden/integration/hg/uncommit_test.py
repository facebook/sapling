#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class UncommitTest(EdenHgTestCase):
    commit1: str
    commit2: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello", "hello")
        self.commit1 = repo.commit("Initial commit.\n")

        repo.write_file("hello", "hola")
        self.commit2 = repo.commit("Second commit.\n")

    def test_uncommit(self) -> None:
        self.repo.run_hg("uncommit")
        self.assertEqual("hola", self.read_file("hello"))

    def test_uncommit_added_file(self) -> None:
        self.repo.write_file("added", "added")
        self.repo.commit("Third commit.\n")
        # Make sure that the file is dematerialized by updating to the parent
        # commit where it isn't present and then back, the file will then be a
        # virtual one.
        self.repo.run_hg("prev")
        self.repo.run_hg("next")
        self.repo.run_hg("uncommit")
        self.assertEqual("added", self.read_file("added"))
