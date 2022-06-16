# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from __future__ import absolute_import, division, print_function, unicode_literals

import binascii
import hashlib
import struct
import sys
from typing import Callable, Dict, IO, Tuple


# Version number for the format of the .hg/dirstate file that is read/written by
# this library.
CURRENT_DIRSTATE_VERSION = 1

# Valid values for the merge state.
MERGE_STATE_NOT_APPLICABLE = 0
MERGE_STATE_BOTH_PARENTS = -1
MERGE_STATE_OTHER_PARENT = -2


def write(
    file: "IO[bytes]",
    parents: "Tuple[bytes, bytes]",
    tuples_dict: "Dict[str, Tuple[str, int, int]]",
    copymap: "Dict[str, str]",
) -> None:
    #
    # The serialization format of the dirstate is as follows:
    # - The first 40 bytes are the hashes of the two parent pointers.
    # - The next 4 bytes are the version number of the format.
    # - The next section is the dirstate tuples. Each dirstate tuple is
    #   represented as follows:
    #   - The first byte is '\x01'.
    #   - The second byte represents the status. It is the ASCII value of
    #     'n', 'm', 'r', 'a', '?', as appropriate.
    #   - The next four bytes are an unsigned integer representing mode_t.
    #   - The seventh byte (which is signed) represents the merge state:
    #     - 0 is NotApplicable
    #     - -1 is BothParents
    #     - -2 is OtherParent
    #   - The next two bytes are an unsigned short representing the length of
    #     the path, in bytes.
    #   - The bytes of the path itself. Note that a path cannot contain \0.
    # - The next section is the copymap. Each entry in the copymap is
    #   represented as follows.
    #   - The first byte is '\x02'.
    #   - An unsigned short (two bytes) representing the length, followed by
    #     that number of bytes, which constitutes the relative path name of the
    #     *destination* of the copy.
    #   - An unsigned short (two bytes) representing the length, followed by
    #     that number of bytes, which constitutes the relative path name of the
    #     *source* of the copy.
    # - The last section is the checksum. Although the other tuples can be
    #   interleaved or reordered without issue, the checksum must come last.
    #   The checksum is a function of all of the bytes written up to this point
    #   plus the \xFF header for the checksum section.
    #   - The first byte is '\xFF' to distinguish it from the other fields.
    #   - Because we use SHA-256 as the hash algorithm for the checksum, the
    #     remaining 32 bytes are used for the hash.
    sha = hashlib.sha256()

    def hashing_write(data: bytes) -> None:
        sha.update(data)
        file.write(data)

    hashing_write(parents[0])
    hashing_write(parents[1])
    hashing_write(struct.pack(">I", CURRENT_DIRSTATE_VERSION))
    for path, dirstate_tuple in tuples_dict.items():
        status, mode, merge_state = dirstate_tuple
        hashing_write(b"\x01")
        hashing_write(struct.pack(">BIb", ord(status), mode, merge_state))
        _write_path(hashing_write, path)
    for dest, source in copymap.items():
        hashing_write(b"\x02")
        _write_path(hashing_write, dest)
        _write_path(hashing_write, source)
    hashing_write(b"\xFF")

    # Write the checksum, so we use file.write() instead of hashing_write().
    file.write(sha.digest())


def read(
    fp: "IO[bytes]", filename: str
) -> "Tuple[Tuple[bytes, bytes], Dict[str, Tuple[str, int, int]], Dict[str, str]]":  # noqa: C901
    """Returns a tuple of (parents, tuples_dict, copymap) if successful.

    Any exception from create_file(), such as IOError with errno == ENOENT, will
    be bubbled up to the caller.

    If contents of the dirstate file do not match the expected format, then a
    DirstateParseException will be thrown.
    """
    parents = None
    tuples_dict = {}
    copymap = {}

    sha = hashlib.sha256()

    def hashing_read(num):
        data = fp.read(num)
        sha.update(data)
        return data

    parent_bytes = hashing_read(40)
    num_parents_bytes = len(parent_bytes)
    if num_parents_bytes != 40:
        raise DirstateParseException(
            "Reached EOF while reading dirstate parents in {}.\n".format(filename)
        )
    parents = parent_bytes[:20], parent_bytes[20:40]

    binary_version = hashing_read(4)
    if len(binary_version) != 4:
        raise DirstateParseException(
            "Reached EOF while reading the version number in {}.\n".format(filename)
        )
    version: int = struct.unpack(">I", binary_version)[0]
    if version != CURRENT_DIRSTATE_VERSION:
        raise DirstateParseException(
            "Unknown dirstate version in {}. Found {} but expected {}.\n".format(
                filename, version, CURRENT_DIRSTATE_VERSION
            )
        )

    while True:
        header = hashing_read(1)
        if not header:
            # We have reached the end of the file.
            break
        elif header == b"\x01":
            scalars = hashing_read(6)
            if len(scalars) != 6:
                raise DirstateParseException(
                    "Malformed dirstate tuple in {}".format(filename)
                    + ". Aborting read().\n"
                )
            path = _read_path(hashing_read, filename)
            status: int = 0
            mode: int = 0
            merge: int = 0
            status, mode, merge = struct.unpack(">BIb", scalars)
            # TODO(mbolin): Verify status and merge?
            tuples_dict[path] = (chr(status), mode, merge)
        elif header == b"\x02":
            dest = _read_path(hashing_read, filename)
            source = _read_path(hashing_read, filename)
            copymap[dest] = source
        elif header == b"\xFF":
            # Reading the checksum, so we use fp.read() instead of
            # hashing_read().
            binary_checksum = fp.read(32)
            if len(binary_checksum) != 32:
                raise DirstateParseException(
                    "Reached EOF while reading checksum hash in {}.\n".format(filename)
                )
            digest = sha.digest()
            if binary_checksum == digest:
                if fp.read(1) == b"":
                    # There is no more data, as expected.
                    break
                else:
                    raise DirstateParseException(
                        "Suspicious data is present after "
                        "the end of the valid checksum in {}.\n".format(filename)
                    )
            else:
                raise DirstateParseException(
                    "Checksum mismatch when reading {}. Observed checksum is "
                    "{}, but the checksum in the file is {}.\n".format(
                        filename,
                        binascii.hexlify(digest),
                        binascii.hexlify(binary_checksum),
                    )
                )
        else:
            raise DirstateParseException(
                "Unexpected header byte "
                "when reading {:s}: 0x{:x}.".format(filename, header)
                + " Ignoring remaining dirstate data.\n"
            )

    return parents, tuples_dict, copymap


def _write_path(writer: "Callable[[bytes], None]", path: str) -> None:
    if sys.version_info[0] < 3:
        byte_path = path
    else:
        byte_path = path.encode("utf-8")
    writer(struct.pack(">H", len(byte_path)))
    writer(byte_path)


def _read_path(reader: "Callable[[int], bytes]", filename: str) -> str:
    binary_path_len = reader(2)
    if len(binary_path_len) != 2:
        raise DirstateParseException(
            "Reached EOF while reading path length in {}.\n".format(filename)
        )

    path_len: int = struct.unpack(">H", binary_path_len)[0]
    path = reader(path_len)
    if len(path) == path_len:
        if isinstance(path, str):
            # Python 2.
            return path
        else:
            # Python 3
            return str(path, "utf8")
    else:
        raise DirstateParseException(
            "Reached EOF while reading path in {}.\n".format(filename)
        )


class DirstateParseException(Exception):
    pass
