#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import binascii
import re
import struct
import subprocess
import typing
import unittest
from pathlib import Path
from typing import List, Optional

from eden.fs.cli import cmd_util

from eden.integration.lib import edenclient, testcase
from eden.integration.snapshot import snapshot as snapshot_mod, verify as verify_mod
from eden.integration.snapshot.types.basic import BasicSnapshot
from eden.test_support.temporary_directory import TemporaryDirectoryMixin


class FsckError(metaclass=abc.ABCMeta):
    @abc.abstractmethod
    def is_match(self, other) -> bool:
        pass

    @staticmethod
    def _regex_match(regex: str, line: str) -> bool:
        return re.search(regex, line) is not None


class MissingMaterializedInode(FsckError):
    regex = (
        r"error: missing overlay file for materialized (directory|file) inode [0-9]+"
    )

    def __init__(self, inode_number: int) -> None:
        super().__init__()
        self.inode_number = inode_number

    def is_match(self, other) -> bool:
        if self.__class__ != other.__class__:
            return False

        return (self.inode_number) == (other.inode_number)

    def __str__(self) -> str:
        return f"MissingMaterializedInode({self.inode_number})"

    @staticmethod
    def create(line: str) -> "MissingMaterializedInode":
        match = re.search(MissingMaterializedInode.regex, line)
        assert match
        snippet = match.group(0)
        inode_number = re.findall(r"\d+", snippet)[0]

        return MissingMaterializedInode(inode_number)

    @staticmethod
    def is_instance(line: str) -> bool:
        return FsckError._regex_match(MissingMaterializedInode.regex, line)


class InvalidMaterializedInode(FsckError):

    regex = r"error: error reading data for inode [0-9]+"

    def __init__(
        self, inode_number: int, path: str, parent_inode: int, bad_data: bytes
    ) -> None:
        super().__init__()
        self.inode_number = inode_number
        self.path = path
        self.parent_inode_number = parent_inode
        self.bad_data = bad_data

    def is_match(self, other) -> bool:
        if self.__class__ != other.__class__:
            return False

        return (self.inode_number) == (other.inode_number)

    def __str__(self) -> str:
        return f"InvalidMaterializedInode({self.inode_number})"

    @staticmethod
    def create(line: str) -> "InvalidMaterializedInode":
        snippet = re.findall(InvalidMaterializedInode.regex, line)[0]
        inode_number = re.findall(r"\d+", snippet)[0]
        path = ""
        parent_inode_number = 0
        bad_data = b""

        return InvalidMaterializedInode(
            inode_number, path, parent_inode_number, bad_data
        )

    @staticmethod
    def is_instance(line: str) -> bool:
        return FsckError._regex_match(InvalidMaterializedInode.regex, line)


class OrphanFile(FsckError):

    regex = r"error: found orphan file inode [0-9]+"

    def __init__(
        self, inode_number: int, file_info: verify_mod.ExpectedFileBase
    ) -> None:
        self.inode_number = inode_number
        self.file_info = file_info

    def is_match(self, other) -> bool:
        if self.__class__ != other.__class__:
            return False

        return (self.inode_number) == (other.inode_number)

    def __str__(self) -> str:
        return f"OrphanFile({self.inode_number})"

    @staticmethod
    def create(line: str) -> "OrphanFile":
        snippet = re.findall(OrphanFile.regex, line)[0]
        inode_number = re.findall(r"\d+", snippet)[0]

        return OrphanFile(inode_number, verify_mod.ExpectedFile("", b""))

    @staticmethod
    def is_instance(line: str) -> bool:
        return FsckError._regex_match(OrphanFile.regex, line)


