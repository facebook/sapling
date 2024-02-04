# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from __future__ import absolute_import, division, print_function, unicode_literals

import hashlib
import struct
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


def _write_path(writer: "Callable[[bytes], None]", path: str) -> None:
    byte_path = path.encode("utf-8")
    writer(struct.pack(">H", len(byte_path)))
    writer(byte_path)
