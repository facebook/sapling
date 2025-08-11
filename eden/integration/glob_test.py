#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import logging
import time
from typing import List, Optional, Tuple

from eden.fs.service.eden.thrift_types import (
    EdenError,
    EdenErrorType,
    GlobParams,
    PrefetchParams,
)

from .lib import testcase


class GlobTestBase(testcase.EdenRepoTest):
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

        self.repo.write_file("other/exclude.java", "")

        self.repo.write_file("case/MIXEDcase", "")

        self.commit1 = self.repo.commit("Commit 1.")

    def setUp(self) -> None:
        # needs to be done before set up because these need to be created
        # for populate_repo() and the supers set up will call this.
        self.commit0 = ""
        self.commit1 = ""

        super().setUp()

    async def assert_glob(
        self,
        globs: List[str],
        expected_matches: List[bytes],
        include_dotfiles: bool = False,
        msg: Optional[str] = None,
        commits: Optional[List[bytes]] = None,
        directories_only: bool = False,
        prefetching: bool = False,
        expected_commits: Optional[List[bytes]] = None,
        search_root: Optional[bytes] = None,
        list_only_files: bool = False,
        background: bool = False,
    ) -> None:
        raise NotImplementedError("assert glob not implemented")


class GlobFilesTestBase(GlobTestBase):
    async def assert_glob(
        self,
        globs: List[str],
        expected_matches: List[bytes],
        include_dotfiles: bool = False,
        msg: Optional[str] = None,
        commits: Optional[List[bytes]] = None,
        directories_only: bool = False,
        prefetching: bool = False,
        expected_commits: Optional[List[bytes]] = None,
        search_root: Optional[bytes] = None,
        list_only_files: bool = False,
        background: bool = False,
    ) -> None:
        params = GlobParams(
            mountPoint=self.mount_path_bytes,
            globs=globs,
            includeDotfiles=include_dotfiles,
            prefetchFiles=prefetching,
            revisions=commits,
            searchRoot=search_root,
            listOnlyFiles=list_only_files,
            background=background,
        )
        async with self.get_thrift_client() as client:
            result = await client.globFiles(params)
            if not result:
                self.assertEqual(expected_matches, [], msg=msg)
                return
            self.assertEqual(
                expected_matches, sorted(list(result.matchingFiles)), msg=msg
            )
            self.assertFalse(result.dtypes)

            if expected_commits:
                result2 = await client.globFiles(params)
                self.assertCountEqual(expected_commits, result2.originHashes, msg=msg)

    async def assert_glob_with_dtypes(
        self,
        globs: List[str],
        expected_matches: List[Tuple[bytes, str]],
        include_dotfiles: bool = False,
        msg: Optional[str] = None,
    ) -> None:
        params = GlobParams(
            mountPoint=self.mount_path_bytes,
            globs=globs,
            includeDotfiles=include_dotfiles,
            wantDtype=True,
        )
        async with self.get_thrift_client() as client:
            result = await client.globFiles(params)
            actual_results = zip(
                result.matchingFiles,
                (_dtype_to_str(dtype) for dtype in result.dtypes),
            )
            self.assertEqual(expected_matches, sorted(actual_results), msg=msg)


class PrefetchTestBase(GlobTestBase):
    async def assert_glob(
        self,
        globs: List[str],
        expected_matches: List[bytes],
        include_dotfiles: bool = False,
        msg: Optional[str] = None,
        commits: Optional[List[bytes]] = None,
        directories_only: bool = False,
        prefetching: bool = False,
        expected_commits: Optional[List[bytes]] = None,
        search_root: Optional[bytes] = None,
        list_only_files: bool = False,
        background: bool = False,
    ) -> None:
        params = PrefetchParams(
            mountPoint=self.mount_path_bytes,
            globs=globs,
            directoriesOnly=directories_only,
            revisions=commits,
            searchRoot=search_root,
            background=background,
            returnPrefetchedFiles=True,
        )
        async with self.get_thrift_client() as client:
            prefetchResult = await client.prefetchFilesV2(params)
            result = prefetchResult.prefetchedFiles
            if not result:
                self.assertEqual(expected_matches, [], msg=msg)
                return
            self.assertEqual(expected_matches, sorted(result.matchingFiles), msg=msg)
            self.assertFalse(result.dtypes)

            if expected_commits:
                prefetchResult2 = await client.prefetchFilesV2(params)
                if prefetchResult2.prefetchedFiles:
                    self.assertCountEqual(
                        expected_commits,
                        prefetchResult2.prefetchedFiles.originHashes,
                        msg=msg,
                    )


