#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import pathlib
import re
import subprocess
import typing


def edenfs_systemd_service_name(eden_dir: pathlib.Path) -> str:
    assert isinstance(eden_dir, pathlib.PosixPath)
    instance_name = systemd_escape_path(eden_dir)
    return f"fb-edenfs@{instance_name}.service"


def systemd_escape_path(path: pathlib.PurePosixPath) -> str:
    """Escape a path for inclusion in a systemd unit name.

    See the 'systemd-escape --path' command for details.
    """
    if not path.is_absolute():
        raise ValueError("systemd_escape_path can only escape absolute paths")
    if ".." in path.parts:
        raise ValueError(
            "systemd_escape_path can only escape paths without '..' components"
        )
    stdout: bytes = subprocess.check_output(
        ["systemd-escape", "--path", "--", str(path)]
    )
    return stdout.decode("utf-8").rstrip("\n")


class EdenFSSystemdServiceConfig:
    __eden_dir: pathlib.Path
    __edenfs_executable_path: pathlib.Path
    __extra_edenfs_arguments: typing.List[str]

    def __init__(
        self,
        eden_dir: pathlib.Path,
        edenfs_executable_path: pathlib.Path,
        extra_edenfs_arguments: typing.Sequence[str],
    ) -> None:
        super().__init__()
        self.__eden_dir = eden_dir
        self.__edenfs_executable_path = edenfs_executable_path
        self.__extra_edenfs_arguments = list(extra_edenfs_arguments)

    @property
    def config_file_path(self) -> pathlib.Path:
        return self.__eden_dir / "systemd.conf"

    @property
    def startup_log_file_path(self) -> pathlib.Path:
        # TODO(T33122320): Move this into <eden_dir>/logs/.
        return self.__eden_dir / "startup.log"

    def write_config_file(self) -> None:
        variables = {
            b"EDENFS_EXECUTABLE_PATH": bytes(self.__edenfs_executable_path),
            b"EDENFS_EXTRA_ARGUMENTS": self.__escape_argument_list(
                self.__extra_edenfs_arguments
            ),
        }
        self.config_file_path.parent.mkdir(parents=True, exist_ok=True)
        self.config_file_path.write_bytes(SystemdEnvironmentFile.dumps(variables))

    @staticmethod
    def __escape_argument_list(arguments: typing.Sequence[str]) -> bytes:
        for argument in arguments:
            if "\n" in arguments:
                raise ValueError(
                    f"Newlines in arguments are not supported\nArgument: {argument!r}"
                )
        return b"\n".join(arg.encode("utf-8") for arg in arguments)