class OrphanDir(FsckError):

    regex = r"error: found orphan directory inode [0-9]+"

    def __init__(
        self, inode_number: int, path: str, contents: List[verify_mod.ExpectedFileBase]
    ) -> None:
        self.inode_number = inode_number
        self.path: Path = Path(path)
        self.contents = contents

    def is_match(self, other) -> bool:
        if self.__class__ != other.__class__:
            return False

        return (self.inode_number) == (other.inode_number)

    def __str__(self) -> str:
        return f"OrphanDir({self.inode_number})"

    @staticmethod
    def create(line: str) -> "OrphanDir":
        snippet = re.findall(OrphanDir.regex, line)[0]
        inode_number = re.findall(r"\d+", snippet)[0]

        return OrphanDir(inode_number, "", [])

    @staticmethod
    def is_instance(line: str) -> bool:
        return FsckError._regex_match(OrphanDir.regex, line)


class MissingNextInodeNumber(FsckError):

    regex = r"Overlay was shut down uncleanly"

    def __init__(self) -> None:
        super().__init__()

    def is_match(self, other) -> bool:
        return self.__class__ == other.__class__

    def __str__(self) -> str:
        return "MissingNextInodeNumber()"

    @staticmethod
    def create(line: str) -> "MissingNextInodeNumber":
        return MissingNextInodeNumber()

    @staticmethod
    def is_instance(line: str) -> bool:
        return FsckError._regex_match(MissingNextInodeNumber.regex, line)


class BadNextInodeNumber(FsckError):

    regex = r"error: bad stored next inode number: read [0-9]+ but should be at least [0-9]+"

    def __init__(
        self, read_next_inode_number: int, correct_next_inode_number: int
    ) -> None:
        super().__init__()
        self.read_next_inode_number = read_next_inode_number
        self.correct_next_inode_number = correct_next_inode_number

    def is_match(self, other) -> bool:
        if self.__class__ != other.__class__:
            return False

        return (self.read_next_inode_number, self.correct_next_inode_number) == (
            other.read_next_inode_number,
            other.correct_next_inode_number,
        )

    def __str__(self) -> str:
        return (
            "BadNextInodeNumber("
            f"{self.read_next_inode_number}, "
            f"{self.correct_next_inode_number}"
            ")"
        )

    @staticmethod
    def create(line: str) -> "BadNextInodeNumber":
        snippet = re.findall(BadNextInodeNumber.regex, line)[0]
        next_inode_numbers = re.findall(r"\d+", snippet)

        return BadNextInodeNumber(next_inode_numbers[0], next_inode_numbers[1])

    @staticmethod
    def is_instance(line: str) -> bool:
        return FsckError._regex_match(BadNextInodeNumber.regex, line)


class CorruptNextInodeNumber(FsckError):

    regex = r"Failed to read entire inode number\. Only read [0-9]+ bytes\. Full overlay scan required\."

    def __init__(self) -> None:
        super().__init__()

    def is_match(self, other) -> bool:
        return self.__class__ == other.__class__

    def __str__(self) -> str:
        return "CorruptNextInodeNumber()"

    @staticmethod
    def create(line: str) -> "CorruptNextInodeNumber":
        return CorruptNextInodeNumber()

    @staticmethod
    def is_instance(line: str) -> bool:
        return FsckError._regex_match(CorruptNextInodeNumber.regex, line)


def createFsckError(line: str) -> Optional[FsckError]:

    if MissingMaterializedInode.is_instance(line):
        return MissingMaterializedInode.create(line)

    if InvalidMaterializedInode.is_instance(line):
        return InvalidMaterializedInode.create(line)

    if OrphanFile.is_instance(line):
        return OrphanFile.create(line)

    if OrphanDir.is_instance(line):
        return OrphanDir.create(line)

    if MissingNextInodeNumber.is_instance(line):
        return MissingNextInodeNumber.create(line)

    if BadNextInodeNumber.is_instance(line):
        return BadNextInodeNumber.create(line)

    if CorruptNextInodeNumber.is_instance(line):
        return CorruptNextInodeNumber.create(line)

    return None


