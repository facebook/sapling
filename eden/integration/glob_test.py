#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging
from typing import List, Optional, Tuple

from facebook.eden.ttypes import EdenError, EdenErrorType, GlobParams

from .lib import testcase


@testcase.eden_repo_test
class GlobTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "bonjour\n")
        self.repo.write_file("hola", "hello\n")
        self.repo.write_file("adir/phile", "phoo!\n")

        self.commit0 = self.repo.commit("Commit 0.")

        self.repo.remove_files(["hello", "hola", "adir/phile"])
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/file", "bar!\n")
        self.repo.write_file("bdir/otherfile", "foo!\n")
        self.repo.symlink("slink", "hello")
        self.repo.write_file("cdir/subdir/new.txt", "and improved")
        self.repo.write_file("ddir/notdotfile", "")
        self.repo.write_file("ddir/subdir/notdotfile", "")
        self.repo.write_file("ddir/subdir/.dotfile", "")

        self.repo.write_file("java/com/example/package.html", "")
        self.repo.write_file("java/com/example/Example.java", "")
        self.repo.write_file("java/com/example/foo/Foo.java", "")
        self.repo.write_file("java/com/example/foo/bar/Bar.java", "")
        self.repo.write_file("java/com/example/foo/bar/baz/Baz.java", "")

        self.commit1 = self.repo.commit("Commit 1.")

    def setUp(self) -> None:
        # needs to be done before set up because these need to be created
        # for populate_repo() and the supers set up will call this.
        self.commit0 = ""
        self.commit1 = ""

        super().setUp()

        self.client = self.get_thrift_client()
        self.client.open()
        self.addCleanup(self.client.close)

    def test_exact_path_component_match(self) -> None:
        self.assert_glob(["hello"], ["hello"])
        self.assert_glob(["ddir/subdir/.dotfile"], ["ddir/subdir/.dotfile"])

    def test_wildcard_path_component_match(self) -> None:
        self.assert_glob(["hel*"], ["hello"])
        self.assert_glob(["ad*"], ["adir"])
        self.assert_glob_with_dtypes(["ad*"], [("adir", "d")])
        self.assert_glob(["a*/file"], ["adir/file"])
        self.assert_glob_with_dtypes(["a*/file"], [("adir/file", "f")])

    def test_no_accidental_substring_match(self) -> None:
        self.assert_glob(["hell"], [], msg="No accidental substring match")

    def test_match_all_files_in_directory(self) -> None:
        self.assert_glob(["bdir/*"], ["bdir/file", "bdir/otherfile"])

    def test_match_all_files_in_directory_with_dotfile(self) -> None:
        self.assert_glob(["ddir/subdir/*"], ["ddir/subdir/notdotfile"])

    def test_overlapping_globs(self) -> None:
        self.assert_glob(
            ["adir/*", "**/file"],
            ["adir/file", "bdir/file"],
            msg="De-duplicate results from multiple globs",
        )

    def test_recursive_wildcard_prefix(self) -> None:
        self.assert_glob(["**/file"], ["adir/file", "bdir/file"])

    def test_recursive_wildcard_suffix(self) -> None:
        self.assert_glob(["adir/**"], ["adir/file"])
        self.assert_glob(["adir/**/*"], ["adir/file"])

    def test_recursive_wildcard_suffix_with_dotfile(self) -> None:
        self.assert_glob(
            ["ddir/**"], ["ddir/notdotfile", "ddir/subdir", "ddir/subdir/notdotfile"]
        )
        self.assert_glob(
            ["ddir/**"],
            [
                "ddir/notdotfile",
                "ddir/subdir",
                "ddir/subdir/.dotfile",
                "ddir/subdir/notdotfile",
            ],
            include_dotfiles=True,
        )

        self.assert_glob(
            ["ddir/**/*"], ["ddir/notdotfile", "ddir/subdir", "ddir/subdir/notdotfile"]
        )
        self.assert_glob(
            ["ddir/**/*"],
            [
                "ddir/notdotfile",
                "ddir/subdir",
                "ddir/subdir/.dotfile",
                "ddir/subdir/notdotfile",
            ],
            include_dotfiles=True,
        )

    def test_qualified_recursive_wildcard(self) -> None:
        self.assert_glob(
            ["java/com/**/*.java"],
            [
                "java/com/example/Example.java",
                "java/com/example/foo/Foo.java",
                "java/com/example/foo/bar/Bar.java",
                "java/com/example/foo/bar/baz/Baz.java",
            ],
        )
        self.assert_glob(
            ["java/com/example/*/*.java"], ["java/com/example/foo/Foo.java"]
        )

    def test_malformed_query(self) -> None:
        with self.assertRaises(EdenError) as ctx:
            self.client.globFiles(
                GlobParams(mountPoint=self.mount_path_bytes, globs=["adir["])
            )
        self.assertIn("unterminated bracket sequence", str(ctx.exception))
        self.assertEqual(EdenErrorType.POSIX_ERROR, ctx.exception.errorType)

        with self.assertRaises(EdenError) as ctx:
            self.client.globFiles(GlobParams(self.mount_path_bytes, ["adir["], True))
        self.assertIn("unterminated bracket sequence", str(ctx.exception))
        self.assertEqual(EdenErrorType.POSIX_ERROR, ctx.exception.errorType)

    def test_globs_may_not_include_dotdot(self):
        with self.assertRaises(EdenError) as ctx:
            self.client.globFiles(
                GlobParams(self.mount_path_bytes, ["java/../java/com/**/*.java"])
            )
        self.assertEqual(
            "Invalid glob (PathComponent must not be ..): java/../java/com/**/*.java",
            str(ctx.exception),
        )
        self.assertEqual(EdenErrorType.ARGUMENT_ERROR, ctx.exception.errorType)

    def test_glob_on_non_current_commit(self) -> None:
        self.assert_glob(["hello"], ["hello"], commits=[bytes.fromhex(self.commit0)])
        self.assert_glob(["hola"], ["hola"], commits=[bytes.fromhex(self.commit0)])

    def test_glob_multiple_commits(self) -> None:
        self.assert_glob(
            ["hello"],
            ["hello", "hello"],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )
        self.assert_glob(
            ["h*"],
            ["hello", "hello", "hola"],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )
        self.assert_glob(
            ["a*/*ile"],
            ["adir/file", "adir/phile"],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )

    def test_prefetch_matching_files(self) -> None:
        self.assert_glob(["hello"], ["hello"], prefetching=True)
        self.assert_glob(
            ["hello"],
            ["hello"],
            prefetching=True,
            commits=[bytes.fromhex(self.commit0)],
        )
        self.assert_glob(
            ["hello"],
            ["hello", "hello"],
            prefetching=True,
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )

    def test_simple_matching_commit(self) -> None:
        self.assert_glob(
            ["hello"],
            expected_matches=["hello"],
            expected_commits=[bytes.fromhex(self.commit1)],
        )

        self.assert_glob(
            ["hello"],
            expected_matches=["hello"],
            expected_commits=[bytes.fromhex(self.commit0)],
            commits=[bytes.fromhex(self.commit0)],
        )

    def test_duplicate_file_multiple_commits(self) -> None:
        self.assert_glob(
            ["hello"],
            expected_matches=["hello", "hello"],
            expected_commits=[
                bytes.fromhex(self.commit0),
                bytes.fromhex(self.commit1),
            ],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )

        def test_multiple_file_multiple_commits(self) -> None:
            self.assert_glob(
                ["a*/*ile"],
                [b"adir/file", b"adir/phile"],
                expected_commits=[
                    bytes.fromhex(self.commit1),
                    bytes.fromhex(self.commit0),
                ],
                commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
            )

    def assert_glob(
        self,
        globs: List[str],
        expected_matches: List[str],
        include_dotfiles: bool = False,
        msg: Optional[str] = None,
        commits: Optional[List[bytes]] = None,
        prefetching: bool = False,
        expected_commits: Optional[List[bytes]] = None,
    ) -> None:
        params = GlobParams(
            mountPoint=self.mount_path_bytes,
            globs=globs,
            includeDotfiles=include_dotfiles,
            prefetchFiles=prefetching,
            revisions=commits,
        )
        result = self.client.globFiles(params)
        path_results = (
            path.decode("utf-8", errors="surrogateescape")
            for path in result.matchingFiles
        )
        self.assertEqual(expected_matches, sorted(path_results), msg=msg)
        self.assertFalse(result.dtypes)

        if expected_commits:
            self.assertCountEqual(
                expected_commits, self.client.globFiles(params).originHashes, msg=msg
            )

    def assert_glob_with_dtypes(
        self,
        globs: List[str],
        expected_matches: List[Tuple[str, str]],
        include_dotfiles: bool = False,
        msg: Optional[str] = None,
    ) -> None:
        params = GlobParams(
            self.mount_path_bytes,
            globs,
            includeDotfiles=include_dotfiles,
            wantDtype=True,
        )
        result = self.client.globFiles(params)
        actual_results = zip(
            (
                path.decode("utf-8", errors="surrogateescape")
                for path in result.matchingFiles
            ),
            (_dtype_to_str(dtype) for dtype in result.dtypes),
        )
        self.assertEqual(expected_matches, sorted(actual_results), msg=msg)


# Mac and Linux fortunately appear to share the same dtype definitions
_DT_DIR = 4
_DT_REG = 8
_DT_LNK = 10


def _dtype_to_str(value: int) -> str:
    if value == _DT_REG:
        return "f"
    elif value == _DT_DIR:
        return "d"
    elif value == _DT_LNK:
        return "l"
    else:
        logging.error(f"unexpected dtype {value!r}")
        return "?"
