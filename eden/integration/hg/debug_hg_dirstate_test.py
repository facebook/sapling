#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from textwrap import dedent

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[38]: `DebugHgDirstateTest` does not implement all inherited abstract
#  methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class DebugHgDirstateTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("rootfile.txt", "")
        repo.commit("Initial commit.")

    def test_empty_hg_dirstate(self) -> None:
        output = self.eden.run_cmd("debug", "hg_dirstate", cwd=self.mount)
        expected = dedent(
            """\
        Non-normal Files (0):
        Copymap (0):
        """
        )
        self.assertEqual(expected, output)

    def test_hg_dirstate_with_modified_files(self) -> None:
        self.write_file("a.txt", "")
        self.write_file("b.txt", "")
        self.write_file("c.txt", "")
        self.write_file("d.txt", "")
        self.hg("add")
        self.hg("rm", "rootfile.txt")

        output = self.eden.run_cmd("debug", "hg_dirstate", cwd=self.mount)
        expected = dedent(
            """\
        Non-normal Files (5):
        a.txt
            status = MarkedForAddition
            mode = 0o0
            mergeState = BothParents
        b.txt
            status = MarkedForAddition
            mode = 0o0
            mergeState = BothParents
        c.txt
            status = MarkedForAddition
            mode = 0o0
            mergeState = BothParents
        d.txt
            status = MarkedForAddition
            mode = 0o0
            mergeState = BothParents
        rootfile.txt
            status = MarkedForRemoval
            mode = 0o0
            mergeState = NotApplicable
        Copymap (0):
        """
        )
        self.assertEqual(expected, output)

    def test_hg_dirstate_with_copies(self) -> None:
        self.hg("copy", "rootfile.txt", "root1.txt")
        self.hg("copy", "rootfile.txt", "root2.txt")

        output = self.eden.run_cmd("debug", "hg_dirstate", cwd=self.mount)
        expected = dedent(
            """\
        Non-normal Files (2):
        root1.txt
            status = MarkedForAddition
            mode = 0o0
            mergeState = BothParents
        root2.txt
            status = MarkedForAddition
            mode = 0o0
            mergeState = BothParents
        Copymap (2):
        rootfile.txt -> root1.txt
        rootfile.txt -> root2.txt
        """
        )
        self.assertEqual(expected, output)
