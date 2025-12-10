#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import sys
import time
from typing import Dict, List

from eden.fs.service.eden.thrift_types import (
    DebugInvalidateRequest,
    GetStatInfoParams,
    MountId,
    STATS_MOUNTS_STATS,
    TimeSpec,
)

from .lib import testcase


@testcase.eden_repo_test
class InvalidateTest(testcase.EdenRepoTest):
    directories: List[str] = ["a", "b", "c"]
    num_files: int = 10

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {
            "eden.fs.inodes.TreeInode": "DBG5",
        }

    def populate_repo(self) -> None:
        for directory in self.directories:
            for i in range(self.num_files):
                self.repo.write_file(f"{directory}/{i}", f"{i}\n")
        self.repo.commit("Initial commit.")

    async def get_loaded_count(self) -> int:
        async with self.get_thrift_client() as client:
            stats = await client.getStatInfo(
                GetStatInfoParams(statsMask=STATS_MOUNTS_STATS)
            )
        mountPointInfo = stats.mountPointInfo
        if mountPointInfo is None:
            raise Exception("stats.mountPointInfo is not set")
        self.assertEqual(len(mountPointInfo), 1)
        for mountPath in mountPointInfo:
            info = mountPointInfo[mountPath]
            return info.loadedFileCount + info.loadedTreeCount
        return 0  # Apppease pyre

    async def assert_invalidation(
        self,
        invalidated: int,
        expected_invalidated_darwin: int,
        expected_invalidated_other: int,
        expected_loaded_darwin: int,
        expected_loaded_other: int,
    ) -> None:
        """
        On all platforms, both trees and files are invalidated.
        On macOS, only invalidated trees are included in the
        invalidated count; invalidated file counts are not.

        Additionally, when all inodes are invalidated, GC
        unload behavior for top-level directories differs by
        platform, so the expected loaded count after GC can vary.
        """
        if sys.platform == "darwin":
            expected_invalidated = expected_invalidated_darwin
            expected_loaded = expected_loaded_darwin
        else:
            expected_invalidated = expected_invalidated_other
            expected_loaded = expected_loaded_other
        self.assertEqual(invalidated, expected_invalidated)
        self.assertEqual(await self.get_loaded_count(), expected_loaded)

    async def invalidate(
        self, path: str, seconds: int = 0, background: bool = False
    ) -> int:
        async with self.get_thrift_client() as client:
            result = await client.debugInvalidateNonMaterialized(
                DebugInvalidateRequest(
                    mount=MountId(mountPoint=self.mount_path_bytes),
                    path=os.fsencode(path),
                    age=TimeSpec(seconds=seconds, nanoSeconds=0),
                    background=background,
                )
            )
            return result.numInvalidated

    def read_directory(
        self, directory: str, start: int = 0, stop: int = num_files
    ) -> None:
        for i in range(start, stop):
            content = self.read_file(f"{directory}/{i}")
            self.assertEqual(content, f"{i}\n")

    def read_all(self) -> None:
        for directory in self.directories:
            self.read_directory(directory)

    async def test_invalidate_all(self) -> None:
        initial_loaded = await self.get_loaded_count()
        self.read_all()
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 33)
        invalidated = await self.invalidate("")
        await self.assert_invalidation(
            invalidated,
            expected_invalidated_darwin=3,
            expected_invalidated_other=33,
            expected_loaded_darwin=initial_loaded + 2,
            expected_loaded_other=initial_loaded - 1,
        )
        self.read_all()

    async def test_invalidate_subdir(self) -> None:
        initial_loaded = await self.get_loaded_count()
        self.read_all()
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 33)
        invalidated = await self.invalidate("a")
        await self.assert_invalidation(
            invalidated,
            expected_invalidated_darwin=1,
            expected_invalidated_other=10,
            expected_loaded_darwin=initial_loaded + 23,
            expected_loaded_other=initial_loaded + 23,
        )
        self.read_all()

    async def test_no_invalidation_with_age(self) -> None:
        initial_loaded = await self.get_loaded_count()
        self.read_all()
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 33)
        invalidated = await self.invalidate("a", seconds=3600)
        self.assertEqual(invalidated, 0)
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 33)

    async def test_invalidate_with_age(self) -> None:
        initial_loaded = await self.get_loaded_count()
        self.read_all()
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 33)
        time.sleep(10)
        invalidated = await self.invalidate("a", seconds=5)
        await self.assert_invalidation(
            invalidated,
            expected_invalidated_darwin=1,
            expected_invalidated_other=10,
            expected_loaded_darwin=initial_loaded + 23,
            expected_loaded_other=initial_loaded + 23,
        )
        self.read_all()

    async def test_partial_invalidate(self) -> None:
        initial_loaded = await self.get_loaded_count()
        self.read_directory("a")
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 11)
        time.sleep(10)
        self.read_directory("b")
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 22)
        invalidated = await self.invalidate("", seconds=5)
        await self.assert_invalidation(
            invalidated,
            expected_invalidated_darwin=1,
            expected_invalidated_other=11,
            expected_loaded_darwin=initial_loaded + 11,
            expected_loaded_other=initial_loaded + 10,
        )
        self.read_all()

    async def test_partial_directory_invalidate(self) -> None:
        initial_loaded = await self.get_loaded_count()
        self.read_directory("a", 0, 6)
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 7)
        time.sleep(10)
        self.read_directory("a", 6)
        self.assertEqual(await self.get_loaded_count(), initial_loaded + 11)
        invalidated = await self.invalidate("a", seconds=5)
        # On macOS, the lastAccessedTime of the directory is the latest lastAccessedTime
        # of its children. Therefore, as one of the children is accessed after the cutoff,
        # the whole directory is not invalidated.
        await self.assert_invalidation(
            invalidated,
            expected_invalidated_darwin=0,
            expected_invalidated_other=6,
            expected_loaded_darwin=initial_loaded + 11,
            expected_loaded_other=initial_loaded + 5,
        )
        self.read_all()

    async def test_invalidate_background(self) -> None:
        """Verify that starting an invalidation in the background doesn't crash EdenFS."""
        self.read_all()
        await self.invalidate("", seconds=10, background=True)
        time.sleep(2)

    async def test_invalidate_keep_timestamp(self) -> None:
        self.read_all()
        st_before = os.stat(self.get_path("a/1"))
        time.sleep(5)
        await self.invalidate("", seconds=0)
        st_after = os.stat(self.get_path("a/1"))

        self.assertEqual(st_before.st_mtime, st_after.st_mtime)
        # pyre-fixme[16]: `stat_result` has no attribute `st_ctime`.
        self.assertEqual(st_before.st_ctime, st_after.st_ctime)
