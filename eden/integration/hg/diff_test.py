#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import List

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[38]: `DiffTest` does not implement all inherited abstract methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class DiffTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("rootfile.txt", "")
        repo.write_file("dir1/a.txt", "original contents\n")
        repo.commit("Initial commit.")

    def check_output(self, output: str, expected_lines: List[str]):
        output_lines = output.splitlines()
        self.assertListEqual(output_lines, expected_lines)

    def test_modify_file(self) -> None:
        self.write_file("dir1/a.txt", "new line\noriginal contents\n")
        diff_output = self.hg("diff")
        expected_lines = [
            "diff --git a/dir1/a.txt b/dir1/a.txt",
            "--- a/dir1/a.txt",
            "+++ b/dir1/a.txt",
            "@@ -1,1 +1,2 @@",
            "+new line",
            " original contents",
        ]
        self.check_output(diff_output, expected_lines)

    def test_add_file(self) -> None:
        self.write_file("dir1/b.txt", "new file\n1234\n5678\n")
        self.hg("add", "dir1/b.txt")
        diff_output = self.hg("diff")
        expected_lines = [
            "diff --git a/dir1/b.txt b/dir1/b.txt",
            "new file mode 100644",
            "--- /dev/null",
            "+++ b/dir1/b.txt",
            "@@ -0,0 +1,3 @@",
            "+new file",
            "+1234",
            "+5678",
        ]
        self.check_output(diff_output, expected_lines)
