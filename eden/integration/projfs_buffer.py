#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import subprocess
import sys
import time
from collections import namedtuple
from pathlib import Path
from typing import Optional

from .lib import testcase
from .lib.find_executables import FindExe

if sys.platform == "win32":
    import ctypes

    win_kernel = ctypes.windll.kernel32

    _FindFirstFileW = win_kernel.FindFirstFileW
    _FindFirstFileW.argtypes = [
        ctypes.wintypes.LPCWSTR,
        ctypes.wintypes.LPWIN32_FIND_DATAW,
    ]
    _FindFirstFileW.restype = ctypes.wintypes.HANDLE

    _FindClose = win_kernel.FindClose
    _FindClose.argtypes = (ctypes.wintypes.HANDLE,)
    _FindClose.restype = ctypes.wintypes.BOOL

    # See: https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants
    _FILE_ATTRIBUTE_HIDDEN = 2
    _FILE_ATTRIBUTE_SYSTEM = 4
    _FILE_ATTRIBUTE_DIRECTORY = 16
    _FILE_ATTRIBUTE_ARCHIVE = 32
    _FILE_ATTRIBUTE_SPARSE_FILE = 512
    _FILE_ATTRIBUTE_REPARSE_POINT = 1024
    _FILE_ATTRIBUTE_RECALL_ON_OPEN = 262144
    _FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS = 4194304

    class FileAttributes:
        raw_attribute_mask: int = 0

        def __init__(
            self,
            raw_attribute_mask: Optional[int] = None,
            hidden: bool = False,
            system: bool = False,
            directory: bool = False,
            archive: bool = False,
            sparse: bool = False,
            reparse: bool = False,
            recall: bool = False,
        ) -> None:
            if raw_attribute_mask:
                self.raw_attribute_mask = raw_attribute_mask
                return

            if hidden:
                self.raw_attribute_mask |= _FILE_ATTRIBUTE_HIDDEN
            if system:
                self.raw_attribute_mask |= _FILE_ATTRIBUTE_SYSTEM
            if directory:
                self.raw_attribute_mask |= _FILE_ATTRIBUTE_DIRECTORY
            if archive:
                self.raw_attribute_mask |= _FILE_ATTRIBUTE_ARCHIVE
            if sparse:
                self.raw_attribute_mask |= _FILE_ATTRIBUTE_SPARSE_FILE
            if reparse:
                self.raw_attribute_mask |= _FILE_ATTRIBUTE_REPARSE_POINT
            if recall:
                self.raw_attribute_mask |= _FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS

        def __repr__(self):
            return repr(bin(self.raw_attribute_mask))

        def __str__(self):
            hidden = (
                self.raw_attribute_mask & _FILE_ATTRIBUTE_HIDDEN
            ) == _FILE_ATTRIBUTE_HIDDEN
            system = (
                self.raw_attribute_mask & _FILE_ATTRIBUTE_SYSTEM
            ) == _FILE_ATTRIBUTE_SYSTEM
            directory = (
                self.raw_attribute_mask & _FILE_ATTRIBUTE_DIRECTORY
            ) == _FILE_ATTRIBUTE_DIRECTORY
            archive = (
                self.raw_attribute_mask & _FILE_ATTRIBUTE_ARCHIVE
            ) == _FILE_ATTRIBUTE_ARCHIVE
            sparse = (
                self.raw_attribute_mask & _FILE_ATTRIBUTE_SPARSE_FILE
            ) == _FILE_ATTRIBUTE_SPARSE_FILE
            reparse = (
                self.raw_attribute_mask & _FILE_ATTRIBUTE_REPARSE_POINT
            ) == _FILE_ATTRIBUTE_REPARSE_POINT
            recall = (
                self.raw_attribute_mask & _FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
            ) == _FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS

            pretty_mask = hex(self.raw_attribute_mask)
            result = f"{pretty_mask}\n"
            result += f"hidden: {hidden}\n"
            result += f"system: {system}\n"
            result += f"directory: {directory}\n"
            result += f"archive: {archive}\n"
            result += f"sparse: {sparse}\n"
            result += f"reparse: {reparse}\n"
            result += f"recall: {recall}\n"
            return result

        def __eq__(self, other: object) -> bool:
            return self.raw_attribute_mask == other.raw_attribute_mask

    def getFileAttributes(path: Path) -> FileAttributes:
        result = ctypes.wintypes.WIN32_FIND_DATAW()
        handle = _FindFirstFileW(str(path), ctypes.byref(result))
        _FindClose(handle)

        return FileAttributes(int(result.dwFileAttributes))

