#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import stat
import subprocess
import time
from threading import Thread
from typing import Dict, List, Optional

from .lib import prjfs_test, testcase


class PrjFSStressBase(prjfs_test.PrjFSTestBase):
    initial_commit: str = ""

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.initial_commit = self.repo.commit("Initial commit.")

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden.strace": "DBG7"}

    def get_initial_commit(self) -> str:
        return self.initial_commit


@testcase.eden_repo_test
class PrjFSStress(PrjFSStressBase):
    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("prjfs", []).append("listen-to-pre-convert-to-full = true")
        return result

    def test_create_and_remove_file(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.touch("foo")
            # EdenFS will now block due to the fault above
            self.wait_on_fault_unblock(keyClass="PrjfsDispatcherImpl::fileNotification")
            self.rm("foo")
            self.wait_on_fault_unblock(keyClass="PrjfsDispatcherImpl::fileNotification")

            self.assertNotMaterialized("foo")

    def test_create_already_removed(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.touch("foo")
            # EdenFS will now block due to the fault above, remove the file to
            # force it down the removal path.
            self.rm("foo")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=2
            )

            self.assertNotMaterialized("foo")

    def test_create_file_to_directory(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.touch("foo")
            # EdenFS will now block due to the fault above, remove the file to
            # force it down the removal path.
            self.rm("foo")
            # And then create the directory
            self.mkdir("foo")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )

            self.assertMaterialized("foo", stat.S_IFDIR)

    def test_create_directory_to_file(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.mkdir("foo")
            self.rmdir("foo")
            self.touch("foo")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )

            self.assertMaterialized("foo", stat.S_IFREG)

    def test_rename_hierarchy(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.mkdir("foo")
            self.touch("foo/bar")
            self.touch("foo/baz")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )

            self.rename("foo", "bar")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=2
            )  # A rename is a total removal and a total creation

            self.assertMaterialized("bar", stat.S_IFDIR)
            self.assertNotMaterialized("foo")

    def test_rename_to_file(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.mkdir("foo")
            self.touch("foo/bar")
            self.touch("foo/baz")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )

            self.rename("foo", "bar")
            self.rm("bar/bar")
            self.rm("bar/baz")
            self.rmdir("bar")
            self.touch("bar")

            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=6
            )

            self.assertMaterialized("bar", stat.S_IFREG)
            self.assertNotMaterialized("foo")

    def test_rename_and_replace(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.mkdir("foo")
            self.touch("foo/bar")
            self.touch("foo/baz")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )

            self.rename("foo", "bar")
            self.mkdir("foo")
            self.mkdir("foo/hello")

            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=4
            )

            self.assertAllMaterialized(
                {
                    ("adir", stat.S_IFDIR),
                    ("bar", stat.S_IFDIR),
                    ("bar/bar", stat.S_IFREG),
                    ("bar/baz", stat.S_IFREG),
                    ("foo", stat.S_IFDIR),
                    ("foo/hello", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                }
            )

    def test_out_of_order_file_removal(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.mkdir("a/b")
            self.touch("a/b/c")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )

            self.rm("a/b/c")
            # A wait_on_fault_unblock(keyClass = "PrjfsDispatcherImpl::fileNotification", numToUnblock=1) below will just wait for the rm to be
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
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )

            self.assertAllMaterialized(
                {
                    ("a/b", stat.S_IFREG),
                    ("a", stat.S_IFDIR),
                    ("adir", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                },
                waitTime=0,
            )

            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=2
            )
            self.assertAllMaterialized(
                {
                    ("a/b", stat.S_IFREG),
                    ("a", stat.S_IFDIR),
                    ("adir", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                }
            )

    def test_out_of_order_file_removal_to_renamed(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.mkdir("a/b")
            self.touch("a/b/c")
            self.mkdir("z")
            self.touch("z/y")
            self.touch("z/x")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=6
            )

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
            self.rename("z", "a/b")

            # Unblock rm("a/b/c") touch("foo") and rm("foo")
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )

            self.assertAllMaterialized(
                {
                    ("a/b", stat.S_IFDIR),
                    ("a", stat.S_IFDIR),
                    ("adir", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                    ("z", stat.S_IFDIR),
                    ("z/x", stat.S_IFREG),
                    ("z/y", stat.S_IFREG),
                },
                waitTime=0,
            )

            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=3
            )
            self.assertAllMaterialized(
                {
                    ("a/b/x", stat.S_IFREG),
                    ("a/b/y", stat.S_IFREG),
                    ("a/b", stat.S_IFDIR),
                    ("a", stat.S_IFDIR),
                    ("adir", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                }
            )

    def test_rename_twice(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.mkdir("first")
            self.touch("first/a")
            self.mkdir("first/b")

            self.mkdir("second")
            self.touch("second/c")
            self.touch("second/d")

            self.rename("first", "third")
            self.rename("second", "first")
            self.rename("third", "second")

            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=12
            )

            self.assertAllMaterialized(
                {
                    ("adir", stat.S_IFDIR),
                    ("first", stat.S_IFDIR),
                    ("first/c", stat.S_IFREG),
                    ("first/d", stat.S_IFREG),
                    ("second", stat.S_IFDIR),
                    ("second/a", stat.S_IFREG),
                    ("second/b", stat.S_IFDIR),
                    ("hello", stat.S_IFREG),
                }
            )

    def unmount(self) -> None:
        self.eden.unmount(self.mount_path)

    def wait_until_unmount_started(self) -> None:
        """Wait until reading a directory starts failing and raising an
        exception. This is a sign that either EdenFS is in the process of
        unmounting, or EdenFS crashed.
        """
        while True:
            try:
                self.read_dir("adir")
                time.sleep(0.1)
                continue
            except Exception:
                break

    def test_unmount_with_ongoing_notification(self) -> None:
        with self.run_with_blocking_fault(
            keyClass="PrjfsDispatcherImpl::fileNotification"
        ):
            self.touch("adir/a")

            unmount_thread = Thread(target=self.unmount)
            unmount_thread.start()

            self.wait_until_unmount_started()
            self.wait_on_fault_unblock(
                keyClass="PrjfsDispatcherImpl::fileNotification", numToUnblock=1
            )

            unmount_thread.join(timeout=30.0)

            self.assertTrue(self.eden.is_healthy())

    def test_truncate(self) -> None:
        rel_path = "adir/file"
        path = self.mount_path / rel_path

        self.assertNotMaterialized(rel_path)
        subprocess.run(["powershell.exe", "Clear-Content", str(path)])

        # file should be materialized at this point.
        self.assertMaterialized(rel_path, stat.S_IFREG)

        st = os.lstat(path)
        with path.open("rb") as f:
            read_back = f.read().decode()
        self.assertEqual("", read_back)

        self.assertEqual(0, st.st_size)

    def test_read_and_unmount(self) -> None:
        rel_path = "adir/file"
        path = self.mount_path / rel_path

        # Lays a placeholder on disk
        os.lstat(path)

        def read_file() -> None:
            with self.run_with_blocking_fault("PrjfsDispatcherImpl::read"):
                with path.open("rb") as f:
                    f.read()

        read_thread = Thread(target=read_file)
        read_thread.start()

        unmount_thread = Thread(target=self.unmount)
        unmount_thread.start()

        self.wait_until_unmount_started()
        self.wait_on_fault_unblock(keyClass="PrjfsDispatcherImpl::read")
        read_thread.join(timeout=30.0)
        unmount_thread.join(timeout=30.0)

        self.assertTrue(self.eden.is_healthy())


@testcase.eden_repo_test
class PrjfsStressNoListenToFull(PrjFSStressBase):
    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("prjfs", []).append("listen-to-pre-convert-to-full = false")
        return result

    # this test should start failing once msft fixes the bug on their side.
    # i.e. once truncation starts to send file closed and modified notifications.
    def test_truncate(self) -> None:
        rel_path = "adir/file"
        path = self.mount_path / rel_path

        self.assertNotMaterialized(rel_path)
        subprocess.run(["powershell.exe", "Clear-Content", str(path)])

        # file should be materialized at this point.
        self.assertNotMaterialized(rel_path, stat.S_IFREG)

        st = os.lstat(path)
        with path.open("rb") as f:
            read_back = f.read().decode()
        self.assertEqual("", read_back)

        self.assertEqual(0, st.st_size)
