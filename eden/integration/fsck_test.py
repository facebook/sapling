#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from .lib import overlay, repobase, testcase


class FsckTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("README.md", "tbd\n")
        self.repo.write_file("proj/src/main.c", "int main() { return 0; }\n")
        self.repo.write_file("proj/src/lib.c", "void foo() {}\n")
        self.repo.write_file("proj/src/include/lib.h", "#pragma once\nvoid foo();\n")
        self.repo.write_file(
            "proj/test/test.sh", "#!/bin/bash\necho test\n", mode=0o755
        )
        self.repo.write_file("doc/foo.txt", "foo\n")
        self.repo.write_file("doc/bar.txt", "bar\n")
        self.repo.symlink("proj/doc", "../doc")
        self.repo.commit("Initial commit.")

    def create_repo(self, name: str) -> repobase.Repository:
        return self.create_hg_repo("main")

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        self.overlay = overlay.OverlayStore(self.eden, self.mount_path)

    def test_fsck_no_issues(self) -> None:
        output = self.eden.run_cmd("fsck", self.mount)
        self.assertIn("No issues found", output)

    def test_fsck_empty_overlay_file(self) -> None:
        overlay_path = self.overlay.materialize_file("doc/foo.txt")
        self.eden.run_cmd("unmount", self.mount)

        # Truncate the file to 0 length
        with overlay_path.open("wb"):
            pass

        self.eden.run_cmd("mount", self.mount)

        cmd_result = self.eden.run_unchecked("fsck", self.mount)
        self.assertEqual(1, cmd_result.returncode)
