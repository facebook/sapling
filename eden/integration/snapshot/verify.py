#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import abc
import os
import stat as stat_mod
import typing
from pathlib import Path
from typing import Dict, Iterator, List, Mapping, Optional, TypeVar, Union

from eden.integration.lib import hgrepo


_AnyPath = Union[Path, str]


class _DefaultObject:
    pass


_DEFAULT_OBJECT: _DefaultObject = _DefaultObject()


class ExpectedFileBase(metaclass=abc.ABCMeta):
    def __init__(
        self, path: _AnyPath, contents: bytes, perms: int, file_type: int
    ) -> None:
        self.path: Path = Path(path)
        self.contents: bytes = contents
        self.permissions: int = perms
        self.file_type: int = file_type

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
            self.verify_contents(verifier, path)

    @abc.abstractmethod
    def verify_contents(self, verifier: "SnapshotVerifier", path: Path) -> None:
        pass

    def _error(self, msg: str) -> None:
        raise ValueError(msg)


class ExpectedFile(ExpectedFileBase):
    def __init__(self, path: _AnyPath, contents: bytes, perms: int = 0o644) -> None:
        super().__init__(path, contents, perms, stat_mod.S_IFREG)

    def verify_contents(self, verifier: "SnapshotVerifier", path: Path) -> None:
        with path.open("rb") as f:
            actual_contents = f.read()
        if actual_contents != self.contents:
            verifier.error(
                f"file contents mismatch for {self.path}:\n"
                f"expected: {self.contents!r}\n"
                f"actual:   {actual_contents!r}"
            )


class ExpectedSymlink(ExpectedFileBase):
    def __init__(self, path: _AnyPath, contents: bytes, perms: int = 0o777) -> None:
        super().__init__(path, contents, perms, stat_mod.S_IFLNK)

    def verify_contents(self, verifier: "SnapshotVerifier", path: Path) -> None:
        actual_contents = os.readlink(bytes(path))
        if actual_contents != self.contents:
            verifier.error(
                f"symlink contents mismatch for {self.path}:\n"
                f"expected: {self.contents!r}\n"
                f"actual:   {actual_contents!r}"
            )


class ExpectedSocket(ExpectedFileBase):
    def __init__(self, path: _AnyPath, perms: int = 0o755) -> None:
        super().__init__(path, b"", perms, stat_mod.S_IFSOCK)

    def verify_contents(self, verifier: "SnapshotVerifier", path: Path) -> None:
        pass


_ExpectedFile = TypeVar("_ExpectedFile", bound=ExpectedFileBase)


class ExpectedFileSet(Mapping[Path, ExpectedFileBase]):
    """
    ExpectedFileSet is basically a container of ExpectedFileBase objects,
    but also provides some helper methods for accessing and updating entries by path.
    """

    def __init__(self) -> None:
        self._entries: Dict[Path, ExpectedFileBase] = {}

    def __len__(self) -> int:
        return len(self._entries)

    def __iter__(self) -> Iterator[Path]:
        return iter(self._entries.keys())

    def __getitem__(self, path: _AnyPath) -> ExpectedFileBase:
        key = Path(path)
        return self._entries[key]

    def __delitem__(self, path: _AnyPath) -> None:
        key = Path(path)
        del self._entries[key]

    def __contains__(self, path: object) -> bool:
        if isinstance(path, str):
            key = Path(path)
        elif isinstance(path, Path):
            key = path
        else:
            return False
        return key in self._entries

    @typing.overload
    def pop(self, path: _AnyPath) -> ExpectedFileBase:
        ...

    @typing.overload  # noqa: F811
    def pop(self, path: _AnyPath, default: ExpectedFileBase) -> ExpectedFileBase:
        ...

    @typing.overload  # noqa: F811
    def pop(self, path: _AnyPath, default: None) -> Optional[ExpectedFileBase]:
        ...

    def pop(  # noqa: F811
        self,
        path: _AnyPath,
        default: Union[ExpectedFileBase, None, _DefaultObject] = _DEFAULT_OBJECT,
    ) -> Optional[ExpectedFileBase]:
        key = Path(path)
        if default is _DEFAULT_OBJECT:
            return self._entries.pop(key)
        else:
            tmp = typing.cast(Optional[ExpectedFileBase], default)
            return self._entries.pop(key, tmp)

    def add_file(
        self, path: _AnyPath, contents: bytes, perms: int = 0o644
    ) -> ExpectedFile:
        return self.add(ExpectedFile(path=path, contents=contents, perms=perms))

    def add_symlink(
        self, path: _AnyPath, contents: bytes, perms: int = 0o777
    ) -> ExpectedSymlink:
        return self.add(ExpectedSymlink(path=path, contents=contents, perms=perms))

    def add_socket(self, path: _AnyPath, perms: int = 0o755) -> ExpectedSocket:
        return self.add(ExpectedSocket(path=path, perms=perms))

    def add(self, entry: _ExpectedFile) -> _ExpectedFile:
        assert entry.path not in self
        self._entries[entry.path] = entry
        return entry

    def set_file(
        self, path: _AnyPath, contents: bytes, perms: int = 0o644
    ) -> ExpectedFile:
        return self.set(ExpectedFile(path=path, contents=contents, perms=perms))

    def set_symlink(
        self, path: _AnyPath, contents: bytes, perms: int = 0o777
    ) -> ExpectedSymlink:
        return self.set(ExpectedSymlink(path=path, contents=contents, perms=perms))

    def set_socket(self, path: _AnyPath, perms: int = 0o755) -> ExpectedSocket:
        return self.set(ExpectedSocket(path=path, perms=perms))

    def set(self, entry: _ExpectedFile) -> _ExpectedFile:
        self._entries[entry.path] = entry
        return entry


class SnapshotVerifier:
    def __init__(self) -> None:
        self.errors: List[str] = []
        self.quiet: bool = False

    def error(self, message: str) -> None:
        self.errors.append(message)
        if not self.quiet:
            print(f"==ERROR== {message}")

    def verify_directory(self, path: Path, expected: ExpectedFileSet) -> None:
        """Confirm that the contents of a directory match the expected file state."""
        found_files = enumerate_directory(path)
        for expected_entry in expected.values():
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
        stat_info: os.stat_result = entry.stat(follow_symlinks=False)  # type: ignore
        entry_path: Path = rel_path / entry.name
        results[entry_path] = stat_info
        if stat_mod.S_ISDIR(stat_info.st_mode):
            _enumerate_directory_helper(root_path, entry_path, results)
