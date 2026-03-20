#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import sys
import time
from typing import Dict, List, Optional

from eden.fs.service.eden.thrift_types import (
    DebugInvalidateRequest,
    GetStatInfoParams,
    MountId,
    STATS_MOUNTS_STATS,
    TimeSpec,
)

from .lib import testcase


@testcase.eden_repo_test(run_on_nfs=False)
class ActiveFuseInvalidationTest(testcase.EdenRepoTest):
    """Test that with pressure-based GC enabled, the active FUSE invalidation
    path in handleChildrenNotAccessedRecently sends FUSE_NOTIFY_INVAL_ENTRY
    for stale inodes, causing the kernel to FORGET them so they can be
    unloaded.

    Without pressure-based GC, the FUSE path doesn't invalidate anything
    (it relies on the kernel dropping references naturally). With it enabled,
    active invalidation means GC can actually reclaim inodes on Linux/FUSE.
    """

    directories: List[str] = ["a", "b", "c"]
    num_files: int = 10

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("experimental", []).append("enable-pressure-based-gc = true")
        return result

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
        async with self.get_async_thrift_client() as client:
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
        return 0  # Appease pyre

    def read_all(self) -> None:
        for directory in self.directories:
            for i in range(self.num_files):
                content = self.read_file(f"{directory}/{i}")
                self.assertEqual(content, f"{i}\n")

    async def invalidate(self, path: str, seconds: int = 0) -> int:
        async with self.get_async_thrift_client() as client:
            result = await client.debugInvalidateNonMaterialized(
                DebugInvalidateRequest(
                    mount=MountId(mountPoint=self.mount_path_bytes),
                    path=os.fsencode(path),
                    age=TimeSpec(seconds=seconds, nanoSeconds=0),
                )
            )
            return result.numInvalidated

    async def test_active_invalidation_unloads_inodes(self) -> None:
        """With pressure-based GC, debugInvalidateNonMaterialized triggers
        active FUSE invalidation which causes the kernel to FORGET inodes,
        allowing them to be unloaded."""
        self.read_all()
        loaded_after_read = await self.get_loaded_count()
        # 30 files + 3 directories + root = at least 34
        self.assertGreaterEqual(loaded_after_read, 34)

        # Wait so inodes are "old"
        time.sleep(3)

        # Trigger GC via debugInvalidateNonMaterialized.
        # With pressure-based GC enabled, this goes through
        # invalidateChildrenNotAccessedRecentlyFuse which sends
        # FUSE_NOTIFY_INVAL_ENTRY, then unloadChildrenUnreferencedByFs.
        invalidated = await self.invalidate("")

        loaded_after = await self.get_loaded_count()
        if sys.platform == "linux":
            # On Linux with active FUSE invalidation, inodes should
            # actually get unloaded (unlike the legacy path which can't
            # invalidate on FUSE).
            self.assertGreater(invalidated, 0)
            self.assertLess(loaded_after, loaded_after_read)
        elif sys.platform == "darwin":
            self.assertLess(loaded_after, loaded_after_read)

        # Files should still be readable
        self.read_all()

    async def test_active_invalidation_respects_age(self) -> None:
        """Active invalidation should only affect inodes older than the
        specified age."""
        # Read directory "a" first
        for i in range(self.num_files):
            self.read_file(f"a/{i}")

        time.sleep(3)

        # Read directory "b" now (so "a" is old, "b" is fresh)
        for i in range(self.num_files):
            self.read_file(f"b/{i}")

        loaded_before = await self.get_loaded_count()

        # Invalidate with 2s age — should affect "a" but not "b"
        await self.invalidate("", seconds=2)

        loaded_after = await self.get_loaded_count()
        # Some inodes from "a" should have been unloaded
        self.assertLess(loaded_after, loaded_before)

        # Everything should still be readable
        self.read_all()