# assert_glob defined above. This is a base class that holds shared test cases.
class GlobTestCasesBase:
    def __init__(self) -> None:
        self.commit0 = ""
        self.commit1 = ""

    async def assert_glob(
        self,
        globs: List[str],
        expected_matches: List[bytes],
        include_dotfiles: bool = False,
        msg: Optional[str] = None,
        commits: Optional[List[bytes]] = None,
        directories_only: bool = False,
        prefetching: bool = False,
        expected_commits: Optional[List[bytes]] = None,
        search_root: Optional[bytes] = None,
        list_only_files: bool = False,
        background: bool = False,
    ) -> None:
        raise NotImplementedError("assert glob not implemented")

    async def test_exact_path_component_match(self) -> None:
        await self.assert_glob(["hello"], [b"hello"])
        await self.assert_glob(["ddir/subdir/.dotfile"], [b"ddir/subdir/.dotfile"])

    async def test_wildcard_path_component_match(self) -> None:
        await self.assert_glob(["hel*"], [b"hello"])
        await self.assert_glob(["ad*"], [b"adir"])
        await self.assert_glob(["a*/file"], [b"adir/file"])

    async def test_no_accidental_substring_match(self) -> None:
        await self.assert_glob(["hell"], [], msg="No accidental substring match")

    async def test_match_all_files_in_directory(self) -> None:
        await self.assert_glob(["bdir/*"], [b"bdir/file", b"bdir/otherfile"])

    async def test_overlapping_globs(self) -> None:
        await self.assert_glob(
            ["adir/*", "**/file"],
            [b"adir/file", b"bdir/file"],
            msg="De-duplicate results from multiple globs",
        )

    async def test_recursive_wildcard_prefix(self) -> None:
        await self.assert_glob(["**/file"], [b"adir/file", b"bdir/file"])

    async def test_recursive_wildcard_suffix(self) -> None:
        await self.assert_glob(["adir/**"], [b"adir/file"])
        await self.assert_glob(["adir/**/*"], [b"adir/file"])

    async def test_qualified_recursive_wildcard(self) -> None:
        await self.assert_glob(
            ["java/com/**/*.java"],
            [
                b"java/com/example/Example.java",
                b"java/com/example/foo/Foo.java",
                b"java/com/example/foo/bar/Bar.java",
                b"java/com/example/foo/bar/baz/Baz.java",
            ],
        )
        await self.assert_glob(
            ["java/com/example/*/*.java"], [b"java/com/example/foo/Foo.java"]
        )

    async def test_glob_on_non_current_commit(self) -> None:
        await self.assert_glob(
            ["hello"], [b"hello"], commits=[bytes.fromhex(self.commit0)]
        )
        await self.assert_glob(
            ["hola"], [b"hola"], commits=[bytes.fromhex(self.commit0)]
        )

    async def test_glob_multiple_commits(self) -> None:
        await self.assert_glob(
            ["hello"],
            [b"hello", b"hello"],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )
        await self.assert_glob(
            ["h*"],
            [b"hello", b"hello", b"hola"],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )
        await self.assert_glob(
            ["a*/*ile"],
            [b"adir/file", b"adir/phile"],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )

    async def test_prefetch_matching_files(self) -> None:
        await self.assert_glob(["hello"], [b"hello"], prefetching=True)
        await self.assert_glob(
            ["hello"],
            [b"hello"],
            prefetching=True,
            commits=[bytes.fromhex(self.commit0)],
        )
        await self.assert_glob(
            ["hello"],
            [b"hello", b"hello"],
            prefetching=True,
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )

    async def test_simple_matching_commit(self) -> None:
        await self.assert_glob(
            ["hello"],
            expected_matches=[b"hello"],
            expected_commits=[bytes.fromhex(self.commit1)],
        )

        await self.assert_glob(
            ["hello"],
            expected_matches=[b"hello"],
            expected_commits=[bytes.fromhex(self.commit0)],
            commits=[bytes.fromhex(self.commit0)],
        )

    async def test_duplicate_file_multiple_commits(self) -> None:
        await self.assert_glob(
            ["hello"],
            expected_matches=[b"hello", b"hello"],
            expected_commits=[
                bytes.fromhex(self.commit0),
                bytes.fromhex(self.commit1),
            ],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )

    async def test_multiple_file_multiple_commits(self) -> None:
        await self.assert_glob(
            ["a*/*ile"],
            [b"adir/file", b"adir/phile"],
            expected_commits=[
                bytes.fromhex(self.commit1),
                bytes.fromhex(self.commit0),
            ],
            commits=[bytes.fromhex(self.commit0), bytes.fromhex(self.commit1)],
        )

    async def test_search_root(self) -> None:
        await self.assert_glob(
            ["**/*.java"],
            expected_matches=[
                b"example/Example.java",
                b"example/foo/Foo.java",
                b"example/foo/bar/Bar.java",
                b"example/foo/bar/baz/Baz.java",
            ],
            search_root=b"java/com",
        )

    async def test_search_root_with_specified_commits(self) -> None:
        await self.assert_glob(
            ["**/*.java"],
            expected_matches=[
                b"example/Example.java",
                b"example/foo/Foo.java",
                b"example/foo/bar/Bar.java",
                b"example/foo/bar/baz/Baz.java",
            ],
            expected_commits=[
                bytes.fromhex(self.commit1),
                bytes.fromhex(self.commit1),
                bytes.fromhex(self.commit1),
                bytes.fromhex(self.commit1),
            ],
            commits=[bytes.fromhex(self.commit1)],
            search_root=b"java/com",
        )

    async def test_glob_list_includes_dirs(self) -> None:
        await self.assert_glob(
            ["java/com/**/*"],
            [
                b"java/com/example",
                b"java/com/example/Example.java",
                b"java/com/example/foo",
                b"java/com/example/foo/Foo.java",
                b"java/com/example/foo/bar",
                b"java/com/example/foo/bar/Bar.java",
                b"java/com/example/foo/bar/baz",
                b"java/com/example/foo/bar/baz/Baz.java",
                b"java/com/example/package.html",
            ],
        )

    async def test_glob_background(self) -> None:
        # Make sure that we don't have weird use after free in background globs
        await self.assert_glob(
            ["**/*"],
            [],
            background=True,
            prefetching=True,
        )
        # The glob above returns immediately, we need to wait so it completes.
        time.sleep(1)


