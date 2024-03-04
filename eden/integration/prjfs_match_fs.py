#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from typing import List

from facebook.eden.ttypes import (
    EdenError,
    EdenErrorType,
    FaultDefinition,
    MatchFileSystemRequest,
    MountId,
    ScmFileStatus,
)

from .lib import prjfs_test, testcase


@testcase.eden_repo_test
class PrjfsMatchFsTest(prjfs_test.PrjFSTestBase):
    """Windows fsck integration tests"""

    initial_commit: str = ""
    enable_fault_injection: bool = True

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("subdir/bdir/file", "foo!\n")
        self.repo.write_file("subdir/cdir/file", "foo!\n")
        self.repo.write_file(".gitignore", "ignored/\n")
        self.initial_commit = self.repo.commit("Initial commit.")

    def select_storage_engine(self) -> str:
        return "sqlite"

    def get_initial_commit(self) -> str:
        return self.initial_commit

    def match_fs(self, files: List[bytes]) -> None:
        with self.eden.get_thrift_client_legacy() as client:
            errors = client.matchFilesystem(
                MatchFileSystemRequest(
                    MountId(self.mount.encode()),
                    files,
                )
            )
            for error in errors.results:
                self.assertEqual(error.error, None)

    def test_fix_no_problems(self) -> None:
        self.assertNotInStatus(b"adir/file")

        self.match_fs([b"adir/file"])

        self.assertNotInStatus(b"adir/file")

    def test_fix_missed_removal(self) -> None:
        self.assertNotInStatus(b"adir/file")

        with self.run_with_notifications_dropped_fault():
            afile = self.mount_path / "adir" / "file"
            afile.unlink()

        self.assertNotInStatus(b"adir/file")
        self.match_fs([b"adir/file"])

        self.assertEqual(
            self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED},
        )

    def test_fix_missed_addition(self) -> None:
        self.assertNotInStatus(b"adir/anewfile")

        with self.run_with_notifications_dropped_fault():
            afile = self.mount_path / "adir" / "anewfile"
            afile.touch()

        self.assertNotInStatus(b"adir/anewfile")
        self.match_fs([b"adir/anewfile"])

        self.assertEqual(
            self.eden_status(),
            {b"adir/anewfile": ScmFileStatus.ADDED},
        )

    def test_fix_missed_directory_delete(self) -> None:
        self.assertNotInStatus(b"adir/file")

        with self.run_with_notifications_dropped_fault():
            adir = self.mount_path / "adir"
            afile = adir / "file"
            afile.unlink()
            adir.rmdir()

        self.assertNotInStatus(b"adir/file")
        self.match_fs([b"adir"])

        self.assertEqual(
            self.eden_status(),
            {b"adir/file": ScmFileStatus.REMOVED},
        )

    def test_fix_missed_directory_addition(self) -> None:
        self.assertNotInStatus(b"adir/asubdir/anewfile")

        with self.run_with_notifications_dropped_fault():
            asubdir = self.mount_path / "adir" / "asubdir"
            afile = asubdir / "anewfile"
            asubdir.mkdir()
            afile.touch()

        self.assertNotInStatus(b"adir/asubdir/anewfile")
        self.match_fs([b"adir/asubdir"])

        self.assertEqual(
            self.eden_status(),
            {b"adir/asubdir/anewfile": ScmFileStatus.ADDED},
        )

    def test_fix_failed(self) -> None:
        self.assertNotInStatus(b"adir/file")

        with self.run_with_notifications_dropped_fault():
            afile = self.mount_path / "adir" / "file"
            afile.unlink()

        self.assertNotInStatus(b"adir/file")

        with self.eden.get_thrift_client_legacy() as client:
            client.injectFault(
                FaultDefinition(
                    keyClass="PrjfsDispatcherImpl::fileNotification",
                    keyValueRegex=".*",
                    errorMessage="Blocked",
                    errorType="runtime_error",
                )
            )
            errors = client.matchFilesystem(
                MatchFileSystemRequest(
                    MountId(self.mount.encode()),
                    [b"adir/file"],
                )
            )
            print(errors)
            for error in errors.results:
                self.assertEqual(
                    error.error,
                    EdenError(
                        message="class std::runtime_error: Blocked",
                        errorType=EdenErrorType.GENERIC_ERROR,
                    ),
                )
