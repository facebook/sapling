#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import time
from typing import Dict, List

from facebook.eden.constants import STATS_MOUNTS_STATS

from facebook.eden.ttypes import (
    DebugInvalidateRequest,
    GetStatInfoParams,
    MountId,
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

    def get_loaded_count(self) -> int:
        with self.get_thrift_client_legacy() as client:
            stats = client.getStatInfo(GetStatInfoParams(statsMask=STATS_MOUNTS_STATS))
        mountPointInfo = stats.mountPointInfo
        if mountPointInfo is None:
            raise Exception("stats.mountPointInfo is not set")
        self.assertEqual(len(mountPointInfo), 1)
        for mountPath in mountPointInfo:
            info = mountPointInfo[mountPath]
            return info.loadedFileCount + info.loadedTreeCount
        return 0  # Apppease pyre

    def invalidate(self, path: str, seconds: int = 0) -> int:
        with self.get_thrift_client_legacy() as client:
            return client.debugInvalidateNonMaterialized(
                DebugInvalidateRequest(
                    mount=MountId(mountPoint=self.mount_path_bytes),
                    path=os.fsencode(path),
                    age=TimeSpec(seconds=seconds, nanoSeconds=0),
                )
            ).numInvalidated

    def read_directory(
        self, directory: str, start: int = 0, stop: int = num_files
    ) -> None:
        for i in range(start, stop):
            content = self.read_file(f"{directory}/{i}")
            self.assertEqual(content, f"{i}\n")

    def read_all(self) -> None:
        for directory in self.directories:
            self.read_directory(directory)

    def test_invalidate_all(self) -> None:
        initial_loaded = self.get_loaded_count()
        self.read_all()
        self.assertEqual(self.get_loaded_count(), initial_loaded + 33)
        invalidated = self.invalidate("")
        self.assertEqual(invalidated, 33)
        # pyre-fixme[6]: Incompatible parameter type [6]: In call `unittest.case.TestCase.assertAlmostEqual`, for 3rd parameter `delta` expected `None` but got `int`.
        self.assertAlmostEqual(self.get_loaded_count(), initial_loaded, delta=1)
        self.read_all()

    def test_invalidate_subdir(self) -> None:
        initial_loaded = self.get_loaded_count()
        self.read_all()
        self.assertEqual(self.get_loaded_count(), initial_loaded + 33)
        invalidated = self.invalidate("a")
        self.assertEqual(invalidated, 10)
        self.assertEqual(self.get_loaded_count(), initial_loaded + 23)
        self.read_all()

    def test_no_invalidation_with_age(self) -> None:
        initial_loaded = self.get_loaded_count()
        self.read_all()
        self.assertEqual(self.get_loaded_count(), initial_loaded + 33)
        invalidated = self.invalidate("a", seconds=3600)
        self.assertEqual(invalidated, 0)
        self.assertEqual(self.get_loaded_count(), initial_loaded + 33)

    def test_invalidate_with_age(self) -> None:
        initial_loaded = self.get_loaded_count()
        self.read_all()
        self.assertEqual(self.get_loaded_count(), initial_loaded + 33)
        time.sleep(10)
        invalidated = self.invalidate("a", seconds=5)
        self.assertEqual(invalidated, 10)
        self.assertEqual(self.get_loaded_count(), initial_loaded + 23)
        self.read_all()

    def test_partial_invalidate(self) -> None:
        initial_loaded = self.get_loaded_count()
        self.read_directory("a")
        self.assertEqual(self.get_loaded_count(), initial_loaded + 11)
        time.sleep(10)
        self.read_directory("b")
        self.assertEqual(self.get_loaded_count(), initial_loaded + 22)
        invalidated = self.invalidate("", seconds=5)
        self.assertEqual(invalidated, 11)
        # pyre-fixme[6]: Incompatible parameter type [6]: In call `unittest.case.TestCase.assertAlmostEqual`, for 3rd parameter `delta` expected `None` but got `int`.
        self.assertAlmostEqual(self.get_loaded_count(), initial_loaded + 11, delta=1)
        self.read_all()

    def test_partial_directory_invalidate(self) -> None:
        initial_loaded = self.get_loaded_count()
        self.read_directory("a", 0, 6)
        self.assertEqual(self.get_loaded_count(), initial_loaded + 7)
        time.sleep(10)
        self.read_directory("a", 6)
        self.assertEqual(self.get_loaded_count(), initial_loaded + 11)
        invalidated = self.invalidate("a", seconds=5)
        self.assertEqual(invalidated, 6)
        self.assertEqual(self.get_loaded_count(), initial_loaded + 5)
        self.read_all()