@unittest.skipIf(not edenclient.can_run_eden(), "unable to run edenfs")
class SnapshotTestBase(
    unittest.TestCase, TemporaryDirectoryMixin, metaclass=abc.ABCMeta
):
    """Tests for fsck that extract the basic-20210712 snapshot, corrupt it in various
    ways, and then run fsck to try and repair it.
    """

    tmp_dir: Path
    snapshot: BasicSnapshot

    @abc.abstractmethod
    def get_snapshot_path(self) -> Path:
        raise NotImplementedError()

    def setUp(self) -> None:
        self.tmp_dir = Path(self.make_temporary_directory())
        snapshot = snapshot_mod.unpack_into(self.get_snapshot_path(), self.tmp_dir)
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

    def _run_fsck(self, expected_errors: List[FsckError]) -> None:
        output = subprocess.check_output(
            [
                cmd_util.get_fsck_command(),
                str(self._overlay_path()),
            ],
            stderr=subprocess.STDOUT,
        )

        actual_errors: List[FsckError] = []

        for line in output.splitlines():
            print(line.decode())
            err = createFsckError(line.decode())
            if err:
                actual_errors.append(err)

        print(len(actual_errors))

        self._check_expected_errors(actual_errors, expected_errors)

    def _verify_contents(self, expected_files: verify_mod.ExpectedFileSet) -> None:
        verifier = verify_mod.SnapshotVerifier()
        with self.snapshot.edenfs() as eden:
            eden.start()
            verifier.verify_directory(self.snapshot.checkout_path, expected_files)

        if verifier.errors:
            self.fail(
                f"found errors when checking checkout contents: {verifier.errors}"
            )

    def _check_expected_errors(
        self, actual_errors: List[FsckError], expected_errors: List[FsckError]
    ) -> None:

        actual_errors_str = sorted([str(x) for x in actual_errors])
        expected_errors_str = sorted([str(x) for x in expected_errors])

        if actual_errors_str == expected_errors_str:
            return

        error = ""
        if len(actual_errors_str) > len(expected_errors_str):
            err_list = "  \n".join(set(actual_errors_str) - set(expected_errors_str))
            error = f"unexpected fsck errors reported:\n  {err_list}"
        else:
            err_list = "  \n".join(set(expected_errors_str) - set(actual_errors_str))
            error = f"did not find all expected fsck errors:\n  {err_list}"

        self.fail(error)


