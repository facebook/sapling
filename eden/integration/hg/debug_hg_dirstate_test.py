#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from textwrap import dedent

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class DebugHgDirstateTest(EdenHgTestCase):
    def populate_backing_repo(self, repo):
        repo.write_file("rootfile.txt", "")
        repo.commit("Initial commit.")

    def test_empty_hg_dirstate(self):
        output = self.eden.run_cmd("debug", "hg_dirstate", cwd=self.mount)
        expected = dedent(
            """\
        Non-normal Files (0):
        Copymap (0):
        """
        )
        self.assertEqual(expected, output)

    def test_hg_dirstate_with_modified_files(self):
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

    def test_hg_dirstate_with_copies(self):
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
