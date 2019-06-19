#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess
from typing import List

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class FilesTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("README.md", "docs\n")
        repo.write_file("LICENSE", "legal legal\n")
        repo.write_file("src/main.cpp", "code\n")
        repo.write_file("src/lib.cpp", "more code\n")
        repo.write_file("src/stuff.cpp", "more code\n")
        repo.write_file("src/util.py", "utils\n")
        repo.write_file("src/lib/module.cpp", "module\n")
        repo.write_file("src/lib/foo.cpp", "foo\n")
        repo.write_file("src/include/stuff.h", "header\n")
        repo.write_file("test/test1.py", "test\n")
        repo.write_file("test/test2.py", "test\n")
        repo.commit("Initial commit.")

    def _assert_files(self, args: List[str], expected: List[str], cwd=None) -> None:
        stdout = self.hg("files", *args, cwd=cwd)
        results = stdout.splitlines()
        # `hg files` currently produces results in sorted order,
        # so we check for exact ordering here.
        self.assertEqual(expected, results)

    def test_all_files(self) -> None:
        self._assert_files(
            [],
            [
                "LICENSE",
                "README.md",
                "src/include/stuff.h",
                "src/lib.cpp",
                "src/lib/foo.cpp",
                "src/lib/module.cpp",
                "src/main.cpp",
                "src/stuff.cpp",
                "src/util.py",
                "test/test1.py",
                "test/test2.py",
            ],
        )

    def test_globs(self) -> None:
        self._assert_files(
            ["glob:src/*.cpp"], ["src/lib.cpp", "src/main.cpp", "src/stuff.cpp"]
        )

        self._assert_files(
            ["glob:**.cpp"],
            [
                "src/lib.cpp",
                "src/lib/foo.cpp",
                "src/lib/module.cpp",
                "src/main.cpp",
                "src/stuff.cpp",
            ],
        )

    def test_subdirectory(self) -> None:
        self._assert_files(
            [],
            [
                "../LICENSE",
                "../README.md",
                "include/stuff.h",
                "lib.cpp",
                "lib/foo.cpp",
                "lib/module.cpp",
                "main.cpp",
                "stuff.cpp",
                "util.py",
                "../test/test1.py",
                "../test/test2.py",
            ],
            cwd=os.path.join(self.repo.path, "src"),
        )

        self._assert_files(
            ["."],
            [
                "include/stuff.h",
                "lib.cpp",
                "lib/foo.cpp",
                "lib/module.cpp",
                "main.cpp",
                "stuff.cpp",
                "util.py",
            ],
            cwd=os.path.join(self.repo.path, "src"),
        )

        self._assert_files(
            ["glob:*.cpp"],
            ["lib.cpp", "main.cpp", "stuff.cpp"],
            cwd=os.path.join(self.repo.path, "src"),
        )

    def test_bad_matches(self) -> None:
        # No matches at all should return 1
        proc = self.repo.run_hg(
            "files",
            "foobar",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(proc.stdout, b"")
        self.assertEqual(proc.stderr, b"")
        self.assertEqual(proc.returncode, 1)

        # Some matching and some non-matching patterns returns 0
        # and does not print any diagnostics about the non-matching patterns.
        proc = self.repo.run_hg(
            "files",
            "foobar",
            "test",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(proc.stdout, b"test/test1.py\ntest/test2.py\n")
        self.assertEqual(proc.stderr, b"")
        self.assertEqual(proc.returncode, 0)

    def test_files_with_changes(self) -> None:
        self.write_file("src/new.cpp", "new file\n")
        self.hg("add", "src/new.cpp")
        self.hg("rm", "src/lib/foo.cpp")
        self.write_file("src/untracked.cpp", "should not be included\n")

        self._assert_files(
            [],
            [
                "LICENSE",
                "README.md",
                "src/include/stuff.h",
                "src/lib.cpp",
                "src/lib/module.cpp",
                "src/main.cpp",
                "src/new.cpp",
                "src/stuff.cpp",
                "src/util.py",
                "test/test1.py",
                "test/test2.py",
            ],
        )
        self._assert_files(
            ["glob:**.cpp"],
            [
                "src/lib.cpp",
                "src/lib/module.cpp",
                "src/main.cpp",
                "src/new.cpp",
                "src/stuff.cpp",
            ],
        )