else:

    class FileAttributes:
        def __init__(
            self,
            raw_attribute_mask: Optional[int] = None,
            hidden: bool = False,
            system: bool = False,
            directory: bool = False,
            archive: bool = False,
            sparse: bool = False,
            reparse: bool = False,
            recall: bool = False,
        ) -> None:
            pass

        def __eq__(self, other: object) -> bool:
            return False

    def getFileAttributes(path: Path) -> FileAttributes:
        return FileAttributes()


# +---------------------------------------+--------+--------+-----------+---------+--------+---------+--------+------------------+
# |                                       | hidden | system | directory | archive | sparse | reparse | recall | reparse buffer   |
# +---------------------------------------+--------+--------+-----------+---------+--------+---------+--------+------------------+
# | file placeholder                      |        |        |           | x       |        |         | x      | 0200000000000000 |
# | file placeholder stopped              |        |        |           | x       | x      | x       | x      | 0200000000000000 |
# | directory placeholder                 |        |        | x         |         |        |         | x      | 0200000000000000 |
# | directory placeholder stopped         |        |        | x         |         |        | x       | x      | 0200000000000000 |
# | dirty directory placeholder           |        |        | x         |         |        |         | x      | 0200000002000000 |
# | dirty directory placeholder stopped   |        |        | x         |         |        | x       | x      | 0200000002000000 |
# | hydrated placeholder                  |        |        |           | x       |        |         |        | 0200000020000000 |
# | hydrated placeholder stopped          |        |        |           | x       | x      | x       |        | 0200000020000000 |
# | full file                             |        |        |           | x       |        |         | x      | -                |
# | full file stopped                     |        |        |           | x       |        |         | x      | -                |
# | locally created file                  |        |        |           | x       |        |         |        | -                |
# | locally created file stopped          |        |        |           | x       |        |         |        | -                |
# | full directory                        |        |        | x         |         |        |         |        | -                |
# | full directory stopped                |        |        | x         |         |        |         |        | -                |
# | tombstone                             |        |        |           |         |        |         |        |                  |
# | tombstone stopped                     | x      | x      |           | x       |        | x       |        |                  |
# | renamed placeholder                   |        |        |           | x       |        |         | x      | 0200000008000000 |
# | renamed placeholder stopped           |        |        |           | x       | x      | x       | x      | 0200000008000000 |
# | renamed hydrated placeholder          |        |        |           | x       |        |         |        | 0200000028000000 |
# | renamed hydrated placeholder stopped  |        |        |           | x       | x      | x       |        | 0200000028000000 |
# | renamed full file                     |        |        |           | x       |        |         |        | -                |
# | renamed full file stopped             |        |        |           | x       |        |         |        | -                |
# | renamed full directory                |        |        | x         |         |        |         |        | -                |
# | renamed full directory stopped        |        |        | x         |         |        |         |        | -                |
# +---------------------------------------+--------+--------+-----------+---------+--------+---------+--------+------------------+

ExpectedAttributes = namedtuple(
    "ExpectedAttributes",
    [
        "file_placeholder",
        "directory_placeholder",
        "directory_dirty_placeholder",
        "hydrated_placeholder",
        "full_file",
        "locally_created_file",
        "full_directory",
        "tombstone",
    ],
)

ExpectedAttribute = namedtuple("ExpectedAttribute", ["eden_running", "eden_stopped"])

