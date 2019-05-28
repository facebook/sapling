#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import typing
from pathlib import Path

from .lib import testcase
from .lib.hgrepo import HgRepository


class FUSEStatsTest(testcase.EdenRepoTest):
    def test_reading_committed_file_bumps_read_counter(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "file"
        path.read_bytes()
        counters_after = self.get_counters()

        self.assertGreater(
            counters_after.get("fuse.read_us.count", 0),
            counters_before.get("fuse.read_us.count", 0),
            f"Reading {path} should increment fuse.read_us.count",
        )

    def test_writing_untracked_file_bumps_write_counter(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "new_file"
        path.write_bytes(b"hello")
        counters_after = self.get_counters()

        self.assertGreater(
            counters_after.get("fuse.write_us.count", 0),
            counters_before.get("fuse.write_us.count", 0),
            f"Writing to {path} should increment fuse.write_us.count",
        )

    def create_repo(self, name: str) -> HgRepository:
        return self.create_hg_repo(name)

    def populate_repo(self) -> None:
        self.repo.write_file("file", "hello world!\n")
        self.repo.commit("Initial commit.")


class HgBackingStoreStatsTest(testcase.EdenRepoTest):
    def test_reading_file_gets_file_from_hg(self) -> None:
        counters_before = self.get_counters()
        path = Path(self.mount) / "dir" / "subdir" / "file"
        path.read_bytes()
        counters_after = self.get_counters()

        self.assertEqual(
            counters_after.get("store.hg.get_file.count", 0),
            counters_before.get("store.hg.get_file.count", 0) + 1,
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