class SystemdEnvironmentFile:
    _comment_characters = b"#;"
    _escape_characters = b"\\"
    _name_characters = (
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_"
    )
    _newline_characters = b"\n\r"
    _quote_characters = b"'\""
    _whitespace_characters = b" \t"

    def __init__(self, entries: typing.Sequence[typing.Tuple[bytes, bytes]]) -> None:
        super().__init__()
        self.__entries = list(entries)

    @classmethod
    def loads(cls, content: bytes) -> "SystemdEnvironmentFile":
        content = _truncated_at_null_byte(content)
        entries = _EnvironmentFileParser(content).parse_entries()
        return cls(entries=entries)

    @classmethod
    def dumps(cls, variables: typing.Mapping[bytes, bytes]) -> bytes:
        output = bytearray()
        for name, value in variables.items():
            cls._validate_entry(name, value)
            output.extend(name)
            output.extend(b"=")
            output.extend(cls.__escape_value(value))
            output.extend(b"\n")
        return bytes(output)

    @staticmethod
    def __escape_value(value: bytes) -> bytes:
        return (
            b'"'
            # pyre-fixme[6]: Expected `AnyStr` for 2nd param but got `Callable[[Any],...
            + re.sub(b'[\\\\"]', lambda match: b"\\" + match.group(0), value)
            + b'"'
        )

    @classmethod
    def _is_valid_entry(cls, name: bytes, value: bytes) -> bool:
        try:
            cls._validate_entry(name, value)
            return True
        except (VariableNameError, VariableValueError):
            return False

    @classmethod
    def _validate_entry(cls, name: bytes, value: bytes) -> None:
        if not name:
            raise VariableNameError("Variables must have a non-empty name")
        if name[0:1].isdigit():
            raise VariableNameError("Variable names must not begin with a digit")
        for c in name:
            if c in cls._whitespace_characters:
                raise VariableNameError("Variable names must not contain whitespace")
            if c in cls._newline_characters:
                raise VariableNameError(
                    "Variable names must not contain any newline characters"
                )
            if c < 0x20:
                raise VariableNameError(
                    f"Variable names must not contain any control characters"
                )
            if c < 0x80 and c not in cls._name_characters:
                offending_character = bytes([c]).decode("utf-8")
                raise VariableNameError(
                    f"Variable names must not contain '{offending_character}'"
                )
        for c in value:
            if c in b"\r":
                raise VariableValueError(
                    "Variable values must not contain carriage returns"
                )
            if c < 0x20 and c not in b"\n\t":
                raise VariableValueError(
                    "Variable values must not contain any control characters"
                )

    @property
    def entries(self) -> typing.List[typing.Tuple[bytes, bytes]]:
        return self.__entries


class VariableNameError(ValueError):
    pass


class VariableValueError(ValueError):
    pass


class _Scanner:
    def __init__(self, input: bytes) -> None:
        super().__init__()
        self.__input = input
        self.__index = 0

    @property
    def at_eof(self) -> bool:
        return self.__index == len(self.__input)

    def scan_one_byte(self) -> int:
        if self.at_eof:
            raise ValueError("Cannot scan past end of file")
        c = self.__input[self.__index]
        self.__index += 1
        return c

    def peek_one_byte(self) -> int:
        if self.at_eof:
            raise ValueError("Cannot peek past end of file")
        return self.__input[self.__index]

    def skip_one_byte(self) -> None:
        if self.at_eof:
            raise ValueError("Cannot skip past end of file")
        self.__index += 1

    def scan_while_any(self, scan_bytes: typing.Sequence[int]) -> bytes:
        return self.__scan_while(lambda c: c in scan_bytes)

    def scan_until_any(self, stop_bytes: typing.Sequence[int]) -> bytes:
        return self.__scan_while(lambda c: c not in stop_bytes)

    def skip_while_any(self, skip_bytes: typing.Sequence[int]) -> None:
        self.__skip_while(lambda c: c in skip_bytes)

    def skip_until_any(self, stop_bytes: typing.Sequence[int]) -> None:
        self.__skip_while(lambda c: c not in stop_bytes)

    def __scan_while(self, scan_predicate: typing.Callable[[int], bool]) -> bytes:
        begin_index = self.__index
        while not self.at_eof:
            if not scan_predicate(self.__input[self.__index]):
                break
            self.__index += 1
        end_index = self.__index
        return self.__input[begin_index:end_index]

    def __skip_while(self, skip_predicate: typing.Callable[[int], bool]) -> None:
        while not self.at_eof:
            if not skip_predicate(self.__input[self.__index]):
                break
            self.__index += 1


