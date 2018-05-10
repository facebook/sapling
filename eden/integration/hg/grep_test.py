#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from textwrap import dedent

from ..lib import hgrepo
from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class GrepTest(EdenHgTestCase):

    def populate_backing_repo(self, repo):
        repo.write_file("file_in_root.txt", "\n".join(["apple", "  banana", "cat"]))
        repo.write_file("d1/d2/afile.txt", "\n".join(["banana", "  banana"]))
        repo.write_file("d1/d2/bfile.txt", "\n".join(["    banana", "cat", "dog"]))
        repo.commit("Initial commit.")

    def test_grep_file(self):
        stdout = self.hg("grep", "-n", "banana", "file_in_root.txt")
        self.assertEqual("file_in_root.txt:2:  banana\n", stdout)

    def test_grep_directory_from_root(self):
        stdout = self.hg("grep", "-n", "banana", "d1/d2")
        expected = dedent(
            """\
        d1/d2/afile.txt:1:banana
        d1/d2/afile.txt:2:  banana
        d1/d2/bfile.txt:1:    banana
        """
        )

        self.assertEqual(expected, stdout)

    def test_grep_directory_from_subdirectory(self):
        stdout = self.hg("grep", "-n", "banana", "d2", cwd=self.get_path("d1"))
        expected = dedent(
            """\
        d2/afile.txt:1:banana
        d2/afile.txt:2:  banana
        d2/bfile.txt:1:    banana
        """
        )

        self.assertEqual(expected, stdout)

    def test_grep_that_does_not_match_anything(self):
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("grep", "NOT IN THERE")
        self.assertEqual(b"", context.exception.stdout)
        self.assertEqual(b"", context.exception.stderr)
        self.assertEqual(123, context.exception.returncode)

    def test_grep_that_does_not_match_anything_in_directory(self):
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("grep", "NOT IN THERE", "d1")
        self.assertEqual(b"", context.exception.stdout)
        self.assertEqual(b"", context.exception.stderr)
        self.assertEqual(123, context.exception.returncode)
