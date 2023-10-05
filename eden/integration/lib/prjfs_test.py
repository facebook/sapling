#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from contextlib import contextmanager
from pathlib import Path
from typing import Dict, Generator, Optional, Set, Tuple

from eden.fs.cli import util
from facebook.eden.constants import DIS_REQUIRE_MATERIALIZED
from facebook.eden.ttypes import (
    FaultDefinition,
    GetScmStatusParams,
    RemoveFaultArg,
    ScmFileStatus,
    SyncBehavior,
    UnblockFaultArg,
)

from . import testcase


class PrjFSTestBase(testcase.EdenRepoTest):
    """A base class with helper methods for PrjFS integration tests"""

    enable_fault_injection: bool = True

    # All subclasses must support this method.
    def get_initial_commit(self) -> str:
        raise NotImplementedError

    def eden_status(self, listIgnored: bool = False) -> Dict[bytes, ScmFileStatus]:
        with self.eden.get_thrift_client_legacy() as client:
            status = client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=self.mount.encode(),
                    commit=self.get_initial_commit().encode(),
                    listIgnored=listIgnored,
                    rootIdOptions=None,
                )
            )
            return status.status.entries

    def assertInStatus(self, *files: bytes) -> None:
        status = self.eden_status(listIgnored=True).keys()
        for filename in files:
            self.assertIn(filename, status)

    def assertNotInStatus(self, *files: bytes) -> None:
        status = self.eden_status(listIgnored=True).keys()
        for filename in files:
            self.assertNotIn(filename, status)

    def make_eden_drop_all_notifications(
        self,
        keyClass: str = "PrjfsDispatcherImpl::fileNotification",
        keyValueRegex: str = ".*",
    ) -> None:
        with self.eden.get_thrift_client_legacy() as client:
            client.injectFault(
                FaultDefinition(
                    keyClass=keyClass,
                    keyValueRegex=keyValueRegex,
                    errorMessage="Blocked",
                    errorType="quiet",
                )
            )

    def make_eden_start_processing_notifications_again(
        self,
        keyClass: str = "PrjfsDispatcherImpl::fileNotification",
        keyValueRegex: str = ".*",
    ) -> None:
        with self.eden.get_thrift_client_legacy() as client:
            client.removeFault(
                RemoveFaultArg(keyClass=keyClass, keyValueRegex=keyValueRegex)
            )

    @contextmanager
    def run_with_notifications_dropped_fault(self) -> Generator[None, None, None]:
        self.make_eden_drop_all_notifications()
        try:
            yield
        finally:
            self.make_eden_start_processing_notifications_again()

    def wait_on_fault_unblock(
        self,
        numToUnblock: int = 1,
        keyClass: str = "PrjfsDispatcherImpl::fileNotification",
        keyValueRegex: str = ".*",
    ) -> None:
        def unblock() -> Optional[bool]:
            with self.eden.get_thrift_client_legacy() as client:
                unblocked = client.unblockFault(
                    UnblockFaultArg(
                        keyClass=keyClass,
                        keyValueRegex=keyValueRegex,
                    )
                )
            if unblocked == 1:
                return True
            return None

        for _ in range(numToUnblock):
            util.poll_until(unblock, timeout=30)

    def getAllMaterialized(self, waitTime: int = 5) -> Set[Tuple[Path, int]]:
        """Return all the materialized files/directories minus .hg and .eden"""
        res = set()

        with self.eden.get_thrift_client_legacy() as client:
            inodes = client.debugInodeStatus(
                self.mount_path_bytes,
                b"",
                DIS_REQUIRE_MATERIALIZED,
                SyncBehavior(waitTime),
            )

        for tree_inode in inodes:
            parent_dir = Path(os.fsdecode(tree_inode.path))
            for dirent in tree_inode.entries:
                dirent_path = parent_dir / Path(os.fsdecode(dirent.name))
                top_level_parent = dirent_path.parts[0]
                if top_level_parent != ".hg" and top_level_parent != ".eden":
                    res.add((dirent_path, dirent.mode))

        return res

    def assertNotMaterialized(self, path: str, waitTime: int = 5) -> None:
        materialized = self.getAllMaterialized(waitTime)
        self.assertNotIn(
            Path(path),
            {materialized_path for materialized_path, mode in materialized},
            msg=f"{path} is materialized",
        )

    def assertMaterialized(self, path: str, mode: int, waitTime: int = 5) -> None:
        materialized = self.getAllMaterialized(waitTime)
        self.assertIn(
            (Path(path), mode), materialized, msg=f"{path} is not materialized"
        )

    def assertAllMaterialized(
        self, paths: Set[Tuple[str, int]], waitTime: int = 5
    ) -> None:
        materialized = self.getAllMaterialized(waitTime)
        self.assertSetEqual(materialized, {(Path(path), mode) for path, mode in paths})

    @contextmanager
    def run_with_blocking_fault(
        self,
        keyClass: str = "PrjfsDispatcherImpl::fileNotification",
        keyValueRegex: str = ".*",
    ) -> Generator[None, None, None]:
        with self.eden.get_thrift_client_legacy() as client:
            client.injectFault(
                FaultDefinition(
                    keyClass=keyClass,
                    keyValueRegex=keyValueRegex,
                    block=True,
                )
            )

            try:
                yield
            finally:
                client.removeFault(
                    RemoveFaultArg(
                        keyClass=keyClass,
                        keyValueRegex=keyValueRegex,
                    )
                )
                client.unblockFault(
                    UnblockFaultArg(
                        keyClass=keyClass,
                        keyValueRegex=keyValueRegex,
                    )
                )
