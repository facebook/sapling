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
class DebugHgGetDirstateTupleTest(EdenHgTestCase):

    def populate_backing_repo(self, repo):
        repo.write_file("hello", "hola\n")
        repo.write_file("dir/file", "blah\n")
        repo.commit("Initial commit.")

    def test_get_dirstate_tuple_normal_file(self):
        output = self.eden.run_cmd(
            "debug", "hg_get_dirstate_tuple", self.get_path("hello")
        )
        expected = dedent(
            """\
        hello
            status = Normal
            mode = 0o100644
            mergeState = NotApplicable
        """
        )
        self.assertEqual(expected, output)
