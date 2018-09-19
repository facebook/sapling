#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from eden.integration.snapshot.snapshot import HgSnapshot, snapshot_class


@snapshot_class(
    "basic",
    "A simple directory structure with a mix of loaded, materialized, "
    "and unloaded files.",
)
class BaseSnapshot(HgSnapshot):
    def populate_backing_repo(self) -> None:
        repo = self.backing_repo
        repo.write_file("README.md", "project docs")
        repo.write_file("src/main.c", 'printf("hello world!\\n");\n')
        repo.write_file("src/lib.c", "void do_stuff() {}\n")
        repo.write_file("src/test/test.c", 'printf("success!\\n");\n')
        repo.write_file("include/lib.h", "void do_stuff();\n")
        repo.write_file("other/foo.txt", "foo\n")
        repo.write_file("other/bar.txt", "bar\n")
        repo.write_file("other/a/b/c.txt", "abc\n")
        repo.commit("Initial commit.")

    def populate_checkout(self) -> None:
        # Load the src directory and the src/lib.c file
        self.list_dir("src")
        self.read_file("src/lib.c")
        # Modify src/test/test.c to force it to be materialized
        self.write_file("src/test/test.c", b"new test contents")
