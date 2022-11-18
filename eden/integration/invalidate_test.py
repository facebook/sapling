#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from typing import List

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

    def invalidate(self, path: str) -> int:
        with self.get_thrift_client_legacy() as client:
            return client.debugInvalidateNonMaterialized(
                DebugInvalidateRequest(
                    mount=MountId(mountPoint=self.mount_path_bytes),
                    path=os.fsencode(path),
                    age=TimeSpec(seconds=0, nanoSeconds=0),
                )
            ).numInvalidated

    def read_all(self) -> None:
        for directory in self.directories:
            for i in range(self.num_files):
                content = self.read_file(f"{directory}/{i}")
                self.assertEqual(content, f"{i}\n")

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
