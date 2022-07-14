#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from contextlib import contextmanager
from pathlib import Path
from typing import Dict, Generator, Optional, Set

from eden.fs.cli import util
from facebook.eden.constants import DIS_REQUIRE_MATERIALIZED
from facebook.eden.ttypes import (
    FaultDefinition,
    RemoveFaultArg,
    SyncBehavior,
    UnblockFaultArg,
)

from .lib import testcase


@testcase.eden_repo_test
class PrjFSStress(testcase.EdenRepoTest):
    enable_fault_injection: bool = True

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden.strace": "DBG7"}

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

    def getAllMaterialized(self) -> Set[Path]:
        res = set()

        with self.eden.get_thrift_client_legacy() as client:
            inodes = client.debugInodeStatus(
                self.mount_path_bytes, b"", DIS_REQUIRE_MATERIALIZED, SyncBehavior(5)
            )

        for tree_inode in inodes:
            parent_dir = Path(os.fsdecode(tree_inode.path))
            for dirent in tree_inode.entries:
                dirent_path = parent_dir / Path(os.fsdecode(dirent.name))
                res.add(dirent_path)

        return res

    def assertNotMaterialized(self, path: str) -> None:
        materialized = self.getAllMaterialized()
        self.assertNotIn(Path(path), materialized, msg=f"{path} is materialized")

    def assertMaterialized(self, path: str) -> None:
        materialized = self.getAllMaterialized()
        self.assertIn(Path(path), materialized, msg=f"{path} is not materialized")

    @contextmanager
    def run_with_fault(
        self, keyClass="PrjfsDispatcherImpl::fileNotification", keyValueRegex=".*"
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

    def test_create_and_remove_file(self) -> None:
        with self.run_with_fault():
            self.touch("foo")
            # EdenFS will now block due to the fault above
            self.wait_on_fault_unblock()
            self.rm("foo")
            self.wait_on_fault_unblock()

            self.assertNotMaterialized("foo")

    def test_create_already_removed(self) -> None:
        with self.run_with_fault():
            self.touch("foo")
            # EdenFS will now block due to the fault above, remove the file to
            # force it down the removal path.
            self.rm("foo")
            self.wait_on_fault_unblock(2)

            self.assertNotMaterialized("foo")

    def test_create_file_to_directory(self) -> None:
        with self.run_with_fault():
            self.touch("foo")
            # EdenFS will now block due to the fault above, remove the file to
            # force it down the removal path.
            self.rm("foo")
            # And then create the directory
            self.mkdir("foo")
            self.wait_on_fault_unblock(3)

            self.assertMaterialized("foo")

    def test_rename_hierarchy(self) -> None:
        with self.run_with_fault():
            self.mkdir("foo")
            self.touch("foo/bar")
            self.touch("foo/baz")
            self.wait_on_fault_unblock(3)

            self.rename("foo", "bar")
            self.wait_on_fault_unblock(
                2
            )  # A rename is a total removal and a total creation

            self.assertMaterialized("bar")
            self.assertNotMaterialized("foo")