@testcase.eden_repo_test
class GlobTest(GlobFilesTestBase, GlobTestCasesBase):
    async def test_wildcard_path_component_match_with_dtypes(self) -> None:
        await self.assert_glob_with_dtypes(["ad*"], [(b"adir", "d")])
        await self.assert_glob_with_dtypes(["a*/file"], [(b"adir/file", "f")])

    async def test_match_all_files_in_directory_with_dotfile(self) -> None:
        await self.assert_glob(["ddir/subdir/*"], [b"ddir/subdir/notdotfile"])

    async def test_recursive_wildcard_suffix_with_dotfile(self) -> None:
        await self.assert_glob(
            ["ddir/**"], [b"ddir/notdotfile", b"ddir/subdir", b"ddir/subdir/notdotfile"]
        )
        await self.assert_glob(
            ["ddir/**"],
            [
                b"ddir/notdotfile",
                b"ddir/subdir",
                b"ddir/subdir/.dotfile",
                b"ddir/subdir/notdotfile",
            ],
            include_dotfiles=True,
        )

        await self.assert_glob(
            ["ddir/**/*"],
            [b"ddir/notdotfile", b"ddir/subdir", b"ddir/subdir/notdotfile"],
        )
        await self.assert_glob(
            ["ddir/**/*"],
            [
                b"ddir/notdotfile",
                b"ddir/subdir",
                b"ddir/subdir/.dotfile",
                b"ddir/subdir/notdotfile",
            ],
            include_dotfiles=True,
        )

    async def test_malformed_query(self) -> None:
        async with self.get_thrift_client() as client:
            with self.assertRaises(EdenError) as ctx:
                await client.globFiles(
                    GlobParams(mountPoint=self.mount_path_bytes, globs=["adir["])
                )
            self.assertIn("unterminated bracket sequence", str(ctx.exception))
            self.assertEqual(EdenErrorType.POSIX_ERROR, ctx.exception.errorType)

            with self.assertRaises(EdenError) as ctx:
                await client.globFiles(
                    GlobParams(
                        mountPoint=self.mount_path_bytes,
                        globs=["adir["],
                        includeDotfiles=True,
                    )
                )
            self.assertIn("unterminated bracket sequence", str(ctx.exception))
            self.assertEqual(EdenErrorType.POSIX_ERROR, ctx.exception.errorType)

    async def test_globs_may_not_include_dotdot(self) -> None:
        async with self.get_thrift_client() as client:
            with self.assertRaises(EdenError) as ctx:
                await client.globFiles(
                    GlobParams(
                        mountPoint=self.mount_path_bytes,
                        globs=["java/../java/com/**/*.java"],
                    )
                )
            self.assertEqual(
                "Invalid glob (PathComponent must not be ..): java/../java/com/**/*.java",
                str(ctx.exception),
            )
            self.assertEqual(EdenErrorType.ARGUMENT_ERROR, ctx.exception.errorType)

    async def test_glob_list_only_files(self) -> None:
        await self.assert_glob(
            ["java/com/**/*"],
            [
                b"java/com/example/Example.java",
                b"java/com/example/foo/Foo.java",
                b"java/com/example/foo/bar/Bar.java",
                b"java/com/example/foo/bar/baz/Baz.java",
                b"java/com/example/package.html",
            ],
            list_only_files=True,
        )


