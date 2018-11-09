#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import os
import typing
import unittest
from pathlib import Path
from typing import List, Optional, Sequence

from eden.cli import fsck as fsck_mod, overlay as overlay_mod
from eden.integration.lib.temporary_directory import TemporaryDirectoryMixin
from eden.integration.snapshot import snapshot as snapshot_mod


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

        err_path = os.path.join(error.inode.compute_path(), error.child.name)
        if err_path != self.path:
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


class Test(unittest.TestCase, TemporaryDirectoryMixin):
    """Tests for fsck that extract the basic-20181030 snapshot, corrupt it in various
    ways, and then run fsck to try and repair it.
    """

    def setUp(self) -> None:
        snapshot_path = Path("eden/test-data/snapshots/basic-20181030.tar.xz")

        self.tmp_dir = Path(self.make_temporary_directory())
        self.snapshot = snapshot_mod.unpack_into(snapshot_path, self.tmp_dir)

    def _overlay_path(self) -> Path:
        return self.snapshot.eden_state_dir / "clients" / "checkout" / "local"

    def _replace_overlay_inode(self, inode_number: int, data: Optional[bytes]) -> None:
        inode_path = (
            self._overlay_path() / f"{inode_number % 256:02x}" / str(inode_number)
        )
        inode_path.unlink()
        if data is not None:
            inode_path.write_bytes(data)

    def _run_fsck(
        self, expected_errors: Sequence[ExpectedError]
    ) -> fsck_mod.FilesystemChecker:
        overlay = overlay_mod.Overlay(str(self._overlay_path()))
        fsck = fsck_mod.FilesystemChecker(overlay)
        fsck.scan_for_errors()

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

        return fsck

    def test_untracked_file_removed(self) -> None:
        # inode 33 is untracked/new/normal2.txt
        self._replace_overlay_inode(33, None)

        expected_errors = [MissingMaterializedInode(33, "untracked/new/normal2.txt")]
        self._run_fsck(expected_errors)

    def test_untracked_file_empty(self) -> None:
        # inode 33 is untracked/new/normal2.txt
        self._replace_overlay_inode(33, b"")

        expected_errors = [InvalidMaterializedInode(33, "untracked/new/normal2.txt")]
        self._run_fsck(expected_errors)

    def test_untracked_file_short_header(self) -> None:
        # inode 33 is untracked/new/normal2.txt
        self._replace_overlay_inode(33, b"OVFL\x00\x00\x00\x01")

        expected_errors = [InvalidMaterializedInode(33, "untracked/new/normal2.txt")]
        self._run_fsck(expected_errors)

    # TODO: untracked dir removed
    # TODO: untracked dir truncated
    # TODO: untracked dir bad header
    # TODO: untracked dir bad body
    # TODO: and the same with modified file/dir
    # TODO: and the same with unmodified dir

    # TODO: replace untracked dir with file
    # TODO: replace untracked file with dir

    # TODO: fuzz tests?