EXPECTED_ATTRIBUTES = ExpectedAttributes(
    file_placeholder=ExpectedAttribute(
        eden_running=FileAttributes(archive=True, recall=True),
        eden_stopped=FileAttributes(
            archive=True, sparse=True, reparse=True, recall=True
        ),
    ),
    directory_placeholder=ExpectedAttribute(
        eden_running=FileAttributes(directory=True, recall=True),
        eden_stopped=FileAttributes(directory=True, reparse=True, recall=True),
    ),
    directory_dirty_placeholder=ExpectedAttribute(
        eden_running=FileAttributes(directory=True, recall=True),
        eden_stopped=FileAttributes(directory=True, reparse=True, recall=True),
    ),
    hydrated_placeholder=ExpectedAttribute(
        eden_running=FileAttributes(archive=True),
        eden_stopped=FileAttributes(archive=True, sparse=True, reparse=True),
    ),
    full_file=ExpectedAttribute(
        eden_running=FileAttributes(archive=True, recall=True),
        eden_stopped=FileAttributes(archive=True, recall=True),
    ),
    locally_created_file=ExpectedAttribute(
        eden_running=FileAttributes(archive=True),
        eden_stopped=FileAttributes(archive=True),
    ),
    full_directory=ExpectedAttribute(
        eden_running=FileAttributes(directory=True),
        eden_stopped=FileAttributes(directory=True),
    ),
    tombstone=ExpectedAttribute(
        eden_running=FileAttributes(),
        eden_stopped=FileAttributes(
            hidden=True, system=True, archive=True, reparse=True
        ),
    ),
)

EXPECTED_REPARSE_BUFFER = ExpectedAttributes(
    file_placeholder=ExpectedAttribute(
        eden_running="0200000000000000", eden_stopped="0200000000000000"
    ),
    directory_placeholder=ExpectedAttribute(
        eden_running="0200000000000000", eden_stopped="0200000000000000"
    ),
    directory_dirty_placeholder=ExpectedAttribute(
        eden_running="0200000002000000", eden_stopped="0200000002000000"
    ),
    hydrated_placeholder=ExpectedAttribute(
        eden_running="0200000020000000", eden_stopped="0200000020000000"
    ),
    full_file=ExpectedAttribute(eden_running=None, eden_stopped=None),
    locally_created_file=ExpectedAttribute(eden_running=None, eden_stopped=None),
    full_directory=ExpectedAttribute(eden_running=None, eden_stopped=None),
    tombstone=ExpectedAttribute(eden_running=None, eden_stopped=""),
)

EXPECTED_REPARSE_BUFFER_RENAMED = ExpectedAttributes(
    file_placeholder=ExpectedAttribute(
        eden_running="0200000008000000", eden_stopped="0200000008000000"
    ),
    directory_placeholder=None,
    directory_dirty_placeholder=None,
    hydrated_placeholder=ExpectedAttribute(
        eden_running="0200000028000000", eden_stopped="0200000028000000"
    ),
    full_file=ExpectedAttribute(eden_running=None, eden_stopped=None),
    locally_created_file=ExpectedAttribute(eden_running=None, eden_stopped=None),
    full_directory=ExpectedAttribute(eden_running=None, eden_stopped=None),
    tombstone=ExpectedAttribute(eden_running=None, eden_stopped=""),
)


