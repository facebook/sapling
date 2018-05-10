#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class StatusTest(EdenHgTestCase):

    def populate_backing_repo(self, repo):
        repo.write_file("hello.txt", "hola")
        repo.commit("Initial commit.")

    def test_status(self):
        """Test various `hg status` states in the root of an Eden mount."""
        self.assert_status_empty()

        self.touch("world.txt")
        self.assert_status({"world.txt": "?"})

        self.hg("add", "world.txt")
        self.assert_status({"world.txt": "A"})

        self.rm("hello.txt")
        self.assert_status({"hello.txt": "!", "world.txt": "A"})

        with open(self.get_path("hello.txt"), "w") as f:
            f.write("new contents")
        self.assert_status({"hello.txt": "M", "world.txt": "A"})

        self.hg("forget", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("rm", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        # If the file is already forgotten, `hg rm` does not remove it from
        # disk.
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("add", "hello.txt")
        self.assert_status({"hello.txt": "M", "world.txt": "A"})
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("rm", "--force", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        self.assertFalse(os.path.exists(self.get_path("hello.txt")))


# Define a separate TestCase class purely to test with different initial
# repository contents.
@hg_test
class StatusRevertTest(EdenHgTestCase):

    def populate_backing_repo(self, repo):
        repo.write_file("dir1/a.txt", "original contents of a\n")
        repo.write_file("dir1/b.txt", "b.txt\n")
        repo.write_file("dir1/c.txt", "c.txt\n")
        repo.write_file("dir2/x.txt", "x.txt\n")
        repo.write_file("dir2/y.txt", "y.txt\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("dir1/a.txt", "updated contents of a\n", add=False)
        self.commit2 = repo.commit("commit 2")

        repo.write_file("dir1/b.txt", "updated b\n", add=False)
        self.commit3 = repo.commit("commit 3")

        repo.write_file("dir1/a.txt", "original contents of a\n")
        self.commit4 = repo.commit("commit 4")

    def test_reverted_contents(self):
        self.assert_status_empty()
        # Read dir1/a.txt so it is loaded by edenfs
        self.read_file("dir1/a.txt")

        # Reset the state from commit4 to commit1 without actually doing a
        # checkout.  dir1/a.txt has the same contents in commit4 as in commit1,
        # but different blob hashes.
        self.hg("reset", "--keep", self.commit1)
        # Only dir1/b.txt should be reported as modified.
        # dir1/a.txt should not show up in the status output.
        self.assert_status({"dir1/b.txt": "M"})
