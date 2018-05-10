#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import hashlib
import os

from facebook.eden.ttypes import ScmFileStatus, SHA1Result, TimeSpec

from .lib import testcase


@testcase.eden_repo_test
class ThriftTest(testcase.EdenRepoTest):

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("README", "docs\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/file", "bar!\n")
        self.repo.symlink("slink", "hello")
        self.commit1 = self.repo.commit("Initial commit.")

        self.repo.write_file("bdir/file", "bar?\n")
        self.repo.write_file("cdir/subdir/new.txt", "and improved")
        self.repo.remove_file("README")
        self.commit2 = self.repo.commit("Commit 2.")

    def setUp(self) -> None:
        super().setUp()
        self.client = self.get_thrift_client()
        self.client.open()
        self.addCleanup(self.client.close)

    def get_loaded_inodes_count(self, path: str) -> int:
        result = self.client.debugInodeStatus(self.mount, path)
        inode_count = 0
        for item in result:
            assert item.entries is not None
            for inode in item.entries:
                if inode.loaded:
                    inode_count += 1
        return inode_count

    def test_list_mounts(self) -> None:
        mounts = self.client.listMounts()
        self.assertEqual(1, len(mounts))

        mount = mounts[0]
        self.assertEqual(self.mount, mount.mountPoint)
        assert mount.edenClientPath is not None
        # The client path should always be inside the main eden directory
        self.assertTrue(
            mount.edenClientPath.startswith(self.eden.eden_dir),
            msg="unexpected client path: %r" % mount.edenClientPath,
        )

    def test_get_sha1(self) -> None:
        expected_sha1_for_hello = hashlib.sha1(b"hola\n").digest()
        result_for_hello = SHA1Result()
        result_for_hello.set_sha1(expected_sha1_for_hello)

        expected_sha1_for_adir_file = hashlib.sha1(b"foo!\n").digest()
        result_for_adir_file = SHA1Result()
        result_for_adir_file.set_sha1(expected_sha1_for_adir_file)

        self.assertEqual(
            [result_for_hello, result_for_adir_file],
            self.client.getSHA1(self.mount, ["hello", "adir/file"]),
        )

    def test_get_sha1_throws_for_empty_string(self) -> None:
        results = self.client.getSHA1(self.mount, [""])
        self.assertEqual(1, len(results))
        self.assert_error(results[0], "path cannot be the empty string")

    def test_get_sha1_throws_for_directory(self) -> None:
        results = self.client.getSHA1(self.mount, ["adir"])
        self.assertEqual(1, len(results))
        self.assert_error(results[0], "adir: Is a directory")

    def test_get_sha1_throws_for_non_existent_file(self) -> None:
        results = self.client.getSHA1(self.mount, ["i_do_not_exist"])
        self.assertEqual(1, len(results))
        self.assert_error(results[0], "i_do_not_exist: No such file or directory")

    def test_get_sha1_throws_for_symlink(self) -> None:
        """Fails because caller should resolve the symlink themselves."""
        results = self.client.getSHA1(self.mount, ["slink"])
        self.assertEqual(1, len(results))
        self.assert_error(results[0], "slink: file is a symlink: Invalid argument")

    def assert_error(self, sha1result: SHA1Result, error_message: str) -> None:
        self.assertIsNotNone(sha1result, msg="Must pass a SHA1Result")
        self.assertEqual(
            SHA1Result.ERROR, sha1result.getType(), msg="SHA1Result must be an error"
        )
        error = sha1result.get_error()
        self.assertIsNotNone(error)
        self.assertEqual(error_message, error.message)

    def test_unload_free_inodes(self) -> None:
        for i in range(100):
            self.write_file("testfile%d.txt" % i, "unload test case")

        inode_count_before_unload = self.get_loaded_inodes_count("")
        self.assertGreater(
            inode_count_before_unload, 100, "Number of loaded inodes should increase"
        )

        age = TimeSpec()
        age.seconds = 0
        age.nanoSeconds = 0
        unload_count = self.client.unloadInodeForPath(self.mount, "", age)

        self.assertGreaterEqual(
            unload_count, 100, "Number of loaded inodes should reduce after unload"
        )

    def get_counter(self, name: str) -> int:
        self.client.flushStatsNow()
        return self.client.getCounters()[name]

    def test_invalidate_inode_cache(self) -> None:
        filename = "bdir/file"
        full_dirname = os.path.join(self.mount, "bdir/")

        # Exercise eden a bit to make sure counters are ready
        for _ in range(20):
            fn = os.path.join(self.mount, "_tmp_")
            with open(fn, "w") as f:
                f.write("foo!\n")
            os.unlink(fn)

        reads = self.get_counter("fuse.read_us.count")
        self.read_file(filename)
        reads_1read = self.get_counter("fuse.read_us.count")
        self.assertEqual(reads_1read, reads + 1)
        self.read_file(filename)
        reads_2read = self.get_counter("fuse.read_us.count")
        self.assertEqual(reads_1read, reads_2read)
        self.client.invalidateKernelInodeCache(self.mount, "bdir/file")
        self.read_file(filename)
        reads_3read = self.get_counter("fuse.read_us.count")
        self.assertEqual(reads_2read + 1, reads_3read)

        lookups = self.get_counter("fuse.lookup_us.count")
        # -hl makes ls to do a lookup of the file to determine type
        os.system("ls -hl " + full_dirname + " > /dev/null")
        lookups_1ls = self.get_counter("fuse.lookup_us.count")
        # equal, the file was lookup'ed above.
        self.assertEqual(lookups, lookups_1ls)
        self.client.invalidateKernelInodeCache(self.mount, "bdir")
        os.system("ls -hl " + full_dirname + " > /dev/null")
        lookups_2ls = self.get_counter("fuse.lookup_us.count")
        self.assertEqual(lookups_1ls + 1, lookups_2ls)

    def test_diff_revisions(self) -> None:
        # Convert the commit hashes to binary for the thrift call
        with self.get_thrift_client() as client:
            diff = client.getScmStatusBetweenRevisions(
                self.mount,
                binascii.unhexlify(self.commit1),
                binascii.unhexlify(self.commit2),
            )

        self.assertDictEqual(diff.errors, {})
        self.assertDictEqual(
            diff.entries,
            {
                "cdir/subdir/new.txt": ScmFileStatus.ADDED,
                "bdir/file": ScmFileStatus.MODIFIED,
                "README": ScmFileStatus.REMOVED,
            },
        )

    def test_diff_revisions_hex(self) -> None:
        # Watchman currently clals getScmStatusBetweenRevisions()
        # with 40-byte hexadecimal commit IDs, so make sure that works.
        with self.get_thrift_client() as client:
            diff = client.getScmStatusBetweenRevisions(
                self.mount, self.commit1, self.commit2
            )

        self.assertDictEqual(diff.errors, {})
        self.assertDictEqual(
            diff.entries,
            {
                "cdir/subdir/new.txt": ScmFileStatus.ADDED,
                "bdir/file": ScmFileStatus.MODIFIED,
                "README": ScmFileStatus.REMOVED,
            },
        )
