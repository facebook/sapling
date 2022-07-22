#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import stat
import unittest
from pathlib import Path
from typing import Iterable, Tuple, Type

from eden.integration.lib import edenclient, testcase

from . import snapshot as snapshot_mod, verify as verify_mod


def _replicate_snapshot_test(
    test_class: Type[unittest.TestCase],
) -> Iterable[Tuple[str, Type[unittest.TestCase]]]:
    variants = []

    snapshot_dir = snapshot_mod.get_snapshots_root()
    for snapshot_path in snapshot_dir.iterdir():

        class EdenSnapshot(test_class):
            def _getSnapshotPath(self) -> Path:
                return snapshot_path

        # We don't use Path.stem here since it only strips off the very last suffix,
        # so foo.tar.bz2 becomes foo.tar rather than foo.
        stem = snapshot_path.name.split(".", 1)[0]
        variants += [(stem, EdenSnapshot)]

    return variants


snapshot_test = testcase.test_replicator(_replicate_snapshot_test)


@snapshot_test
@unittest.skipIf(not edenclient.can_run_eden(), "unable to run edenfs")
class Test(unittest.TestCase):
    """Tests to verify the contents of various saved snapshots.

    All of the test functions in this class are dynamically added by register_tests()
    """

    def _getSnapshotPath(self) -> Path:
        # This is usually implemented by the @snapshot_tests decorator
        raise NotImplementedError("Subclass must implement getSnapshotPath()")

    def test_snapshot(self) -> None:
        with snapshot_mod.create_tmp_dir() as tmp_dir:
            snapshot = snapshot_mod.unpack_into(self._getSnapshotPath(), tmp_dir)
            self._run_test(snapshot)

    def _run_test(self, snapshot: snapshot_mod.BaseSnapshot) -> None:
        verifier = verify_mod.SnapshotVerifier()
        snapshot.verify(verifier)

        # Fail the test if any errors were found.
        # The individual errors will have been printed out previously
        # as they were found.
        if verifier.errors:
            self.fail(f"found {len(verifier.errors)} errors")


@testcase.eden_test
class InfraTests(unittest.TestCase):
    """Tests for the snapshot generation/verification code itself."""

    def test_verify_directory(self) -> None:
        expected = verify_mod.ExpectedFileSet()
        expected.add_file("a/b/normal.txt", b"abc\n", 0o644)
        expected.add_file("a/b/normal_exe.exe", b"abc\n", 0o755)
        expected.add_file("a/b/missing.txt", b"abc\n", 0o644)
        expected.add_file("a/b/wrong_perms.txt", b"abc\n", 0o644)
        expected.add_file("a/b/wrong_file_type.txt", b"abc\n", 0o644)
        expected.add_socket("a/normal.sock", 0o644)
        expected.add_socket("a/exe.sock", 0o755)
        expected.add_symlink("a/normal.link", b"symlink contents", 0o777)
        expected.add_symlink("a/missing.link", b"missing symlink", 0o777)

        # Define a subclass of HgSnapshot.  We use define this solely so we can use its
        # helper write_file(), make_socket(), and mkdir() methods
        class MockSnapshot(snapshot_mod.HgSnapshot):
            def populate_backing_repo(self) -> None:
                pass

            def populate_checkout(self) -> None:
                pass

            def verify_snapshot_data(
                self, verifier: verify_mod.SnapshotVerifier, eden: edenclient.EdenFS
            ) -> None:
                pass

        with snapshot_mod.create_tmp_dir() as tmp_dir:
            snapshot = MockSnapshot(tmp_dir)
            snapshot.data_dir.mkdir()
            snapshot.checkout_path.mkdir()
            snapshot.write_file("a/b/normal.txt", b"abc\n", 0o644)
            snapshot.write_file("a/b/normal_exe.exe", b"abc\n", 0o755)
            snapshot.write_file("a/b/wrong_perms.txt", b"abc\n", 0o755)
            snapshot.make_socket("a/b/wrong_file_type.txt", 0o755)
            snapshot.make_socket("a/normal.sock", 0o644)
            snapshot.make_socket("a/exe.sock", 0o755)
            os.symlink(b"symlink contents", snapshot.checkout_path / "a/normal.link")
            # The verifier code only checks files, not directories, so it should not
            # complain about extra directories that may be present.
            snapshot.mkdir("a/b/c/extra_dir", 0o755)

            verifier = verify_mod.SnapshotVerifier()
            verifier.verify_directory(snapshot.checkout_path, expected)

        expected_errors = [
            "a/b/missing.txt: file not present in snapshot",
            "a/missing.link: file not present in snapshot",
            f"a/b/wrong_file_type.txt: expected file type to be {stat.S_IFREG:#o}, "
            f"found {stat.S_IFSOCK:#o}",
            "a/b/wrong_file_type.txt: expected permissions to be 0o644, found 0o755",
            "a/b/wrong_perms.txt: expected permissions to be 0o644, found 0o755",
        ]
        self.assertEqual(sorted(verifier.errors), sorted(expected_errors))
