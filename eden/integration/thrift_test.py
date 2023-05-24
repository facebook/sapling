#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import binascii
import hashlib
import os
import re
import socket
import subprocess
import sys
from pathlib import Path
from typing import List, Optional, Pattern, Tuple, TypeVar, Union

from facebook.eden.ttypes import (
    DirListAttributeDataOrError,
    EdenError,
    EdenErrorType,
    FileAttributeData,
    FileAttributeDataOrError,
    FileAttributeDataOrErrorV2,
    FileAttributeDataV2,
    FileAttributes,
    GetAttributesFromFilesParams,
    GetAttributesFromFilesResult,
    GetAttributesFromFilesResultV2,
    ReaddirParams,
    ReaddirResult,
    ScmFileStatus,
    Sha1OrError,
    SHA1Result,
    SizeOrError,
    SourceControlType,
    SourceControlTypeOrError,
    SyncBehavior,
    TimeSpec,
)

from .lib import testcase

EdenThriftResult = TypeVar(
    "EdenThriftResult",
    Union[FileAttributeDataOrError, FileAttributeDataOrErrorV2],
    SHA1Result,
)

# Change this if more attributes are added
ALL_ATTRIBUTES = (
    FileAttributes.FILE_SIZE
    | FileAttributes.SHA1_HASH
    | FileAttributes.SOURCE_CONTROL_TYPE
)


