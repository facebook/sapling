#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging
import os
import time
import typing
from pathlib import Path

from facebook.eden.ttypes import JournalInfo

from .lib import testcase
from .lib.hgrepo import HgRepository


Counters = typing.Mapping[str, float]

logger = logging.getLogger(__name__)


class FUSEStatsTest(testcase.EdenRepoTest):
    def test_reading_committed_file_bumps_read_counter(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "file"
        path.read_bytes()

        self.poll_until_counter_condition(
            lambda counters_after: self.assertGreater(
                counters_after.get("fuse.read_us.count", 0),
                counters_before.get("fuse.read_us.count", 0),
                f"Reading {path} should increment fuse.read_us.count",
            )
        )

    def test_writing_untracked_file_bumps_write_counter(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "new_file"
        path.write_bytes(b"hello")

        self.poll_until_counter_condition(
            lambda counters_after: self.assertGreater(
                counters_after.get("fuse.write_us.count", 0),
                counters_before.get("fuse.write_us.count", 0),
                f"Writing to {path} should increment fuse.write_us.count",
            )
        )

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


class ObjectStoreStatsTest(testcase.EdenRepoTest):
    def create_repo(self, name: str) -> HgRepository:
        return self.create_hg_repo(name)

    def populate_repo(self) -> None:
        self.repo.write_file("foo.txt", "foo\n")
        self.repo.commit("Initial commit.")

    def test_get_blob(self) -> None:
        TEMPLATE = "object_store.get_blob.{}_store.pct"
        LOCAL = TEMPLATE.format("local")
        BACKING = TEMPLATE.format("backing")

        counters = self.get_counters()
        self.assertEqual(counters.get(LOCAL, 0) + counters.get(BACKING, 0), 0)

        foo = Path(self.mount) / "foo.txt"
        foo.read_bytes()

        counters = self.get_counters()
        self.assertEqual(counters.get(LOCAL, 0) + counters.get(BACKING, 0), 100)

    def test_get_blob_size(self) -> None:
        TEMPLATE = "object_store.get_blob_size.{}_store.pct"
        LOCAL = TEMPLATE.format("local")
        BACKING = TEMPLATE.format("backing")

        counters = self.get_counters()
        self.assertEqual(counters.get(LOCAL, 0) + counters.get(BACKING, 0), 0)

        foo = Path(self.mount) / "foo.txt"
        foo.stat()

        counters = self.get_counters()
        self.assertEqual(counters.get(LOCAL, 0) + counters.get(BACKING, 0), 100)


class HgBackingStoreStatsTest(testcase.EdenRepoTest):
    def test_reading_file_gets_file_from_hg(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "dir" / "subdir" / "file"
        path.read_bytes()
        counters_after = self.get_counters()

        self.assertEqual(
            counters_after.get("store.hg.get_blob.count", 0),
            counters_before.get("store.hg.get_blob.count", 0) + 1,
            f"Reading {path} should increment store.hg.get_file.count",
        )

    def create_repo(self, name: str) -> HgRepository:
        return self.create_hg_repo(name)

    def populate_repo(self) -> None:
        # This file evades EdenFS' automatic prefetching by being two levels
        # inside the root.
        self.repo.write_file("dir/subdir/file", "hello world!\n")

        self.repo.commit("Initial commit.")


class HgImporterStatsTest(testcase.EdenRepoTest):
    def test_reading_file_imports_blob(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "dir" / "subdir" / "file"
        path.read_bytes()
        counters_after = self.get_counters()

        self.assertEqual(
            counters_after.get("hg_importer.cat_file.count", 0),
            counters_before.get("hg_importer.cat_file.count", 0) + 1,
            f"Reading {path} should increment hg_importer.cat_file.count",
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
        with self.get_thrift_client() as thrift_client:
            stats = thrift_client.getStatInfo()
            journal_key = self.mount.encode()
            journal = stats.mountPointJournalInfo[journal_key]
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

    # We get rid of the thrift counters since they sporadically appear and can
    # cause this test to fail (since they can appear between counters and counters2)
    @staticmethod
    def get_nonthrift_set(s):
        return {item for item in s if not item.startswith("thrift")}

    def test_mount_unmount_counters(self) -> None:
        self.eden.unmount(self.mount_path)
        counters = self.get_nonthrift_set(self.get_counters().keys())
        mount2 = os.path.join(self.mounts_dir, "mount2")
        self.eden.clone(self.repo_name, mount2)
        self.eden.unmount(Path(mount2))
        counters2 = self.get_nonthrift_set(self.get_counters().keys())
        self.assertEqual(counters, counters2)