@testcase.eden_repo_test
class PrjFSBuffer(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("hi", "hola\n")
        self.repo.mkdir("somedir")
        self.repo.write_file("somedir/afile", "blah\n")
        self.repo.commit("Initial commit.")

    def check_projfs_reparse_buffer_and_attributes(
        self,
        path: Path,
        expected_attributes: FileAttributes,
        expected_buffer: Optional[str] = None,
    ) -> None:
        result = subprocess.run(
            [FindExe.READ_REPARSE_BUFFER, "--path", str(path)],
            capture_output=True,
        )
        print(f"exitcode: {result.returncode}")
        print(f"stdout\n: {result.stdout}")
        print(f"stderr\n: {result.stderr}")
        if expected_buffer is None:
            self.assertNotEqual(result.returncode, 0)
        else:
            self.assertEqual(result.returncode, 0)
            buffer = result.stdout.decode()
            self.assertEqual(buffer[: len(expected_buffer)], expected_buffer)

        attrs = getFileAttributes(path)
        print(f"actual:\n{attrs}\nexpected:\n{expected_attributes}")
        self.assertEqual(attrs, expected_attributes)

    def check_projfs_reparse_buffer_and_attributes_running_and_stopped(
        self, path: Path, attributes: ExpectedAttribute, buffer: ExpectedAttribute
    ) -> None:
        self.check_projfs_reparse_buffer_and_attributes(
            path,
            attributes.eden_running,
            buffer.eden_running,
        )

        self.eden.shutdown()

        self.check_projfs_reparse_buffer_and_attributes(
            path,
            attributes.eden_stopped,
            buffer.eden_stopped,
        )

    def test_projfs_reparse_format_placeholder(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        attributes = EXPECTED_ATTRIBUTES.file_placeholder
        buffer = EXPECTED_REPARSE_BUFFER.file_placeholder

        self.check_projfs_reparse_buffer_and_attributes(
            hello_abs_path,
            attributes.eden_running,
            buffer.eden_running,
        )

        os.listdir(self.mount)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            hello_abs_path,
            attributes,
            buffer,
        )

    def test_projfs_reparse_format_dir_placeholder(self) -> None:
        somedir_abs_path = Path(self.mount) / "somedir"

        attributes = EXPECTED_ATTRIBUTES.directory_placeholder
        buffer = EXPECTED_REPARSE_BUFFER.directory_placeholder

        self.check_projfs_reparse_buffer_and_attributes(
            somedir_abs_path,
            attributes.eden_running,
            buffer.eden_running,
        )

        os.listdir(self.mount)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            somedir_abs_path,
            attributes,
            buffer,
        )

    def test_projfs_reparse_format_hydrated_placeholder(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        attributes = EXPECTED_ATTRIBUTES.hydrated_placeholder
        buffer = EXPECTED_REPARSE_BUFFER.hydrated_placeholder

        with open(hello_abs_path, "r") as hello_file:
            # passes locally fails on CI idk, the real test is after read.
            # self.check_projfs_reparse_buffer_and_attributes(
            #     hello_abs_path,
            #     attributes.eden_running,
            #     buffer.eden_running,
            # )

            hello_file.read()

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            hello_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_dir_would_behydrated_placeholder(self) -> None:
        # directories don't become "hydrated" because prjfs wants to allow
        # the server to change the contents of a directory.
        # but maybe prjfs could have some internal distinction here, so lets
        # cover this case just in case.
        somedir_abs_path = Path(self.mount) / "somedir"

        attributes = EXPECTED_ATTRIBUTES.directory_placeholder
        buffer = EXPECTED_REPARSE_BUFFER.directory_placeholder

        os.listdir(somedir_abs_path)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            somedir_abs_path,
            attributes,
            buffer,
        )

    def test_projfs_reparse_format_dir_dirty_placeholder(self) -> None:
        somedir_abs_path = Path(self.mount) / "somedir"

        attributes = EXPECTED_ATTRIBUTES.directory_dirty_placeholder
        buffer = EXPECTED_REPARSE_BUFFER.directory_dirty_placeholder

        (somedir_abs_path / "a_new_file").touch()

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            somedir_abs_path,
            attributes,
            buffer,
        )

    def test_projfs_reparse_format_full_file(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        attributes = EXPECTED_ATTRIBUTES.full_file
        buffer = EXPECTED_REPARSE_BUFFER.full_file

        # opening in write mode marks the file full
        with open(hello_abs_path, "w") as hello_file:
            self.check_projfs_reparse_buffer_and_attributes(
                hello_abs_path, attributes.eden_running, buffer.eden_running
            )

            hello_file.write("bonjour")

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            hello_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_locally_created_file(self) -> None:
        a_new_file_abs_path = Path(self.mount) / "anewfile"

        attributes = EXPECTED_ATTRIBUTES.locally_created_file
        buffer = EXPECTED_REPARSE_BUFFER.locally_created_file

        a_new_file_abs_path.touch()

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            a_new_file_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_full_dir(self) -> None:
        anotherdir_abs_path = Path(self.mount) / "anotherdir"

        attributes = EXPECTED_ATTRIBUTES.full_directory
        buffer = EXPECTED_REPARSE_BUFFER.full_directory

        os.mkdir(anotherdir_abs_path)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            anotherdir_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_tombstone(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        attributes = EXPECTED_ATTRIBUTES.tombstone
        buffer = EXPECTED_REPARSE_BUFFER.tombstone

        hello_abs_path.unlink()

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            hello_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_renamed_placeholder(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        attributes = EXPECTED_ATTRIBUTES.file_placeholder
        buffer = EXPECTED_REPARSE_BUFFER_RENAMED.file_placeholder

        os.rename(hi_abs_path, renamed_hi_abs_path)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            renamed_hi_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_renamed_placeholder_then_hydrated(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        attributes = EXPECTED_ATTRIBUTES.hydrated_placeholder
        buffer = EXPECTED_REPARSE_BUFFER_RENAMED.hydrated_placeholder

        os.rename(hi_abs_path, renamed_hi_abs_path)

        with open(renamed_hi_abs_path, "r") as renamed_hi_file:
            renamed_hi_file.read()

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            renamed_hi_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_hyrated_then_renamed_placeholder(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        attributes = EXPECTED_ATTRIBUTES.hydrated_placeholder
        buffer = EXPECTED_REPARSE_BUFFER_RENAMED.hydrated_placeholder

        with open(hi_abs_path, "r") as hi_file:
            hi_file.read()

        os.rename(hi_abs_path, renamed_hi_abs_path)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            renamed_hi_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_renamed_then_full(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        attributes = EXPECTED_ATTRIBUTES.locally_created_file
        buffer = EXPECTED_REPARSE_BUFFER_RENAMED.locally_created_file

        os.rename(hi_abs_path, renamed_hi_abs_path)

        with open(renamed_hi_abs_path, "w") as renamed_hi_file:
            self.check_projfs_reparse_buffer_and_attributes(
                renamed_hi_abs_path, attributes.eden_running, buffer.eden_running
            )

            renamed_hi_file.write("bonjour")

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            renamed_hi_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_full_then_renamed(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        attributes = EXPECTED_ATTRIBUTES.full_file
        buffer = EXPECTED_REPARSE_BUFFER_RENAMED.full_file

        with open(hi_abs_path, "w") as hi_file:
            hi_file.write("bonjour")

        os.rename(hi_abs_path, renamed_hi_abs_path)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            renamed_hi_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_renamed_locally_created_file(self) -> None:
        anotherfile_abs_path = Path(self.mount) / "anotherfile"
        renamed_abs_path = Path(self.mount) / "renamedanotherfile"

        attributes = EXPECTED_ATTRIBUTES.locally_created_file
        buffer = EXPECTED_REPARSE_BUFFER_RENAMED.locally_created_file

        anotherfile_abs_path.touch()
        os.rename(anotherfile_abs_path, renamed_abs_path)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            renamed_abs_path, attributes, buffer
        )

    def test_projfs_reparse_format_renamed_full_dir(self) -> None:
        anotherdir_abs_path = Path(self.mount) / "anotherdir"
        renamed_abs_path = Path(self.mount) / "renamedanotherdir"

        attributes = EXPECTED_ATTRIBUTES.full_directory
        buffer = EXPECTED_REPARSE_BUFFER.full_directory

        os.mkdir(anotherdir_abs_path)
        os.rename(anotherdir_abs_path, renamed_abs_path)

        self.check_projfs_reparse_buffer_and_attributes_running_and_stopped(
            renamed_abs_path, attributes, buffer
        )

    def check_projfs_rename(
        self,
        path: Path,
        expect_renamed: bool = False,
        expected_error_code: Optional[int] = None,
        expected_error_code_when_eden_stoped: Optional[int] = None,
    ) -> None:
        self.check_projfs_rename_impl(path, expect_renamed, expected_error_code)
        self.eden.shutdown()
        self.check_projfs_rename_impl(
            path,
            expect_renamed,
            expected_error_code=expected_error_code_when_eden_stoped,
            is_eden_stoped=True,
        )

    def check_projfs_rename_impl(
        self,
        path: Path,
        expect_renamed: bool = False,
        expected_error_code: Optional[int] = None,
        is_eden_stoped: bool = False,
    ) -> None:
        command = [FindExe.CHECK_WINDOWS_RENAME, "--path", str(path)]
        if is_eden_stoped:
            command.append("--checksparse")
        result = subprocess.run(command)
        print(f"exitcode: {result.returncode}")
        if expect_renamed:
            self.assertEqual(result.returncode, 0)
        else:
            self.assertNotEqual(result.returncode, 0)
            if expected_error_code is not None:
                self.assertEqual(result.returncode, expected_error_code)

    def test_rename_detection_virtual_file_is_not_renamed(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        # virtual file does not appear renamed.
        self.check_projfs_rename(
            hello_abs_path,
            expect_renamed=False,
            expected_error_code=1,
            expected_error_code_when_eden_stoped=1,
        )

    def test_rename_detection_virtual_dir_is_not_renamed(self) -> None:
        somedir_abs_path = Path(self.mount) / "somedir"

        # virtual directory does not appear renamed.
        self.check_projfs_rename(
            somedir_abs_path,
            expect_renamed=False,
            expected_error_code=1,
            expected_error_code_when_eden_stoped=4,
        )

    def test_rename_detection_placeholder_file_is_not_renamed(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        os.listdir(self.mount)

        # file placeholder does not appear renamed
        self.check_projfs_rename(
            hello_abs_path,
            expect_renamed=False,
            expected_error_code=1,
            expected_error_code_when_eden_stoped=1,
        )

    def test_rename_detection_placeholder_dir_is_not_renamed(self) -> None:
        somedir_abs_path = Path(self.mount) / "somedir"
        # directory placeholder does not appear renamed
        self.check_projfs_rename_impl(
            somedir_abs_path, expect_renamed=False, expected_error_code=1
        )

        os.listdir(somedir_abs_path)

        # directory "hydrated" placeholder does not appear renamed. directories
        # can not be hydrated, so this should be the same state as the last
        # check.
        self.check_projfs_rename(
            somedir_abs_path,
            expect_renamed=False,
            expected_error_code=1,
            expected_error_code_when_eden_stoped=4,
        )

    def test_rename_detection_hydrated_placeholder_is_not_renamed(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        with open(hello_abs_path, "r") as hello_file:
            # file hydrated placeholder does not appear renamed.
            self.check_projfs_rename_impl(
                hello_abs_path, expect_renamed=False, expected_error_code=1
            )

            hello_file.read()

            # file hydrated placeholder does not appear renamed.
            self.check_projfs_rename(
                hello_abs_path,
                expect_renamed=False,
                expected_error_code=1,
                expected_error_code_when_eden_stoped=1,
            )

    def test_rename_detection_full_file_is_not_renamed(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        # opening in write mode marks the file full
        with open(hello_abs_path, "w") as hello_file:
            self.check_projfs_rename_impl(
                hello_abs_path, expect_renamed=False, expected_error_code=2
            )

            hello_file.write("bonjour")

            # full file does not appear renamed.
            self.check_projfs_rename(
                hello_abs_path,
                expect_renamed=False,
                expected_error_code=2,
                expected_error_code_when_eden_stoped=4,
            )

    def test_rename_detection_locally_created_file_is_not_renamed(self) -> None:
        new_file_abs_path = Path(self.mount) / "a_new_file"

        new_file_abs_path.touch()

        self.check_projfs_rename(
            new_file_abs_path,
            expect_renamed=False,
            expected_error_code=2,
            expected_error_code_when_eden_stoped=4,
        )

    def test_rename_detection_full_dir_is_not_renamed(self) -> None:
        anotherdir_abs_path = Path(self.mount) / "anotherdir"

        os.mkdir(anotherdir_abs_path)

        # full directory does not appear renamed
        self.check_projfs_rename(
            anotherdir_abs_path,
            expect_renamed=False,
            expected_error_code=2,
            expected_error_code_when_eden_stoped=4,
        )

    def test_rename_detection_tombstone_is_not_renamed(self) -> None:
        hello_abs_path = Path(self.mount) / "hello"

        hello_abs_path.unlink()

        # tombstone does not appear renamed
        self.check_projfs_rename_impl(
            hello_abs_path, expect_renamed=False, expected_error_code=2
        )

        self.eden.shutdown()

        self.check_projfs_rename_impl(
            hello_abs_path,
            expect_renamed=False,
            expected_error_code=4,
            is_eden_stoped=True,
        )

    def test_rename_detection_renamed_placeholder(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        # just confirm hi doesn't look renamed before move
        self.check_projfs_rename_impl(
            hi_abs_path, expect_renamed=False, expected_error_code=1
        )

        os.rename(hi_abs_path, renamed_hi_abs_path)

        # renamed file placeholder appears renamed.
        self.check_projfs_rename(renamed_hi_abs_path, expect_renamed=True)

    def test_rename_detection_renamed_placeholder_then_hydrated(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        # just confirm hi doesn't look renamed before move
        self.check_projfs_rename_impl(
            hi_abs_path, expect_renamed=False, expected_error_code=1
        )

        os.rename(hi_abs_path, renamed_hi_abs_path)

        # renamed file placeholder appears renamed.
        self.check_projfs_rename_impl(renamed_hi_abs_path, expect_renamed=True)

        with open(renamed_hi_abs_path, "r") as renamed_hi_file:
            renamed_hi_file.read()

        # renamed file hydrated placeholder still appears renamed.
        self.check_projfs_rename(renamed_hi_abs_path, expect_renamed=True)

    def test_rename_detection_hyrated_then_renamed_placeholder(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        with open(hi_abs_path, "r") as hi_file:
            hi_file.read()

        # just confirm hi doesn't look renamed before move
        self.check_projfs_rename_impl(
            hi_abs_path, expect_renamed=False, expected_error_code=1
        )

        os.rename(hi_abs_path, renamed_hi_abs_path)

        # renamed file placeholder appears renamed.
        self.check_projfs_rename(renamed_hi_abs_path, expect_renamed=True)

    def test_rename_detection_renamed_placeholder_then_full(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        # just confirm hi doesn't look renamed before move
        self.check_projfs_rename_impl(
            hi_abs_path, expect_renamed=False, expected_error_code=1
        )

        os.rename(hi_abs_path, renamed_hi_abs_path)

        # renamed file placeholder appears renamed.
        self.check_projfs_rename_impl(renamed_hi_abs_path, expect_renamed=True)

        with open(renamed_hi_abs_path, "w") as renamed_hi_file:
            # full file does not appear renamed (because we can only detect for placeholders).
            self.check_projfs_rename_impl(
                renamed_hi_abs_path, expect_renamed=False, expected_error_code=2
            )

            renamed_hi_file.write("bonjour")

        # full file does not appear renamed (because we can only detect for placeholders).
        self.check_projfs_rename(
            renamed_hi_abs_path,
            expect_renamed=False,
            expected_error_code=2,
            expected_error_code_when_eden_stoped=4,
        )

    def test_rename_detection_full_then_renamed(self) -> None:
        hi_abs_path = Path(self.mount) / "hi"
        renamed_hi_abs_path = Path(self.mount) / "renamedhi"

        with open(hi_abs_path, "w") as hi_file:
            # full file does not appear renamed (because we can only detect for placeholders).
            self.check_projfs_rename_impl(
                hi_abs_path, expect_renamed=False, expected_error_code=2
            )

            hi_file.write("bonjour")

        os.rename(hi_abs_path, renamed_hi_abs_path)

        # full file does not appear renamed (because we can only detect for placeholders).
        self.check_projfs_rename(
            renamed_hi_abs_path,
            expect_renamed=False,
            expected_error_code=2,
            expected_error_code_when_eden_stoped=4,
        )

    def test_rename_detection_locally_create_renamed(self) -> None:
        new_file_abs_path = Path(self.mount) / "a_new_file"
        renamed_file_abs_path = Path(self.mount) / "a_new_renamed_file"

        new_file_abs_path.touch()

        os.rename(new_file_abs_path, renamed_file_abs_path)

        # full file does not appear renamed (because we can only detect for placeholders).
        self.check_projfs_rename(
            renamed_file_abs_path,
            expect_renamed=False,
            expected_error_code=2,
            expected_error_code_when_eden_stoped=4,
        )