class _EnvironmentFileParser(_Scanner):
    comment_characters = SystemdEnvironmentFile._comment_characters
    escape_characters = SystemdEnvironmentFile._escape_characters
    newline_characters = SystemdEnvironmentFile._newline_characters
    quote_characters = SystemdEnvironmentFile._quote_characters
    whitespace_characters = SystemdEnvironmentFile._whitespace_characters

    def parse_entries(self) -> typing.List[typing.Tuple[bytes, bytes]]:
        entries = []
        while not self.at_eof:
            entry = self.parse_entry()
            if entry is not None:
                entries.append(entry)
        return entries

    def parse_entry(self) -> typing.Optional[typing.Tuple[bytes, bytes]]:
        self.skip_whitespace()
        if self.at_eof:
            return None
        c = self.peek_one_byte()
        if c in self.comment_characters:
            self.parse_comment()
            return None
        elif c in self.newline_characters:
            self.skip_one_byte()
            return None

        name = self.parse_entry_name_and_equal_sign()
        if name is None:
            return None
        self.skip_whitespace()
        value = self.parse_entry_value()
        if not SystemdEnvironmentFile._is_valid_entry(name, value):
            return None
        return (name, value)

    def parse_entry_name_and_equal_sign(self) -> typing.Optional[bytes]:
        name = bytearray([self.scan_one_byte()])
        name.extend(self.scan_until_any(b"=" + self.newline_characters))
        if self.at_eof:
            return None
        c = self.scan_one_byte()
        if c in self.newline_characters:
            return None
        assert c == b"="[0]
        return bytes(name.rstrip(self.whitespace_characters))

    def parse_entry_value(self) -> bytes:
        value = bytearray()
        self.parse_quoted_entry_value(out_value=value)
        self.parse_unquoted_entry_value(out_value=value)
        return bytes(value)

    def parse_quoted_entry_value(self, out_value: bytearray) -> None:
        while not self.at_eof:
            c = self.peek_one_byte()
            if c not in self.quote_characters:
                return
            terminating_quote_characters = bytes([c])

            self.skip_one_byte()

            while not self.at_eof:
                scanned = self.scan_until_any(
                    self.escape_characters + terminating_quote_characters
                )
                out_value.extend(scanned)
                if self.at_eof:
                    return

                c = self.scan_one_byte()
                if c in self.escape_characters:
                    if self.at_eof:
                        return
                    c = self.scan_one_byte()
                    if c not in self.newline_characters:
                        out_value.append(c)
                elif c in terminating_quote_characters:
                    break
                else:
                    raise AssertionError()

            self.skip_whitespace()

    def parse_unquoted_entry_value(self, out_value: bytearray) -> None:
        while not self.at_eof:
            scanned = self.scan_until_any(
                self.escape_characters
                + self.newline_characters
                + self.whitespace_characters
            )
            out_value.extend(scanned)
            if self.at_eof:
                return

            c = self.scan_one_byte()
            if c in self.escape_characters:
                if self.at_eof:
                    return
                c = self.scan_one_byte()
                if c not in self.newline_characters:
                    out_value.append(c)
            elif c in self.newline_characters:
                return
            elif c in self.whitespace_characters:
                scanned = self.scan_while_any(self.whitespace_characters)
                is_trailing_whitespace = (
                    self.at_eof or self.peek_one_byte() in self.newline_characters
                )
                if is_trailing_whitespace:
                    return
                out_value.append(c)
                out_value.extend(scanned)
            else:
                raise AssertionError()

    def parse_comment(self) -> None:
        c = self.scan_one_byte()
        assert c in self.comment_characters
        while not self.at_eof:
            self.skip_until_any(self.escape_characters + self.newline_characters)
            if self.at_eof:
                break
            c = self.scan_one_byte()
            if c in self.escape_characters:
                if self.at_eof:
                    break
                self.skip_one_byte()
            elif c in self.newline_characters:
                break
            else:
                raise AssertionError()

    def skip_whitespace(self) -> None:
        self.skip_while_any(self.whitespace_characters)


def _truncated_at_null_byte(data: bytes) -> bytes:
    end_of_file_index = data.find(b"\x00")
    if end_of_file_index == -1:
        return data
    return data[:end_of_file_index]


class SystemdNotConfiguredError(Exception):
    def __str__(self) -> str:
        return (
            "The XDG_RUNTIME_DIR environment variable is not set, but it is "
            "required to communicate with the systemd user manager"
        )
