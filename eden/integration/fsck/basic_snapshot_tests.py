#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import binascii
import itertools
import os
import typing
import unittest
from pathlib import Path
from typing import List, Optional, Sequence, Type, Union

from eden.cli import fsck as fsck_mod
from eden.integration.lib.temporary_directory import TemporaryDirectoryMixin
from eden.integration.snapshot import snapshot as snapshot_mod, verify as verify_mod
from eden.integration.snapshot.types.basic import BasicSnapshot


class ExpectedError(metaclass=abc.ABCMeta):
    @abc.abstractmethod
    def is_match(self, error: fsck_mod.Error) -> bool:
        pass


class MissingMaterializedInode(ExpectedError):
    def __init__(self, inode_number: int, path: str) -> None:
        super().__init__()
        self.inode_number = inode_number
        self.path = path

    def __str__(self) -> str:
        return f"MissingMaterializedInode({self.inode_number}, {self.path!r})"

    def is_match(self, error: fsck_mod.Error) -> bool:
        if not isinstance(error, fsck_mod.MissingMaterializedInode):
            return False

        if error.child.inode_number != self.inode_number:
            return False

        if error.compute_path() != self.path:
            return False

        return True


class InvalidMaterializedInode(ExpectedError):
    def __init__(self, inode_number: int, path: str) -> None:
        super().__init__()
        self.inode_number = inode_number
        self.path = path

    def __str__(self) -> str:
        return f"InvalidMaterializedInode({self.inode_number}, {self.path!r})"

    def is_match(self, error: fsck_mod.Error) -> bool:
        if not isinstance(error, fsck_mod.InvalidMaterializedInode):
            return False

        if error.inode.inode_number != self.inode_number:
            return False

        err_path = error.inode.compute_path()
        if err_path != self.path:
            return False

        return True


class OrphanInodes(ExpectedError):
    def __init__(self, orphans: List[int]) -> None:
        super().__init__()
        self.orphans = set(orphans)

    def __str__(self) -> str:
        return f"OrphanInodes({self.orphans})"

    def is_match(self, error: fsck_mod.Error) -> bool:
        if not isinstance(error, fsck_mod.OrphanInodes):
            return False

        actual_orphans = {
            inode_info.inode_number
            for inode_info in itertools.chain(
                error.orphan_files, error.orphan_directories
            )
        }
        return actual_orphans == self.orphans


