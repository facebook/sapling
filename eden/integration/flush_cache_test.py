#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import sys

from .lib import testcase


@testcase.eden_repo_test
class FlushCacheTest(testcase.EdenRepoTest):
    """Exercise the invalidateKernelInodeCache thrift endpoint, which is
    exposed to users as `eden debug flush_cache`."""

    num_files: int = 10

    def populate_repo(self) -> None:
        for i in range(self.num_files):
            self.repo.write_file(f"a/{i}", f"{i}\n")
        self.repo.commit("Initial commit.")

    async def test_flush_directory_with_unloaded_children(self) -> None:
        if sys.platform == "win32":
            self.skipTest("flushing a directory is not supported on PrjFS")

        # Load the directory's own inode without loading its children: a
        # lookup of "a" is enough. Having unloaded children is the state
        # invalidateKernelInodeCache must cope with: the NFS flavor loads
        # every child of the directory while invalidating it.
        os.lstat(self.get_path("a"))

        # The timeout matters: the failure mode guarded against here is the
        # RPC deadlocking against itself and never completing, which also
        # wedges the directory until the daemon is restarted.
        async with self.eden.get_async_thrift_client(timeout=30) as client:
            await client.invalidateKernelInodeCache(self.mount_path_bytes, b"a")

        for i in range(self.num_files):
            self.assertEqual(self.read_file(f"a/{i}"), f"{i}\n")
