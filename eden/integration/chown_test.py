#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from .lib import repobase, testcase


@testcase.eden_test
# pyre-ignore[13]: T62487924
class ChownTest(testcase.EdenRepoTest):
    nobody_uid: int
    nobody_gid: int

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
        import grp
        import pwd

        super().setup_eden_test()
        self.nobody_uid = pwd.getpwnam("nobody").pw_uid
        self.nobody_gid = grp.getgrnam("nobody").gr_gid

    def assert_path(self, path: str) -> None:
        stat = os.lstat(path)
        self.assertEqual(
            stat.st_uid,
            self.nobody_uid,
            f"{stat.st_uid} uid does not match expected \
            {self.nobody_uid} for path {path}",
        )
        self.assertEqual(
            stat.st_gid,
            self.nobody_gid,
            f"{stat.st_gid} gid does not match expected \
            {self.nobody_gid} for path {path}",
        )

    def assert_chown_worked(self, mount: str) -> None:
        for root, dirs, files in os.walk(mount, followlinks=False):
            # Avoid checking anything in .eden since the
            # symlinks don't have o+r permissions
            if root.endswith(".eden"):
                continue
            for d in dirs:
                self.assert_path(os.path.join(root, d))
            for f in files:
                self.assert_path(os.path.join(root, f))

    def run_chown(self, mount: str, use_ids: bool = False) -> None:
        if use_ids:
            output = self.eden.run_cmd(
                "chown", mount, str(self.nobody_uid), str(self.nobody_gid)
            )
        else:
            output = self.eden.run_cmd("chown", mount, "nobody", "nobody")
        print(output)

    def test_chown(self) -> None:
        self.run_chown(self.mount, use_ids=True)
        self.assert_chown_worked(self.mount)

    def test_chown_with_overlay(self) -> None:
        with open(os.path.join(self.mount, "notinrepo"), "w") as f:
            f.write("created\n")

        self.run_chown(self.mount)
        self.assert_chown_worked(self.mount)

    def test_chown_with_bindmount(self) -> None:
        self.eden.run_cmd("redirect", "add", "buck-out", "bind", "--mount", self.mount)

        with open(os.path.join(self.mount, "buck-out", "bindmountedfile"), "w") as f:
            f.write("created\n")

        self.run_chown(self.mount)
        self.assert_chown_worked(self.mount)