@testcase.eden_repo_test
class PrefetchTest(PrefetchTestBase, GlobTestCasesBase):
    async def test_match_all_files_in_directory_with_dotfile(self) -> None:
        await self.assert_glob(
            ["ddir/subdir/*"],
            [b"ddir/subdir/.dotfile", b"ddir/subdir/notdotfile"],
            msg="dotfiles are included in prefetching",
        )

    async def test_recursive_wildcard_suffix_with_dotfile(self) -> None:
        await self.assert_glob(
            ["ddir/**"],
            [
                b"ddir/notdotfile",
                b"ddir/subdir",
                b"ddir/subdir/.dotfile",
                b"ddir/subdir/notdotfile",
            ],
            msg="dotfiles are included in prefetching",
        )

        await self.assert_glob(
            ["ddir/**/*"],
            [
                b"ddir/notdotfile",
                b"ddir/subdir",
                b"ddir/subdir/.dotfile",
                b"ddir/subdir/notdotfile",
            ],
            msg="dotfiles are included in prefetching",
        )

    async def test_malformed_query(self) -> None:
        async with self.get_thrift_client() as client:
            with self.assertRaises(EdenError) as ctx:
                await client.prefetchFilesV2(
                    PrefetchParams(mountPoint=self.mount_path_bytes, globs=["adir["])
                )
            self.assertIn("unterminated bracket sequence", str(ctx.exception))
            self.assertEqual(EdenErrorType.POSIX_ERROR, ctx.exception.errorType)

            with self.assertRaises(EdenError) as ctx:
                await client.prefetchFilesV2(
                    PrefetchParams(
                        mountPoint=self.mount_path_bytes,
                        globs=["adir["],
                        directoriesOnly=True,
                    )
                )
            self.assertIn("unterminated bracket sequence", str(ctx.exception))
            self.assertEqual(EdenErrorType.POSIX_ERROR, ctx.exception.errorType)

    async def test_globs_may_not_include_dotdot(self) -> None:
        async with self.get_thrift_client() as client:
            with self.assertRaises(EdenError) as ctx:
                await client.prefetchFilesV2(
                    PrefetchParams(
                        mountPoint=self.mount_path_bytes,
                        globs=["java/../java/com/**/*.java"],
                    )
                )
            self.assertEqual(
                "Invalid glob (PathComponent must not be ..): java/../java/com/**/*.java",
                str(ctx.exception),
            )
            self.assertEqual(EdenErrorType.ARGUMENT_ERROR, ctx.exception.errorType)


@testcase.eden_repo_test(case_sensitivity_dependent=True)
class GlobCaseDependentTest(GlobFilesTestBase, testcase.EdenRepoTest):
    async def test_case_preserving(self) -> None:
        await self.assert_glob(
            ["case/MixedCase"],
            expected_matches=[] if self.is_case_sensitive else [b"case/MIXEDcase"],
        )
        await self.assert_glob(
            ["CASE/mixedcase"],
            expected_matches=[] if self.is_case_sensitive else [b"case/MIXEDcase"],
        )

    async def test_case_insensitive(self) -> None:
        await self.assert_glob(
            ["case/M*C*"],
            expected_matches=[] if self.is_case_sensitive else [b"case/MIXEDcase"],
        )
        await self.assert_glob(
            ["CA*/?ixedcase"],
            expected_matches=[] if self.is_case_sensitive else [b"case/MIXEDcase"],
        )


@testcase.eden_repo_test(case_sensitivity_dependent=True)
class PrefetchCaseDependentTest(PrefetchTestBase, testcase.EdenRepoTest):
    async def test_case_preserving(self) -> None:
        await self.assert_glob(
            ["case/MixedCase"],
            expected_matches=[] if self.is_case_sensitive else [b"case/MIXEDcase"],
        )
        await self.assert_glob(
            ["CASE/mixedcase"],
            expected_matches=[] if self.is_case_sensitive else [b"case/MIXEDcase"],
        )

    async def test_case_insensitive(self) -> None:
        await self.assert_glob(
            ["case/M*C*"],
            expected_matches=[] if self.is_case_sensitive else [b"case/MIXEDcase"],
        )
        await self.assert_glob(
            ["CA*/?ixedcase"],
            expected_matches=[] if self.is_case_sensitive else [b"case/MIXEDcase"],
        )


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
