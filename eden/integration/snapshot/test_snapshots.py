#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import stat
import unittest
from pathlib import Path
from typing import Callable

from eden.integration.lib import edenclient

from . import snapshot as snapshot_mod, verify as verify_mod


class Test(unittest.TestCase):
    """Tests to verify the contents of various saved snapshots.

    All of the test functions in this class are dynamically added by register_tests()
    """

    def _test_snapshot(self, snapshot_path: Path) -> None:
        with snapshot_mod.create_tmp_dir() as tmp_dir:
            snapshot = snapshot_mod.unpack_into(snapshot_path, tmp_dir)
            self._run_test(snapshot)

    def _run_test(self, snapshot: snapshot_mod.BaseSnapshot) -> None:
        verifier = verify_mod.SnapshotVerifier()
        snapshot.verify(verifier)

        # Fail the test if any errors were found.
        # The individual errors will have been printed out previously
        # as they were found.
        if verifier.errors:
            self.fail(f"found {len(verifier.errors)} errors")


class InfraTests(unittest.TestCase):
    """Tests for the snapshot generation/verification code itself."""

    NUM_SNAPSHOTS = 0

    def test_snapshot_list(self) -> None:
        # Ensure that at least one snapshot file was found, so that the tests will
        # fail if we somehow can't find the snapshot data directory correctly.
        self.assertGreater(self.NUM_SNAPSHOTS, 0)

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
        # pyre-fixme[13]: Attribute `backing_repo` is never initialized.
        # pyre-fixme[13]: Attribute `system_hgrc_path` is never initialized.
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
            f"a/b/wrong_file_type.txt: expected permissions to be 0o644, found 0o755",
            "a/b/wrong_perms.txt: expected permissions to be 0o644, found 0o755",
        ]
        self.assertEqual(sorted(verifier.errors), sorted(expected_errors))


def register_tests() -> None:
    # Create one test function for each snapshot
    snapshot_dir = Path("eden/test-data/snapshots").resolve()
    for snapshot in snapshot_dir.iterdir():
        # We don't use Path.stem here since it only strips off the very last suffix,
        # so foo.tar.bz2 becomes foo.tar rather than foo.
        stem = snapshot.name.split(".", 1)[0]
        setattr(Test, f"test_{stem}", _create_test_fn(snapshot))
        InfraTests.NUM_SNAPSHOTS += 1


def _create_test_fn(snapshot: Path) -> Callable[[Test], None]:
    def test_fn(self: Test) -> None:
        self._test_snapshot(snapshot)

    return test_fn


register_tests()
