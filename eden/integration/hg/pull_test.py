#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test("Flatmanifest", "Treemanifest", "TreeOnly")
class PullTest(EdenHgTestCase):

    def create_backing_repo(self):
        # Create a server repository first
        self.server_repo = self.create_server_repo()

        hgrc = self.get_hgrc()
        hgrc["paths"] = {"default": self.server_repo.path}
        repo = self.create_hg_repo("main", hgrc=hgrc)
        self.populate_backing_repo(repo)
        return repo

    def populate_backing_repo(self, repo):
        print("creating backing repo")
        repo.hg("pull")
        repo.hg("update", self.commit1)

    def create_server_repo(self):
        print("creating server repo")
        # Create a server repository.
        hgrc = self.get_hgrc()
        self.apply_hg_config_variant(hgrc)
        if self.config_variant_name in ("Treemanifest", "TreeOnly"):
            # fastmanifest must be disabled on the server repository.
            # The treemanifest server-side code breaks otherwise.
            hgrc["extensions"]["fastmanifest"] = "!"
            hgrc["treemanifest"] = {"server": "True", "autocreatetrees": "True"}

        repo = self.create_hg_repo("server_repo", hgrc=hgrc)

        # Create a commit in the server repository
        repo.write_file("hello.txt", "hola")
        repo.write_file("foo/bar.txt", "bar contents\n")
        repo.write_file("foo/test.txt", "test\n")
        repo.write_file("foo/subdir/test.txt", "test\n")
        repo.write_file("foo/subdir/main.c", 'printf("hello world\\n");\n')
        repo.write_file("src/deep/a/b/c/abc.txt", "abc\n")
        repo.write_file("src/deep/a/b/c/def.txt", "def\n")
        repo.write_file("src/deep/a/b/c/xyz.txt", "xyz\n")
        self.commit1 = repo.commit("Initial commit.\n")
        print("commit1=%s" % (self.commit1,))

        return repo

    def test_pull(self):
        self.assert_status_empty()
        self.assertEqual("test\n", self.read_file("foo/subdir/test.txt"))

        # Create a few new commits on the server
        self.server_repo.write_file(
            "foo/subdir/main.c", 'printf("hello world v2!\\n");\n'
        )
        self.commit2 = self.server_repo.commit("Commit 2\n")

        self.server_repo.write_file("foo/test.txt", "updated test\n")
        self.server_repo.write_file(
            "foo/subdir/main.c", 'printf("hello world v3!\\n");\n'
        )
        self.server_repo.write_file("src/myproject/main.py", 'print("hello")\n')
        self.commit3 = self.server_repo.commit("Commit 3\n")

        # Run "hg pull" inside the Eden checkout
        self.repo.hg("pull", stdout=None)

        # Update the Eden checkout to commit2
        self.repo.hg("update", self.commit2)
        self.assert_status_empty()
        self.assertEqual(
            'printf("hello world v2!\\n");\n', self.read_file("foo/subdir/main.c")
        )

        # Update the Eden checkout to commit3
        self.repo.hg("update", self.commit3)
        self.assert_status_empty()
        self.assertEqual(
            'printf("hello world v3!\\n");\n', self.read_file("foo/subdir/main.c")
        )

        # Create a 4th commit on the server
        self.server_repo.write_file("src/deep/a/b/c/xyz.txt", "xyz2\n")
        self.commit4 = self.server_repo.commit("Commit 4\n")

        # Pull and update the Eden checkout to the 4th commit.
        # This tests that the hg_import_helper can correctly see new data on
        # the server that was created after it first established its connection
        # to the server.
        self.repo.hg("pull", stdout=None)
        self.repo.hg("update", self.commit4)
        self.assert_status_empty()
        self.assertEqual(
            'printf("hello world v3!\\n");\n', self.read_file("foo/subdir/main.c")
        )
        self.assertEqual("xyz2\n", self.read_file("src/deep/a/b/c/xyz.txt"))