@testcase.eden_test
class Basic20210712Test(SnapshotTestBase):
    def get_snapshot_path(self) -> Path:
        return snapshot_mod.get_snapshots_root() / "basic-20210712.tar.xz"

    def get_fsck_log_dirs(self) -> List[Path]:
        return list((self._overlay_path().parent / "fsck").iterdir())

    def _verify_fsck(
        self,
        expected_files: verify_mod.ExpectedFileSet,
        expected_errors: List[FsckError],
        auto_fsck: bool,
    ) -> None:
        if auto_fsck:
            # Remove the next-inode-number file so that edenfs will
            # automatically peform an fsck run when mounting this checkout.
            next_inode_path = self._overlay_path() / "next-inode-number"
            next_inode_path.unlink()

            # Now call _verify_contents() without ever running fsck.
            # edenfs should automatically perform the fsck steps.
            self._verify_contents(expected_files)
            log_dirs = self.get_fsck_log_dirs()
            if len(log_dirs) != 1:
                raise Exception(
                    f"unable to find fsck log directory: candidates are {log_dirs!r}"
                )
        else:
            # manual fsck
            self._run_fsck(expected_errors)
            self._verify_contents(expected_files)

    def test_untracked_file_removed(self) -> None:
        self._test_file_corrupted(None)

    def test_untracked_file_empty(self) -> None:
        self._test_file_corrupted(b"")

    def test_untracked_file_short_header(self) -> None:
        self._test_file_corrupted(b"OVFL\x00\x00\x00\x01")

    def _test_file_corrupted(self, data: Optional[bytes]) -> None:
        inode_number = 52  # untracked/new/normal2.txt
        path = "untracked/new/normal2.txt"
        self._replace_overlay_inode(inode_number, data)

        expected_errors: List[FsckError] = []
        if data is None:
            expected_errors.append(MissingMaterializedInode(inode_number))
        else:
            expected_errors.append(
                InvalidMaterializedInode(
                    inode_number, path, parent_inode=50, bad_data=data
                )
            )
        repaired_files = self.snapshot.get_expected_files()
        repaired_files.set_file(path, b"", perms=0o644)

        self._verify_fsck(
            expected_files=repaired_files,
            expected_errors=expected_errors,
            auto_fsck=False,
        )

    def test_untracked_dir_removed(self) -> None:
        self._test_untracked_dir_corrupted(None, auto_fsck=False)

    def test_untracked_dir_truncated(self) -> None:
        self._test_untracked_dir_corrupted(b"", auto_fsck=False)

    def test_untracked_dir_short_header(self) -> None:
        self._test_untracked_dir_corrupted(b"OVDR\x00\x00\x00\x01", auto_fsck=False)

    def test_untracked_dir_removed_auto_fsck(self) -> None:
        self._test_untracked_dir_corrupted(None, auto_fsck=True)

    def test_untracked_dir_truncated_auto_fsck(self) -> None:
        self._test_untracked_dir_corrupted(b"", auto_fsck=True)

    def test_untracked_dir_short_header_auto_fsck(self) -> None:
        self._test_untracked_dir_corrupted(b"OVDR\x00\x00\x00\x01", auto_fsck=True)

    _short_body_data = binascii.unhexlify(
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

    def test_untracked_dir_short_body(self) -> None:
        self._test_untracked_dir_corrupted(self._short_body_data, auto_fsck=False)

    def test_untracked_dir_short_body_auto_fsck(self) -> None:
        self._test_untracked_dir_corrupted(self._short_body_data, auto_fsck=True)

    def _test_untracked_dir_corrupted(
        self, data: Optional[bytes], auto_fsck: bool
    ) -> None:
        inode_number = 49  # untracked/
        self._replace_overlay_inode(inode_number, data)

        expected_errors: List[FsckError] = []
        if data is None:
            expected_errors.append(MissingMaterializedInode(inode_number))
        else:
            expected_errors.append(
                InvalidMaterializedInode(
                    inode_number, "untracked", parent_inode=1, bad_data=data
                )
            )
        repaired_files = self.snapshot.get_expected_files()
        orphan_files = [
            OrphanFile(57, repaired_files.pop("untracked/executable.exe")),
            OrphanFile(58, repaired_files.pop("untracked/everybody.sock")),
            OrphanFile(59, repaired_files.pop("untracked/owner_only.sock")),
        ]
        orphan_dirs = [
            OrphanDir(
                50,
                "untracked/new",
                [
                    repaired_files.pop("untracked/new/normal.txt"),
                    repaired_files.pop("untracked/new/normal2.txt"),
                    repaired_files.pop("untracked/new/readonly.txt"),
                    repaired_files.pop("untracked/new/subdir/abc.txt"),
                    repaired_files.pop("untracked/new/subdir/xyz.txt"),
                ],
            )
        ]
        expected_errors.extend(orphan_files + orphan_dirs)

        self._verify_fsck(
            expected_files=repaired_files,
            expected_errors=expected_errors,
            auto_fsck=auto_fsck,
        )

    def _test_truncate_main_dir(self, auto_fsck: bool) -> None:
        # inode 4 is main/
        bad_main_data = b""
        self._replace_overlay_inode(4, bad_main_data)
        expected_errors: List[FsckError] = [
            InvalidMaterializedInode(4, "main", parent_inode=1, bad_data=bad_main_data)
        ]

        repaired_files = self.snapshot.get_expected_files()
        orphan_files = [
            OrphanFile(60, repaired_files.pop("main/untracked.txt")),
            OrphanFile(61, repaired_files.pop("main/ignored.txt")),
        ]
        orphan_dirs = [
            OrphanDir(24, "main/loaded_dir", []),
            OrphanDir(
                25,
                "main/materialized_subdir",
                [
                    repaired_files.pop("main/materialized_subdir/script.sh"),
                    repaired_files.pop("main/materialized_subdir/test.c"),
                    repaired_files.pop("main/materialized_subdir/modified_symlink.lnk"),
                    repaired_files.pop("main/materialized_subdir/new_symlink.lnk"),
                    repaired_files.pop("main/materialized_subdir/test/socket.sock"),
                ],
            ),
            OrphanDir(
                26,
                "main/mode_changes",
                [
                    repaired_files.pop("main/mode_changes/exe_to_normal.txt"),
                    repaired_files.pop("main/mode_changes/normal_to_exe.txt"),
                    repaired_files.pop("main/mode_changes/normal_to_readonly.txt"),
                ],
            ),
            OrphanDir(
                62,
                "main/untracked_dir",
                [repaired_files.pop("main/untracked_dir/foo.txt")],
            ),
        ]
        expected_errors.extend(orphan_files + orphan_dirs)

        # The following files are inside the corrupt directory, but they were never
        # materialized and so their contents will not be extracted into lost+found.
        del repaired_files["main/loaded_dir/loaded_file.c"]
        del repaired_files["main/loaded_dir/not_loaded_exe.sh"]
        del repaired_files["main/loaded_dir/not_loaded_file.c"]
        del repaired_files["main/loaded_dir/not_loaded_subdir/a.txt"]
        del repaired_files["main/loaded_dir/not_loaded_subdir/b.exe"]
        del repaired_files["main/loaded_dir/loaded_subdir/dir1/file1.txt"]
        del repaired_files["main/loaded_dir/loaded_subdir/dir2/file2.txt"]
        del repaired_files["main/materialized_subdir/unmodified.txt"]

        self._verify_fsck(
            expected_files=repaired_files,
            expected_errors=expected_errors,
            auto_fsck=auto_fsck,
        )

    def test_main_dir_truncated(self) -> None:
        self._test_truncate_main_dir(auto_fsck=False)

    def test_main_dir_truncated_auto_fsck(self) -> None:
        self._test_truncate_main_dir(auto_fsck=True)

    # The correct next inode number for this snapshot.
    _next_inode_number = 65

    def _compute_next_inode_data(self, inode_number: int) -> bytes:
        return struct.pack("<Q", inode_number)

    def test_missing_next_inode_number(self) -> None:
        self._test_bad_next_inode_number(None, [MissingNextInodeNumber()])

        # Start eden and verify the checkout looks correct.
        # This is relatively slow, compared to running fsck itself, so we only do
        # this on one of the next-inode-number test variants.
        expected_files = self.snapshot.get_expected_files()
        self._verify_contents(expected_files)

    def test_incorrect_next_inode_number(self) -> None:
        # Test replacing the next inode number file with a value too small by 0
        self._test_bad_next_inode_number(
            self._compute_next_inode_data(self._next_inode_number - 1),
            [BadNextInodeNumber(self._next_inode_number - 1, self._next_inode_number)],
        )

        # Test replacing the next inode number file with a much smaller value
        self._test_bad_next_inode_number(
            self._compute_next_inode_data(10),
            [BadNextInodeNumber(10, self._next_inode_number)],
        )

        # Replacing the next inode number file with a larger value should not
        # be reported as an error.
        next_inode_path = self._overlay_path() / "next-inode-number"
        next_inode_path.write_bytes(self._compute_next_inode_data(12345678))
        self._run_fsck([])

    def test_corrupt_next_inode_number(self) -> None:
        self._test_bad_next_inode_number(
            b"abc", [CorruptNextInodeNumber(), MissingNextInodeNumber()]
        )

    def _test_bad_next_inode_number(
        self, next_inode_data: Optional[bytes], expected_errors: List[FsckError]
    ) -> None:
        next_inode_path = self._overlay_path() / "next-inode-number"

        if next_inode_data is None:
            next_inode_path.unlink()
        else:
            next_inode_path.write_bytes(next_inode_data)

        self._run_fsck(expected_errors)

        # Verify the contents of the next-inode-number file now
        new_data = next_inode_path.read_bytes()
        expected_data = self._compute_next_inode_data(self._next_inode_number)
        self.assertEqual(new_data, expected_data)

        # Verify that there are no more errors reported
        self._run_fsck([])

    # TODO: replace untracked dir with file
    # TODO: replace untracked file with dir
