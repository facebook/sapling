#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class DiffTest(EdenHgTestCase):
    def populate_backing_repo(self, repo):
        repo.write_file("rootfile.txt", "")
        repo.write_file("dir1/a.txt", "original contents\n")
        repo.commit("Initial commit.")

    def check_output(self, output, expected_lines):
        output_lines = output.splitlines()
        self.assertListEqual(output_lines, expected_lines)

    def test_modify_file(self):
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

    def test_add_file(self):
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
