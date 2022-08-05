#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import Dict, Optional

from eden.fs.cli.util import poll_until

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

    def test_local_store_stats(self) -> None:
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
        with self.get_thrift_client_legacy() as client:
            # Makes sure that EdenFS picks up our updated config,
            # since we wrote it out after EdenFS started.
            client.reloadConfig()

            # Get the local store counters
            # Assert that the exist and are greater than 0.
            # (Since we include memtable sizes in the values these are currently always
            # reported as taking up at least a small amount of space.)
            initial_counters = client.getRegexCounters(counter_regex)
            # pyre-fixme[6]: For 2nd param expected `SupportsDunderLT[Variable[_T]]`
            #  but got `int`.
            self.assertGreater(initial_counters.get("local_store.blob.size"), 0)
            # pyre-fixme[6]: For 2nd param expected `SupportsDunderLT[Variable[_T]]`
            #  but got `int`.
            self.assertGreater(initial_counters.get("local_store.blobmeta.size"), 0)
            # pyre-fixme[6]: For 2nd param expected `SupportsDunderLT[Variable[_T]]`
            #  but got `int`.
            self.assertGreater(initial_counters.get("local_store.tree.size"), 0)
            self.assertGreater(
                initial_counters.get("local_store.hgcommit2tree.size"),
                # pyre-fixme[6]: For 2nd param expected
                #  `SupportsDunderLT[Variable[_T]]` but got `int`.
                0,
            )
            # pyre-fixme[6]: For 2nd param expected `SupportsDunderLT[Variable[_T]]`
            #  but got `int`.
            self.assertGreater(initial_counters.get("local_store.hgproxyhash.size"), 0)
            self.assertGreater(
                initial_counters.get("local_store.ephemeral.total_size"),
                # pyre-fixme[6]: For 2nd param expected
                #  `SupportsDunderLT[Variable[_T]]` but got `int`.
                0,
            )
            self.assertGreater(
                initial_counters.get("local_store.persistent.total_size"),
                # pyre-fixme[6]: For 2nd param expected
                #  `SupportsDunderLT[Variable[_T]]` but got `int`.
                0,
            )
            # Make sure the counters are less than 500MB, just as a sanity check
            self.assertLess(
                initial_counters.get("local_store.ephemeral.total_size"),
                # pyre-fixme[6]: For 2nd param expected
                #  `SupportsDunderGT[Variable[_T]]` but got `int`.
                500_000_000,
            )
            self.assertLess(
                initial_counters.get("local_store.persistent.total_size"),
                # pyre-fixme[6]: For 2nd param expected
                #  `SupportsDunderGT[Variable[_T]]` but got `int`.
                500_000_000,
            )

            # Read back several files
            self.assertEqual((self.mount_path / "a/dir/foo.txt").read_text(), "foo\n")
            self.assertEqual((self.mount_path / "a/dir/bar.txt").read_text(), "bar\n")
            self.assertEqual(
                (self.mount_path / "a/another_dir/hello.txt").read_text(), "hola\n"
            )

            # The tree store size should be larger now after reading these files.
            # The counters won't be updated until the store.stats-interval expires.
            # Wait for this to happen.
            def tree_size_incremented() -> Optional[bool]:
                tree_size = client.getCounter("local_store.tree.size")

                initial_tree_size = initial_counters.get("local_store.tree.size")
                assert initial_tree_size is not None
                if tree_size > initial_tree_size:
                    return True

                return None

            poll_until(tree_size_incremented, timeout=10, interval=0.1)

            # EdenFS should not import blobs to local store
            self.assertEqual(
                initial_counters.get("local_store.blob.size"),
                client.getCounter("local_store.blob.size"),
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
            def gc_run_succeeded() -> Optional[Dict[str, int]]:
                counters = client.getRegexCounters(counter_regex)
                if counters.get("local_store.auto_gc.last_run_succeeded") is not None:
                    return counters
                return None

            counters = poll_until(gc_run_succeeded, timeout=30, interval=0.05)

            # Check the local_store.auto_gc counters
            self.assertEqual(counters.get("local_store.auto_gc.last_run_succeeded"), 1)
            # pyre-fixme[6]: For 2nd param expected `SupportsDunderLT[Variable[_T]]`
            #  but got `int`.
            self.assertGreater(counters.get("local_store.auto_gc.success"), 0)
            self.assertEqual(counters.get("local_store.auto_gc.failure", 0), 0)
            self.assertGreaterEqual(
                counters.get("local_store.auto_gc.last_duration_ms"),
                # pyre-fixme[6]: For 2nd param expected
                #  `SupportsDunderLE[Variable[_T]]` but got `int`.
                0,
            )

        # Run "eden stats local-store" and check the output
        stats_output = self.eden.run_cmd("stats", "local-store")
        print(stats_output)
        m = re.search(r"Successful Auto-GC Runs:\s+(\d+)", stats_output)
        self.assertIsNotNone(m)
        assert m is not None  # make the type checker happy
        self.assertGreater(int(m.group(1)), 0)

        self.assertRegex(stats_output, r"Last Auto-GC Result:\s+Success")
        self.assertRegex(stats_output, r"Failed Auto-GC Runs:\s+0")
        self.assertRegex(stats_output, r"Total Ephemeral Size:")
        self.assertRegex(stats_output, r"Total Persistent Size:")