class Test(unittest.TestCase, TemporaryDirectoryMixin):
    """Tests for fsck that extract the basic-20181120 snapshot, corrupt it in various
    ways, and then run fsck to try and repair it.
    """

    def setUp(self) -> None:
        snapshot_path = Path("eden/test-data/snapshots/basic-20181120.tar.xz")

        self.tmp_dir = Path(self.make_temporary_directory())
        snapshot = snapshot_mod.unpack_into(snapshot_path, self.tmp_dir)
        self.snapshot = typing.cast(BasicSnapshot, snapshot)

    def _checkout_state_dir(self) -> Path:
        return self.snapshot.eden_state_dir / "clients" / "checkout"

    def _overlay_path(self) -> Path:
        return self._checkout_state_dir() / "local"

    def _replace_overlay_inode(self, inode_number: int, data: Optional[bytes]) -> None:
        inode_path = (
            self._overlay_path() / f"{inode_number % 256:02x}" / str(inode_number)
        )
        inode_path.unlink()
        if data is not None:
            inode_path.write_bytes(data)

    def _run_fsck(self, expected_errors: Sequence[ExpectedError]) -> None:
        with fsck_mod.FilesystemChecker(self._checkout_state_dir()) as fsck:
            fsck.scan_for_errors()
            self._check_expected_errors(fsck, expected_errors)
            fsck.fix_errors()

    def _verify_contents(self, expected_files: verify_mod.ExpectedFileSet) -> None:
        verifier = verify_mod.SnapshotVerifier()
        with self.snapshot.edenfs() as eden:
            eden.start()
            verifier.verify_directory(self.snapshot.checkout_path, expected_files)

    def _check_expected_errors(
        self, fsck: fsck_mod.FilesystemChecker, expected_errors: Sequence[ExpectedError]
    ) -> None:
        remaining_expected = list(expected_errors)
        unexpected_errors: List[fsck_mod.Error] = []
        for found_error in fsck.errors:
            for expected_idx, expected in enumerate(remaining_expected):
                if expected.is_match(found_error):
                    del remaining_expected[expected_idx]
                    break
            else:
                unexpected_errors.append(found_error)

        errors = []
        if unexpected_errors:
            err_list = "  \n".join(str(err) for err in unexpected_errors)
            errors.append(f"unexpected fsck errors reported:\n  {err_list}")
        if remaining_expected:
            err_list = "  \n".join(str(err) for err in remaining_expected)
            errors.append(f"did not find all expected fsck errors:\n  {err_list}")

        if errors:
            self.fail("\n".join(errors))

    def test_untracked_file_removed(self) -> None:
        self._test_file_corrupted(None, MissingMaterializedInode)

    def test_untracked_file_empty(self) -> None:
        self._test_file_corrupted(b"", InvalidMaterializedInode)

    def test_untracked_file_short_header(self) -> None:
        self._test_file_corrupted(b"OVFL\x00\x00\x00\x01", InvalidMaterializedInode)

    def _test_file_corrupted(
        self,
        data: Optional[bytes],
        error_type: Union[
            Type[MissingMaterializedInode], Type[InvalidMaterializedInode]
        ],
    ) -> None:
        inode_number = 35  # untracked/new/normal2.txt
        self._replace_overlay_inode(inode_number, data)

        error = error_type(inode_number, "untracked/new/normal2.txt")
        expected_errors = [error]
        repaired_files = self.snapshot.get_expected_files()
        repaired_files.set_file(error.path, b"", perms=0o644)

        self._run_fsck(expected_errors)
        self._run_fsck([])
        self._verify_contents(repaired_files)

    def test_untracked_dir_removed(self) -> None:
        self._test_untracked_dir_corrupted(None, MissingMaterializedInode)

    def test_untracked_dir_truncated(self) -> None:
        self._test_untracked_dir_corrupted(b"", InvalidMaterializedInode)

    def test_untracked_dir_short_header(self) -> None:
        self._test_untracked_dir_corrupted(
            b"OVDR\x00\x00\x00\x01", InvalidMaterializedInode
        )

    def test_untracked_dir_short_body(self) -> None:
        data = binascii.unhexlify(
            (
                # directory header
                "4f56 4452 0000 0001 0000 0000 5bd8 fcc8"
                "0000 0000 0031 6d28 0000 0000 5bd8 fcc8"
                "0000 0000 0178 73a4 0000 0000 5bd8 fcc8"
                "0000 0000 0178 73a4 0000 0000 0000 0000"
                # partial body
                "1b04 8c0e 6576 6572 7962 6f64 792e 736f"
                "636b 15c8 8606 1648 000e 6578 6563 7574"
            ).replace(" ", "")
        )
        self._test_untracked_dir_corrupted(data, InvalidMaterializedInode)

    def _test_untracked_dir_corrupted(
        self,
        data: Optional[bytes],
        error_type: Union[
            Type[MissingMaterializedInode], Type[InvalidMaterializedInode]
        ],
    ) -> None:
        inode_number = 32  # untracked/
        self._replace_overlay_inode(inode_number, data)

        main_error = error_type(inode_number, "untracked")
        repaired_files = self.snapshot.get_expected_files()
        del repaired_files["untracked/executable.exe"]
        del repaired_files["untracked/everybody.sock"]
        del repaired_files["untracked/owner_only.sock"]
        del repaired_files["untracked/new/normal.txt"]
        del repaired_files["untracked/new/normal2.txt"]
        del repaired_files["untracked/new/readonly.txt"]
        orphan_errors = OrphanInodes(
            [
                33,  # new
                40,  # executable.exe
                41,  # everybody.sock
                42,  # owner_only.sock
            ]
        )
        expected_errors = [main_error, orphan_errors]
        self._run_fsck(expected_errors)
        self._run_fsck([orphan_errors])  # fsck does not currently fix orphan inodes
        self._verify_contents(repaired_files)

    def test_main_dir_truncated(self) -> None:
        # inode 4 is main/
        self._replace_overlay_inode(4, b"")

        repaired_files = self.snapshot.get_expected_files()
        del repaired_files["main/ignored.txt"]
        del repaired_files["main/loaded_dir/loaded_file.c"]
        del repaired_files["main/loaded_dir/not_loaded_exe.sh"]
        del repaired_files["main/loaded_dir/not_loaded_file.c"]
        del repaired_files["main/materialized_subdir/script.sh"]
        del repaired_files["main/materialized_subdir/test.c"]
        del repaired_files["main/materialized_subdir/unmodified.txt"]
        del repaired_files["main/mode_changes/exe_to_normal.txt"]
        del repaired_files["main/mode_changes/normal_to_exe.txt"]
        del repaired_files["main/mode_changes/normal_to_readonly.txt"]
        del repaired_files["main/untracked.txt"]
        del repaired_files["main/untracked_dir/foo.txt"]
        orphan_errors = OrphanInodes(
            [
                19,  # loaded_dir
                20,  # materialized_subdir
                21,  # mode_changes
                43,  # untracked.txt
                44,  # ignored.txt
                45,  # untracked_dir
            ]
        )
        expected_errors = [InvalidMaterializedInode(4, "main"), orphan_errors]
        self._run_fsck(expected_errors)
        self._run_fsck([orphan_errors])  # fsck does not currently fix orphan inodes
        self._verify_contents(repaired_files)

    # TODO: replace untracked dir with file
    # TODO: replace untracked file with dir
