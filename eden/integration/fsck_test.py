#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
from typing import Type

from .lib import repobase, testcase


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

    def create_repo(self, name: str) -> Type[repobase.Repository]:
        return self.create_hg_repo("main")

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        self.client_dir = os.readlink(os.path.join(self.mount, ".eden", "client"))
        self.overlay_dir = os.path.join(self.client_dir, "local")

    def _update_file(self, path: str, contents: str) -> int:
        """
        Update a file by path and return its inode number.

        Updating the file contents ensures it will be materialized and present in the
        overlay.
        """
        with open(os.path.join(self.mount, path), "w") as f:
            f.write(contents)
            stat_info = os.fstat(f.fileno())
        return stat_info.st_ino

    def _get_overlay_path(self, inode_number: int) -> str:
        subdir = "{:02x}".format(inode_number % 256)
        return os.path.join(self.overlay_dir, subdir, str(inode_number))

    def test_fsck_no_issues(self) -> None:
        output = self.eden.run_cmd("fsck", self.mount)
        self.assertIn("No issues found", output)

    def test_fsck_empty_overlay_file(self) -> None:
        inode_number = self._update_file("doc/foo.txt", "new contents\n")
        self.eden.run_cmd("unmount", self.mount)

        # Truncate the file to 0 length
        with open(self._get_overlay_path(inode_number), "w"):
            pass

        self.eden.run_cmd("mount", self.mount)

        cmd_result = self.eden.run_unchecked("fsck", self.mount)
        self.assertEqual(cmd_result, 1)
