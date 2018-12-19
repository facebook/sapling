#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import stat

import hypothesis
from eden.test_support.hypothesis import FILENAME_STRATEGY

from .lib import testcase


@testcase.eden_repo_test
class HypothesisSimpleTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        self.repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        self.repo.symlink("slink", "hello")
        self.repo.commit("Initial commit.")

    @hypothesis.given(FILENAME_STRATEGY)
    def test_create(self, basename: str) -> None:
        filename = os.path.join(self.mount, basename)

        # Ensure that we don't proceed if hypothesis has selected a name that
        # conflicts with the names we generated in the repo.
        hypothesis.assume(not os.path.exists(filename))

        with open(filename, "w") as f:
            f.write("created\n")

        self.assert_checkout_root_entries(
            {".eden", "adir", "bdir", "hello", basename, "slink"}
        )

        with open(filename, "r") as f:
            self.assertEqual(f.read(), "created\n")

        st = os.lstat(filename)
        self.assertEqual(st.st_size, 8)
        self.assertTrue(stat.S_ISREG(st.st_mode))
