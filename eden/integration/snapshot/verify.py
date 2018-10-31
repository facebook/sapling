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
import stat as stat_mod
from pathlib import Path
from typing import Dict, List

from eden.integration.lib import hgrepo


class ExpectedFileBase(metaclass=abc.ABCMeta):
    def __init__(self, path: str, perms: int, file_type: int) -> None:
        self.path = Path(path)
        self.permissions = perms
        self.file_type = file_type

    def verify(
        self, verifier: "SnapshotVerifier", path: Path, stat_info: os.stat_result
    ) -> None:
        found_perms = stat_mod.S_IMODE(stat_info.st_mode)
        if found_perms != self.permissions:
            verifier.error(
                f"{self.path}: expected permissions to be {self.permissions:#o}, "
                f"found {found_perms:#o}"
            )
        found_file_type = stat_mod.S_IFMT(stat_info.st_mode)
        if found_file_type != self.file_type:
            verifier.error(
                f"{self.path}: expected file type to be {self.file_type:#o}, "
                f"found {found_file_type:#o}"
            )
        else:
            self._verify_contents(verifier, path)

    @abc.abstractmethod
    def _verify_contents(self, verifier: "SnapshotVerifier", path: Path) -> None:
        pass

    def _error(self, msg: str) -> None:
        raise ValueError(msg)


class ExpectedFile(ExpectedFileBase):
    def __init__(self, path: str, contents: bytes, perms: int = 0o644) -> None:
        super().__init__(path, perms, stat_mod.S_IFREG)
        self.contents = contents

    def _verify_contents(self, verifier: "SnapshotVerifier", path: Path) -> None:
        with path.open("rb") as f:
            actual_contents = f.read()
        if actual_contents != self.contents:
            verifier.error(
                f"file contents mismatch for {self.path}:\n"
                f"expected: {self.contents!r}\n"
                f"actual:   {actual_contents!r}"
            )


class ExpectedSymlink(ExpectedFileBase):
    def __init__(self, path: str, contents: bytes, perms: int = 0o777) -> None:
        super().__init__(path, perms, stat_mod.S_IFLNK)
        self.contents = contents

    def _verify_contents(self, verifier: "SnapshotVerifier", path: Path) -> None:
        actual_contents = os.readlink(bytes(path))
        if actual_contents != self.contents:
            verifier.error(
                f"symlink contents mismatch for {self.path}:\n"
                f"expected: {self.contents!r}\n"
                f"actual:   {actual_contents!r}"
            )


class ExpectedSocket(ExpectedFileBase):
    def __init__(self, path: str, perms: int = 0o755) -> None:
        super().__init__(path, perms, stat_mod.S_IFSOCK)

    def _verify_contents(self, verifier: "SnapshotVerifier", path: Path) -> None:
        pass


class SnapshotVerifier:
    def __init__(self) -> None:
        self.errors: List[str] = []
        self.quiet = False

    def error(self, message: str) -> None:
        self.errors.append(message)
        if not self.quiet:
            print(f"==ERROR== {message}")

    def verify_directory(self, path: Path, expected: List[ExpectedFileBase]) -> None:
        """Confirm that the contents of a directory match the expected file state."""
        found_files = enumerate_directory(path)
        for expected_entry in expected:
            file_stat = found_files.pop(expected_entry.path, None)
            if file_stat is None:
                self.error(f"{expected_entry.path}: file not present in snapshot")
                continue

            full_path = path / expected_entry.path
            try:
                expected_entry.verify(self, full_path, file_stat)
            except AssertionError as ex:
                self.error(f"{expected_entry.path}: {ex}")
                continue

        for path, stat_info in found_files.items():
            if stat_mod.S_ISDIR(stat_info.st_mode):
                # Don't require directories to be listed explicitly in the input files
                continue
            if str(path.parents[0]) == ".hg":
                # Don't complain about files inside the .hg directory that the caller
                # did not explicitly specify.  Mercurial can create a variety of files
                # here, and we don't care about checking the exact list of files it
                # happened to create when the snapshot was generated.
                continue
            self.error(f"{path}: unexpected file present in snapshot")

    def verify_hg_status(
        self,
        repo: hgrepo.HgRepository,
        expected: Dict[str, str],
        check_ignored: bool = True,
    ) -> None:
        actual_status = repo.status(include_ignored=check_ignored)

        for path, expected_char in expected.items():
            actual_char = actual_status.pop(path, None)
            if expected_char != actual_char:
                self.error(
                    f"{path}: unexpected hg status difference: "
                    f"reported as {actual_char}, expected {expected_char}"
                )

        for path, actual_char in actual_status.items():
            self.error(
                f"{path}: unexpected hg status difference: "
                f"reported as {actual_char}, expected None"
            )


def enumerate_directory(path: Path) -> Dict[Path, os.stat_result]:
    """
    Recursively walk a directory and return a dictionary of all of the files and
    directories it contains.

    Returns a dictionary of [path -> os.stat_result]
    The returned paths are relative to the input directory.
    """
    entries: Dict[Path, os.stat_result] = {}
    _enumerate_directory_helper(path, Path(), entries)
    return entries


def _enumerate_directory_helper(
    root_path: Path, rel_path: Path, results: Dict[Path, os.stat_result]
) -> None:
    for entry in os.scandir(root_path / rel_path):
        # Current versions of typeshed don't know about the follow_symlinks argument,
        # so ignore type errors on the next line.
        stat_info = entry.stat(follow_symlinks=False)  # type: ignore
        entry_path = rel_path / entry.name
        results[entry_path] = stat_info
        if stat_mod.S_ISDIR(stat_info.st_mode):
            _enumerate_directory_helper(root_path, entry_path, results)
