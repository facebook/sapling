#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging
import os
import sys
import time
import typing
from pathlib import Path, PurePath

from facebook.eden.constants import STATS_MOUNTS_STATS
from facebook.eden.ttypes import (
    GetStatInfoParams,
    JournalInfo,
    SynchronizeWorkingCopyParams,
    TimeSpec,
)

from .lib import testcase
from .lib.hgrepo import HgRepository


Counters = typing.Mapping[str, float]

logger = logging.getLogger(__name__)


@testcase.eden_test
class GenericStatsTest(testcase.EdenRepoTest):
    def protocol_type(self) -> str:
        if sys.platform == "linux" or sys.platform == "darwin":
            return "nfs" if self.use_nfs() else "fuse"
        else:
            return "prjfs"

    def test_reading_committed_file_bumps_read_counter(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "file"
        path.read_bytes()

        counter_name = self.protocol_type() + ".read_us.count"
        self.poll_until_counter_condition(
            lambda counters_after: self.assertGreater(
                counters_after[counter_name],
                counters_before.get(counter_name, 0),
                f"Reading {path} should increment {counter_name}",
            )
        )

    def test_writing_untracked_file_bumps_write_counter(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "new_file"
        path.write_bytes(b"hello")

        counter_name = self.protocol_type() + ".write_us.count"
        self.poll_until_counter_condition(
            lambda counters_after: self.assertGreater(
                counters_after[counter_name],
                counters_before.get(counter_name, 0),
                f"Writing to {path} should increment {counter_name}",
            )
        )

    def test_summary_counters_available(self) -> None:
        mountName = PurePath(self.mount).name
        protocol_name = self.protocol_type()
        counter_names_to_check = [
            f"{protocol_name}.{mountName}.live_requests.count",
            f"{protocol_name}.{mountName}.live_requests.max_duration_us",
            f"{protocol_name}.{mountName}.pending_requests.count",
        ]

        counters = self.get_counters()

        for counter_name in counter_names_to_check:
            self.assertIn(counter_name, counters, f"{counter_name} should be available")

    def create_repo(self, name: str) -> HgRepository:
        return self.create_hg_repo(name)

    def populate_repo(self) -> None:
        self.repo.write_file("file", "hello world!\n")
        self.repo.commit("Initial commit.")

    def poll_until_counter_condition(
        self, assertion_condition: typing.Callable[[Counters], None]
    ) -> None:
        timeout_seconds = 2.0
        poll_interval_seconds = 0.1
        deadline = time.monotonic() + timeout_seconds
        while True:
            counters = self.get_counters()
            try:
                assertion_condition(counters)
                break
            except AssertionError as e:
                if time.monotonic() >= deadline:
                    raise
                logger.info(
                    f"Assertion failed. Waiting {poll_interval_seconds} "
                    f"seconds before trying again. {e}"
                )
                time.sleep(poll_interval_seconds)
                continue


@testcase.eden_nfs_repo_test
class ObjectStoreStatsTest(testcase.EdenRepoTest):
    def create_repo(self, name: str) -> HgRepository:
        return self.create_hg_repo(name)

    def populate_repo(self) -> None:
        self.repo.write_file("foo.txt", "foo\n")

        self.repo.commit("Initial commit.")

    def test_get_blob(self) -> None:
        TEMPLATE = "object_store.get_blob.{}_store.count"
        LOCAL = TEMPLATE.format("local")
        BACKING = TEMPLATE.format("backing")

        counters = self.get_counters()
        self.assertEqual(counters.get(LOCAL, 0) + counters.get(BACKING, 0), 0)

        foo = Path(self.mount) / "foo.txt"
        foo.read_bytes()

        counters = self.get_counters()
        self.assertEqual(counters.get(LOCAL, 0) + counters.get(BACKING, 0), 1)


@testcase.eden_test
class ObjectCacheStatsTest(testcase.EdenRepoTest):
    def create_repo(self, name: str) -> HgRepository:
        return self.create_hg_repo(name)

    def populate_repo(self) -> None:
        self.repo.mkdir("dir")
        self.repo.write_file("dir/one.txt", "1\n")
        self.repo.write_file("dir/two.txt", "2\n")

        self.repo.commit("Initial commit.")

    def test_get_tree_memory(self) -> None:
        MEMORY_COUNTER = "object_store.get_tree.memory.count"

        list(os.scandir(Path(self.mount) / "dir"))

        initial_count = self.get_counters().get(MEMORY_COUNTER, 0)

        # To exercise the in-memory tree cache we have to first unload the
        # corresponding inodes (which contains its own cache of directory
        # entries) and OS kernel caches.
        with self.get_thrift_client_legacy() as thrift_client:
            thrift_client.unloadInodeForPath(
                self.mount.encode("utf-8"), b"", TimeSpec(0, 0)
            )

            thrift_client.invalidateKernelInodeCache(self.mount.encode("utf-8"), b"dir")

        # List the directory again, which should result in a TreeCache hit this
        # time around.  We use os.scandir because Path.glob seems to do some
        # caching of its own?
        list(os.scandir(Path(self.mount) / "dir"))

        final_count = self.get_counters().get(MEMORY_COUNTER, 0)
        self.assertTrue(final_count > initial_count)


@testcase.eden_nfs_repo_test
class HgBackingStoreStatsTest(testcase.EdenRepoTest):
    def test_reading_file_gets_file_from_hg(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "dir" / "subdir" / "file"
        path.read_bytes()
        counters_after = self.get_counters()

        self.assertEqual(
            counters_after["store.hg.get_blob_us.count"],
            counters_before.get("store.hg.get_blob_us.count", 0) + 1,
            f"Reading {path} should increment store.hg.get_blob_us.count",
        )

    def test_pending_import_counters_available(self) -> None:
        counters = self.get_counters()

        counter_names_to_check = [
            "store.hg.pending_import.blob.count",
            "store.hg.pending_import.tree.count",
            "store.hg.pending_import.prefetch.count",
            "store.hg.pending_import.count",
            "store.hg.pending_import.blob.max_duration_us",
            "store.hg.pending_import.tree.max_duration_us",
            "store.hg.pending_import.prefetch.max_duration_us",
            "store.hg.pending_import.max_duration_us",
            "store.hg.live_import.blob.count",
            "store.hg.live_import.tree.count",
            "store.hg.live_import.prefetch.count",
            "store.hg.live_import.count",
            "store.hg.live_import.blob.max_duration_us",
            "store.hg.live_import.tree.max_duration_us",
            "store.hg.live_import.prefetch.max_duration_us",
            "store.hg.live_import.max_duration_us",
        ]

        for counter_name in counter_names_to_check:
            self.assertIn(counter_name, counters, f"{counter_name} should be available")

    def create_repo(self, name: str) -> HgRepository:
        return self.create_hg_repo(name)

    def populate_repo(self) -> None:
        # This file evades EdenFS' automatic prefetching by being two levels
        # inside the root.
        self.repo.write_file("dir/subdir/file", "hello world!\n")

        self.repo.commit("Initial commit.")


@testcase.eden_nfs_repo_test
class HgImporterStatsTest(testcase.EdenRepoTest):
    def test_reading_file_imports_blob(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "dir" / "subdir" / "file"
        path.read_bytes()
        counters_after = self.get_counters()

        self.assertEqual(
            counters_after["store.hg.get_blob_us.count"],
            counters_before.get("store.hg.get_blob_us.count", 0) + 1,
            f"Reading {path} should increment store.hg.get_blob_us.count",
        )

    def create_repo(self, name: str) -> HgRepository:
        return self.create_hg_repo(name)

    def populate_repo(self) -> None:
        # This file evades EdenFS' automatic prefetching by being two levels
        # inside the root.
        self.repo.write_file("dir/subdir/file", "hello world!\n")

        self.repo.commit("Initial commit.")


@testcase.eden_repo_test
class JournalInfoTest(testcase.EdenRepoTest):
    def test_journal_info(self) -> None:
        journal = self.journal_stats()
        old_mem = journal.memoryUsage
        old_data_counts = journal.entryCount
        path = Path(self.mount) / "new_file"
        path.write_bytes(b"hello")
        journal = self.journal_stats()
        new_mem = journal.memoryUsage
        new_data_counts = journal.entryCount
        self.assertLess(
            old_data_counts,
            new_data_counts,
            "Changing the repo should cause entry count to increase",
        )
        self.assertLess(
            old_mem, new_mem, "Changing the repo should cause memory usage to increase"
        )

    def journal_stats(self) -> JournalInfo:
        with self.get_thrift_client_legacy() as thrift_client:
            thrift_client.synchronizeWorkingCopy(
                self.mount.encode("utf-8"), SynchronizeWorkingCopyParams()
            )

            stats = thrift_client.getStatInfo(
                GetStatInfoParams(statsMask=STATS_MOUNTS_STATS)
            )
            journal_key = self.mount.encode()
            mountPointJournalInfo = stats.mountPointJournalInfo
            journal = (
                None
                if mountPointJournalInfo is None
                else mountPointJournalInfo[journal_key]
            )
            self.assertIsNotNone(journal, "Journal does not exist")
            return journal

    def populate_repo(self) -> None:
        self.repo.write_file("file", "hello world!\n")
        self.repo.commit("Initial commit.")


@testcase.eden_repo_test
class CountersTest(testcase.EdenRepoTest):
    """Test counters are registered/unregistered correctly."""

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    # We get rid of the thrift and scribe counters since they sporadically
    # appear and can cause this test to fail (since they can appear between
    # counters and counters2)
    @staticmethod
    def get_nonthrift_set(s):
        # and memory_vm_rss_bytes is reported sporadically in the background
        return {
            item
            for item in s
            if not item.startswith("scribe.")
            and not item.startswith("thrift.")
            and not item.startswith("memory_vm_rss_bytes")
        }

    def test_mount_unmount_counters(self) -> None:
        self.eden.unmount(self.mount_path)
        counters = self.get_nonthrift_set(self.get_counters().keys())
        mount2 = os.path.join(self.mounts_dir, "mount2")
        self.eden.clone(self.repo.path, mount2)
        self.eden.unmount(Path(mount2))
        counters2 = self.get_nonthrift_set(self.get_counters().keys())
        self.assertEqual(counters, counters2)
