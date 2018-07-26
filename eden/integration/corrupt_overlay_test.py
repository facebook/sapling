#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import contextlib
import os
import pathlib
import subprocess
import typing

import eden.integration.lib.overlay as overlay_mod
from eden.integration.lib import testcase


class CorruptOverlayTestBase(
    testcase.HgRepoTestMixin, testcase.EdenRepoTest, metaclass=abc.ABCMeta
):
    """Test file operations when Eden's overlay is corrupted.

    Tests in this class apply to all types of files (regular files, directories,
    etc.).
    """

    def test_unmount_succeeds(self) -> None:
        with self.assert_does_not_raise():
            self.eden.unmount(self.mount_path)

    @property
    @abc.abstractmethod
    def corrupted_path(self) -> pathlib.Path:
        raise NotImplementedError()

    @abc.abstractmethod
    def corrupt_overlay_file(self, path: pathlib.Path) -> None:
        raise NotImplementedError()

    @contextlib.contextmanager
    def assert_does_not_raise(self) -> typing.Iterator[None]:
        yield


class CorruptOverlayRegularFileTestBase(CorruptOverlayTestBase):
    """Test a regular file whose overlay was corrupted."""

    def setUp(self) -> None:
        super().setUp()
        self.overlay = overlay_mod.OverlayStore(self.eden, self.mount_path)
        self.overlay.corrupt_file(self.corrupted_path, self.corrupt_overlay_file)

    def populate_repo(self) -> None:
        self.repo.write_file("committed_file", "committed_file content")
        self.repo.commit("Initial commit.")

    def test_unlink_deletes_corrupted_file(self) -> None:
        path = self.mount_path / self.corrupted_path
        path.unlink()

        self.assertFalse(path.exists(), f"{path} should not exist after being deleted")

    def test_rm_program_with_force_deletes_corrupted_file(self) -> None:
        path = self.mount_path / self.corrupted_path
        subprocess.check_call(["rm", "-f", "--", path])

        self.assertFalse(path.exists(), f"{path} should not exist after being deleted")


class DeleteTrackedFile(CorruptOverlayRegularFileTestBase):
    @property
    def corrupted_path(self) -> pathlib.Path:
        return pathlib.Path("committed_file")

    def corrupt_overlay_file(self, path: pathlib.Path) -> None:
        path.unlink()


class DeleteUntrackedFile(CorruptOverlayRegularFileTestBase):
    @property
    def corrupted_path(self) -> pathlib.Path:
        return pathlib.Path("new_file")

    def corrupt_overlay_file(self, path: pathlib.Path) -> None:
        path.unlink()


class TruncateTrackedFile(CorruptOverlayRegularFileTestBase):
    @property
    def corrupted_path(self) -> pathlib.Path:
        return pathlib.Path("committed_file")

    def corrupt_overlay_file(self, path: pathlib.Path) -> None:
        os.truncate(str(path), 0)


del CorruptOverlayTestBase
del CorruptOverlayRegularFileTestBase