@testcase.eden_repo_test
# pyre-fixme[13]: Attribute `commit1` is never initialized.
# pyre-fixme[13]: Attribute `commit2` is never initialized.
# pyre-fixme[13]: Attribute `commit3` is never initialized.
class ThriftTest(testcase.EdenRepoTest):
    commit1: str
    commit2: str
    commit3: str

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("test_fetch1", "testing fetch\n")
        self.repo.write_file("test_fetch2", "testing fetch\n")
        self.repo.write_file("README", "docs\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/file", "bar!\n")
        self.repo.symlink("slink", "hello")
        self.commit1 = self.repo.commit("Initial commit.")

        self.repo.write_file("bdir/file", "bar?\n")
        self.repo.write_file("cdir/subdir/new.txt", "and improved")
        self.repo.remove_file("README")
        self.commit2 = self.repo.commit("Commit 2.")

        # revert the change made to bdir/file
        self.repo.write_file("bdir/file", "bar!\n")
        self.commit3 = self.repo.commit("Commit 3.")

    def get_loaded_inodes_count(self, path: str) -> int:
        with self.get_thrift_client_legacy() as client:
            result = client.debugInodeStatus(
                self.mount_path_bytes, os.fsencode(path), flags=0, sync=SyncBehavior()
            )
        inode_count = 0
        for item in result:
            assert item.entries is not None
            for inode in item.entries:
                if inode.loaded:
                    inode_count += 1
        return inode_count

    def test_pid_fetch_counts(self) -> None:
        # We already test that our fetch counts get incremented correctly in
        # unit tests. This is an end to end test to make sure that our fetch
        # counts are reasonable values. We know touching a file means we must
        # read it at least once. So we expect at least 2 fetches here. In
        # reality there may be more than that because touch will cause multiple
        # requests into fuse for each file.
        touch_p = subprocess.Popen(
            "touch test_fetch1 test_fetch2".split(), cwd=self.mount_path
        )
        touch_p.communicate()

        with self.get_thrift_client_legacy() as client:
            counts = client.getAccessCounts(1)
            accesses = counts.accessesByMount[self.mount_path_bytes]
            self.assertLessEqual(2, accesses.fetchCountsByPid[touch_p.pid])

    def test_list_mounts(self) -> None:
        with self.get_thrift_client_legacy() as client:
            mounts = client.listMounts()
        self.assertEqual(1, len(mounts))

        mount = mounts[0]
        self.assertEqual(self.mount_path_bytes, mount.mountPoint)
        assert mount.edenClientPath is not None
        # The client path should always be inside the main eden directory
        # Path.relative_to() will throw a ValueError if self.eden.eden_dir is not a
        # directory prefix of mount.edenClientPath
        Path(os.fsdecode(mount.edenClientPath)).relative_to(self.eden.eden_dir)

    def test_get_sha1(self) -> None:
        expected_sha1_for_hello = hashlib.sha1(b"hola\n").digest()
        result_for_hello = SHA1Result(expected_sha1_for_hello)

        expected_sha1_for_adir_file = hashlib.sha1(b"foo!\n").digest()
        result_for_adir_file = SHA1Result(expected_sha1_for_adir_file)

        with self.get_thrift_client_legacy() as client:
            self.assertEqual(
                [result_for_hello, result_for_adir_file],
                client.getSHA1(
                    self.mount_path_bytes,
                    [b"hello", b"adir/file"],
                    sync=SyncBehavior(),
                ),
            )

    def test_get_sha1_throws_for_path_with_dot_components(self) -> None:
        with self.get_thrift_client_legacy() as client:
            results = client.getSHA1(
                self.mount_path_bytes, [b"./hello"], sync=SyncBehavior()
            )
        self.assertEqual(1, len(results))
        self.assert_sha1_error(
            results[0],
            re.compile(
                r".*PathComponentValidationError.*: PathComponent must not be \."
            ),
        )

    def test_get_sha1_throws_for_empty_string(self) -> None:
        with self.get_thrift_client_legacy() as client:
            results = client.getSHA1(self.mount_path_bytes, [b""], sync=SyncBehavior())
        self.assertEqual(1, len(results))
        self.assert_sha1_error(results[0], "path cannot be the empty string")

    def test_get_sha1_throws_for_directory(self) -> None:
        with self.get_thrift_client_legacy() as client:
            results = client.getSHA1(
                self.mount_path_bytes, [b"adir"], sync=SyncBehavior(60)
            )
        self.assertEqual(1, len(results))
        self.assert_sha1_error(results[0], "adir: Is a directory")

    def test_get_sha1_throws_for_non_existent_file(self) -> None:
        with self.get_thrift_client_legacy() as client:
            results = client.getSHA1(
                self.mount_path_bytes, [b"i_do_not_exist"], sync=SyncBehavior()
            )
        self.assertEqual(1, len(results))
        self.assert_sha1_error(results[0], "i_do_not_exist: No such file or directory")

    def test_get_sha1_throws_for_symlink(self) -> None:
        """Fails because caller should resolve the symlink themselves."""
        with self.get_thrift_client_legacy() as client:
            results = client.getSHA1(
                self.mount_path_bytes, [b"slink"], sync=SyncBehavior()
            )
        self.assertEqual(1, len(results))
        self.assert_sha1_error(results[0], "slink: file is a symlink: Invalid argument")

    def assert_eden_error(
        self, result: EdenThriftResult, error_message: Union[str, Pattern]
    ) -> None:
        error = result.get_error()
        self.assertIsNotNone(error)
        if isinstance(error_message, str):
            self.assertEqual(error_message, error.message)
        else:
            self.assertRegex(error.message, error_message)

    def assert_sha1_error(
        self, sha1result: SHA1Result, error_message: Union[str, Pattern]
    ) -> None:
        self.assertIsNotNone(sha1result, msg="Must pass a SHA1Result")
        self.assertEqual(
            SHA1Result.ERROR, sha1result.getType(), msg="SHA1Result must be an error"
        )
        self.assert_eden_error(sha1result, error_message)

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
        with self.get_thrift_client_legacy() as client:
            unload_count = client.unloadInodeForPath(self.mount_path_bytes, b"", age)

        self.assertGreaterEqual(
            unload_count, 100, "Number of loaded inodes should reduce after unload"
        )

    def test_unload_thrift_api_accepts_single_dot_as_root(self) -> None:
        self.write_file("testfile.txt", "unload test case")

        age = TimeSpec()
        age.seconds = 0
        age.nanoSeconds = 0
        with self.get_thrift_client_legacy() as client:
            unload_count = client.unloadInodeForPath(self.mount_path_bytes, b".", age)

        self.assertGreater(
            unload_count, 0, "Number of loaded inodes should reduce after unload"
        )

    def get_counter(self, name: str) -> float:
        return self.get_counters()[name]

    def test_diff_revisions(self) -> None:
        # Convert the commit hashes to binary for the thrift call
        with self.get_thrift_client_legacy() as client:
            diff = client.getScmStatusBetweenRevisions(
                os.fsencode(self.mount),
                binascii.unhexlify(self.commit1),
                binascii.unhexlify(self.commit2),
            )

        self.assertDictEqual(diff.errors, {})
        self.assertDictEqual(
            diff.entries,
            {
                b"cdir/subdir/new.txt": ScmFileStatus.ADDED,
                b"bdir/file": ScmFileStatus.MODIFIED,
                b"README": ScmFileStatus.REMOVED,
            },
        )

    def test_diff_revisions_hex(self) -> None:
        # Watchman currently calls getScmStatusBetweenRevisions()
        # with 40-byte hexadecimal commit IDs, so make sure that works.
        with self.get_thrift_client_legacy() as client:
            diff = client.getScmStatusBetweenRevisions(
                os.fsencode(self.mount),
                self.commit1.encode("utf-8"),
                self.commit2.encode("utf-8"),
            )

        self.assertDictEqual(diff.errors, {})
        self.assertDictEqual(
            diff.entries,
            {
                b"cdir/subdir/new.txt": ScmFileStatus.ADDED,
                b"bdir/file": ScmFileStatus.MODIFIED,
                b"README": ScmFileStatus.REMOVED,
            },
        )

    def test_diff_revisions_with_reverted_file(self) -> None:
        # Convert the commit hashes to binary for the thrift call
        with self.get_thrift_client_legacy() as client:
            diff = client.getScmStatusBetweenRevisions(
                os.fsencode(self.mount),
                binascii.unhexlify(self.commit1),
                binascii.unhexlify(self.commit3),
            )

        self.assertDictEqual(diff.errors, {})
        # bdir/file was modified twice between commit1 and commit3 but had a
        # net change of 0 so it should not be reported in the diff results
        self.assertDictEqual(
            diff.entries,
            {
                b"cdir/subdir/new.txt": ScmFileStatus.ADDED,
                b"README": ScmFileStatus.REMOVED,
            },
        )
