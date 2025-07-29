#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import re
from typing import Dict, Optional

from eden.fs.cli.util import poll_until_async

from .lib import testcase


@testcase.eden_nfs_repo_test
class RocksDBStoreTest(testcase.HgRepoTestMixin, testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("a/dir/foo.txt", "foo\n")
        self.repo.write_file("a/dir/bar.txt", "bar\n")
        self.repo.write_file("a/another_dir/hello.txt", "hola\n")
        self.repo.commit("Initial commit.")

    def select_storage_engine(self) -> str:
        return "rocksdb"

    async def test_local_store_stats(self) -> None:
        # Update the config to tell the local store to updates its stats frequently
        # and also check if it needs to reload the config file frequently.
        initial_config = """\
[config]
reload-interval = "100ms"

[store]
stats-interval = "100ms"
"""
        self.eden.user_rc_path.write_text(initial_config)

        counter_regex = r"local_store\..*"
        async with self.get_thrift_client() as client:
            # Makes sure that EdenFS picks up our updated config,
            # since we wrote it out after EdenFS started.
            await client.reloadConfig()

            # Get the local store counters
            # Assert that the exist and are greater than 0.
            # (Since we include memtable sizes in the values these are currently always
            # reported as taking up at least a small amount of space.)
            initial_counters = await client.getRegexCounters(counter_regex)

            initial_blob_size = initial_counters.get("local_store.blob.size")
            initial_blobmeta_size = initial_counters.get("local_store.blobmeta.size")
            initial_tree_size = initial_counters.get("local_store.tree.size")
            initial_hgcommit2tree_size = initial_counters.get(
                "local_store.hgcommit2tree.size"
            )
            initial_hgproxyhash_size = initial_counters.get(
                "local_store.hgproxyhash.size"
            )
            initial_ephemeral_size = initial_counters.get(
                "local_store.ephemeral.total_size"
            )
            initial_persistent_size = initial_counters.get(
                "local_store.persistent.total_size"
            )

            self.assertIsNotNone(initial_blob_size)
            self.assertIsNotNone(initial_blobmeta_size)
            self.assertIsNotNone(initial_tree_size)
            self.assertIsNotNone(initial_hgcommit2tree_size)
            self.assertIsNotNone(initial_hgproxyhash_size)
            self.assertIsNotNone(initial_ephemeral_size)
            self.assertIsNotNone(initial_persistent_size)

            self.assertGreater(initial_blob_size, 0)
            self.assertGreater(initial_blobmeta_size, 0)
            self.assertGreater(initial_tree_size, 0)
            self.assertGreater(initial_hgcommit2tree_size, 0)
            self.assertGreater(initial_hgproxyhash_size, 0)
            self.assertGreater(initial_ephemeral_size, 0)
            self.assertGreater(initial_persistent_size, 0)

            # Make sure the counters are less than 500MB, just as a sanity check
            self.assertLess(initial_ephemeral_size, 500_000_000)
            self.assertLess(initial_persistent_size, 500_000_000)

            # Read back several files
            self.assertEqual((self.mount_path / "a/dir/foo.txt").read_text(), "foo\n")
            self.assertEqual((self.mount_path / "a/dir/bar.txt").read_text(), "bar\n")
            self.assertEqual(
                (self.mount_path / "a/another_dir/hello.txt").read_text(), "hola\n"
            )

            # The tree store size should be larger now after reading these files.
            # The counters won't be updated until the store.stats-interval expires.
            # Wait for this to happen.
            async def tree_size_incremented() -> Optional[bool]:
                tree_size = await client.getCounter("local_store.tree.size")

                if tree_size > initial_tree_size:
                    return True

                return None

            await poll_until_async(tree_size_incremented, timeout=10, interval=0.1)

            blob_size = await client.getCounter("local_store.blob.size")

            # EdenFS should not import blobs to local store
            self.assertEqual(
                initial_blob_size,
                blob_size,
            )

            # Update the config file with a very small GC limit that will force GC to be
            # triggered
            self.eden.user_rc_path.write_text(
                initial_config
                + """
blob-size-limit = "1"
blobmeta-size-limit = "1"
tree-size-limit = "1"
hgcommit2tree-size-limit = "1"
"""
            )

            # Wait until a GC run has completed.
            async def gc_run_succeeded() -> Optional[Dict[str, int]]:
                counters = await client.getRegexCounters(counter_regex)
                if counters.get("local_store.auto_gc.last_run_succeeded") is not None:
                    return counters
                return None

            counters = await poll_until_async(
                gc_run_succeeded, timeout=30, interval=0.05
            )

            # Check the local_store.auto_gc counters

            auto_gc_last_run_succeeded = counters.get(
                "local_store.auto_gc.last_run_succeeded"
            )
            auto_gc_success = counters.get("local_store.auto_gc.success")
            auto_gc_failure = counters.get("local_store.auto_gc.failure")
            auto_gc_last_duration_ms = counters.get(
                "local_store.auto_gc.last_duration_ms"
            )

            self.assertIsNone(auto_gc_failure)

            self.assertIsNotNone(auto_gc_last_run_succeeded)
            self.assertIsNotNone(auto_gc_success)
            self.assertIsNotNone(auto_gc_last_duration_ms)

            self.assertEqual(auto_gc_last_run_succeeded, 1)
            self.assertGreater(auto_gc_success, 0)
            self.assertGreaterEqual(auto_gc_last_duration_ms, 0)

        # Run "eden stats local-store" and check the output
        stats_output = self.eden.run_cmd("stats", "local-store")

        m = re.search(r"Successful Auto-GC Runs:\s+(\d+)", stats_output)
        self.assertIsNotNone(m)
        self.assertGreater(int(m.group(1)), 0)

        self.assertRegex(stats_output, r"Last Auto-GC Result:\s+Success")
        self.assertRegex(stats_output, r"Failed Auto-GC Runs:\s+0")
        self.assertRegex(stats_output, r"Total Ephemeral Size:")
        self.assertRegex(stats_output, r"Total Persistent Size:")
