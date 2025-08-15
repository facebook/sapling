#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from contextlib import asynccontextmanager
from pathlib import Path
from typing import AsyncGenerator, Mapping, Set, Tuple

from eden.fs.service.eden.thrift_types import (
    FaultDefinition,
    GetScmStatusParams,
    RemoveFaultArg,
    ScmFileStatus,
    SyncBehavior,
)
from facebook.eden.constants import DIS_REQUIRE_MATERIALIZED

from . import testcase


class PrjFSTestBase(testcase.EdenRepoTest):
    """A base class with helper methods for PrjFS integration tests"""

    enable_fault_injection: bool = True

    # All subclasses must support this method.
    def get_initial_commit(self) -> str:
        raise NotImplementedError

    async def eden_status(
        self, listIgnored: bool = False
    ) -> Mapping[bytes, ScmFileStatus]:
        async with self.eden.get_thrift_client() as client:
            status = await client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=self.mount.encode(),
                    commit=self.get_initial_commit().encode(),
                    listIgnored=listIgnored,
                    rootIdOptions=None,
                )
            )
            return status.status.entries

    async def assertInStatus(self, *files: bytes) -> None:
        status_dict = await self.eden_status(listIgnored=True)
        status = status_dict.keys()
        for filename in files:
            self.assertIn(filename, status)

    async def assertNotInStatus(self, *files: bytes) -> None:
        status_dict = await self.eden_status(listIgnored=True)
        status = status_dict.keys()
        for filename in files:
            self.assertNotIn(filename, status)

    async def make_eden_drop_all_notifications(
        self,
        keyClass: str = "PrjfsDispatcherImpl::fileNotification",
        keyValueRegex: str = ".*",
    ) -> None:
        async with self.eden.get_thrift_client() as client:
            await client.injectFault(
                FaultDefinition(
                    keyClass=keyClass,
                    keyValueRegex=keyValueRegex,
                    errorMessage="Blocked",
                    errorType="quiet",
                )
            )

    async def make_eden_start_processing_notifications_again(
        self,
        keyClass: str = "PrjfsDispatcherImpl::fileNotification",
        keyValueRegex: str = ".*",
    ) -> None:
        async with self.eden.get_thrift_client() as client:
            await client.removeFault(
                RemoveFaultArg(keyClass=keyClass, keyValueRegex=keyValueRegex)
            )

    @asynccontextmanager
    async def run_with_notifications_dropped_fault(self) -> AsyncGenerator[None, None]:
        await self.make_eden_drop_all_notifications()
        try:
            yield
        finally:
            await self.make_eden_start_processing_notifications_again()

    async def getAllMaterialized(self, waitTime: int = 5) -> Set[Tuple[Path, int]]:
        """Return all the materialized files/directories minus .hg and .eden"""
        res = set()

        async with self.eden.get_thrift_client() as client:
            inodes = await client.debugInodeStatus(
                self.mount_path_bytes,
                b"",
                DIS_REQUIRE_MATERIALIZED,
                SyncBehavior(syncTimeoutSeconds=waitTime),
            )

        for tree_inode in inodes:
            parent_dir = Path(os.fsdecode(tree_inode.path))
            for dirent in tree_inode.entries:
                dirent_path = parent_dir / Path(os.fsdecode(dirent.name))
                top_level_parent = dirent_path.parts[0]
                if top_level_parent != ".hg" and top_level_parent != ".eden":
                    res.add((dirent_path, dirent.mode))

        return res

    async def assertNotMaterialized(self, path: str, waitTime: int = 5) -> None:
        materialized = await self.getAllMaterialized(waitTime)
        self.assertNotIn(
            Path(path),
            {materialized_path for materialized_path, mode in materialized},
            msg=f"{path} is materialized",
        )

    async def assertMaterialized(self, path: str, mode: int, waitTime: int = 5) -> None:
        materialized = await self.getAllMaterialized(waitTime)
        self.assertIn(
            (Path(path), mode), materialized, msg=f"{path} is not materialized"
        )

    async def assertAllMaterialized(
        self, paths: Set[Tuple[str, int]], waitTime: int = 5
    ) -> None:
        materialized = await self.getAllMaterialized(waitTime)
        self.assertSetEqual(materialized, {(Path(path), mode) for path, mode in paths})
