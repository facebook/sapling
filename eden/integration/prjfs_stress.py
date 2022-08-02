#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import stat
from contextlib import contextmanager
from pathlib import Path
from typing import Dict, Generator, Optional, Set, Tuple

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

            self.assertMaterialized("foo", stat.S_IFDIR)

    def test_create_directory_to_file(self) -> None:
        with self.run_with_fault():
            self.mkdir("foo")
            self.rmdir("foo")
            self.touch("foo")
            self.wait_on_fault_unblock(3)

            self.assertMaterialized("foo", stat.S_IFREG)

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

            self.assertMaterialized("bar", stat.S_IFDIR)
            self.assertNotMaterialized("foo")

    def test_rename_to_file(self) -> None:
        with self.run_with_fault():
            self.mkdir("foo")
            self.touch("foo/bar")
            self.touch("foo/baz")
            self.wait_on_fault_unblock(3)

            self.rename("foo", "bar")
            self.rm("bar/bar")
            self.rm("bar/baz")
            self.rmdir("bar")
            self.touch("bar")

            self.wait_on_fault_unblock(6)

            self.assertMaterialized("bar", stat.S_IFREG)
            self.assertNotMaterialized("foo")

    def test_rename_and_replace(self) -> None:
        with self.run_with_fault():
            self.mkdir("foo")
            self.touch("foo/bar")
            self.touch("foo/baz")
            self.wait_on_fault_unblock(3)

            self.rename("foo", "bar")
            self.mkdir("foo")
            self.mkdir("foo/hello")

            self.wait_on_fault_unblock(4)

            self.assertAllMaterialized(
                {
                    ("bar", stat.S_IFDIR),
                    ("bar/bar", stat.S_IFREG),
                    ("bar/baz", stat.S_IFREG),
                    ("foo", stat.S_IFDIR),
                    ("foo/hello", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                }
            )

    def test_out_of_order_file_removal(self) -> None:
        with self.run_with_fault():
            self.mkdir("a/b")
            self.touch("a/b/c")
            self.wait_on_fault_unblock(3)

            self.rm("a/b/c")
            # A wait_on_fault_unblock(1) below will just wait for the rm to be
            # unblocked, not for it to terminate. This is usually not an issue
            # due to Thrift APIs waiting on all IO when a positive SyncBehavior
            # is used, but since we'll need to pass a SyncBehavior of 0
            # seconds, the only way to guarantee the rm above would have
            # completed is by forcing some IO and unblocking these.
            self.touch("foo")
            self.rm("foo")

            self.rmdir("a/b")
            self.touch("a/b")

            # Unblock rm("a/b/c") touch("foo") and rm("foo")
            self.wait_on_fault_unblock(3)

            self.assertAllMaterialized(
                {
                    ("a/b", stat.S_IFREG),
                    ("a", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                },
                waitTime=0,
            )

            self.wait_on_fault_unblock(2)
            self.assertAllMaterialized(
                {
                    ("a/b", stat.S_IFREG),
                    ("a", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                }
            )

    def test_rename_twice(self) -> None:
        with self.run_with_fault():
            self.mkdir("first")
            self.touch("first/a")
            self.mkdir("first/b")

            self.mkdir("second")
            self.touch("second/c")
            self.touch("second/d")

            self.rename("first", "third")
            self.rename("second", "first")
            self.rename("third", "second")

            self.wait_on_fault_unblock(12)

            self.assertAllMaterialized(
                {
                    ("first", stat.S_IFDIR),
                    ("first/c", stat.S_IFREG),
                    ("first/d", stat.S_IFREG),
                    ("second", stat.S_IFDIR),
                    ("second/a", stat.S_IFREG),
                    ("second/b", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                }
            )
