#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import struct
import tempfile
import typing
from pathlib import Path
from typing import BinaryIO, Type


class MdvHeader:
    # uint32_t magic;
    # uint32_t version; // 1
    # uint32_t recordVersion; // T::VERSION
    # uint32_t recordSize; // sizeof(T)
    # uint64_t entryCount; // end() - begin()
    # uint64_t padding;
    FORMAT = struct.Struct("=4sIIIQQ")

    MAGIC = b"MDV\0"
    VERSION_1 = 1

    def __init__(
        self,
        magic: bytes,
        version: int,
        record_version: int,
        record_size: int,
        entry_count: int,
    ) -> None:
        self.magic = magic
        self.version = version
        self.record_version = record_version
        self.record_size = record_size
        self.entry_count = entry_count

    def serialize(self) -> bytes:
        return self.FORMAT.pack(
            self.magic,
            self.version,
            self.record_version,
            self.record_size,
            self.entry_count,
            0,
        )

    @classmethod
    def parse(cls: Type["MdvHeader"], data: bytes) -> "MdvHeader":
        fields = cls.FORMAT.unpack(data)
        (magic, version, record_version, record_size, entry_count, _padding) = fields
        return cls(magic, version, record_version, record_size, entry_count)

    @classmethod
    def read(cls: Type["MdvHeader"], input_file: BinaryIO) -> "MdvHeader":
        data = input_file.read(cls.FORMAT.size)
        return cls.parse(data)


class InodeMetadataV0:
    # uint64_t inode_number
    # mode_t mode
    # uid_t uid
    # gid_t gid
    # uint32_t padding
    # uint64_t atime # encoded as EdenTimestamp (nanoseconds from 1901-12-13)
    # uint64_t mtime # EdenTimestamp
    # uint64_t ctime # EdenTimestamp
    FORMAT = struct.Struct("=QIIIIQQQ")
    VERSION = 0

    def __init__(
        self,
        inode_number: int,
        mode: int,
        uid: int,
        gid: int,
        atime: int,
        mtime: int,
        ctime: int,
    ) -> None:
        self.inode_number = inode_number
        self.mode = mode
        self.uid = uid
        self.gid = gid
        self.atime = atime
        self.mtime = mtime
        self.ctime = ctime

    def serialize(self) -> bytes:
        return self.FORMAT.pack(
            self.inode_number,
            self.mode,
            self.uid,
            self.gid,
            0,
            self.atime,
            self.mtime,
            self.ctime,
        )

    @classmethod
    def parse(cls: Type["InodeMetadataV0"], data: bytes) -> "InodeMetadataV0":
        fields = cls.FORMAT.unpack(data)
        (inode_number, mode, uid, gid, _padding, atime, mtime, ctime) = fields
        return cls(inode_number, mode, uid, gid, atime, mtime, ctime)

    @classmethod
    def read(cls: Type["InodeMetadataV0"], input_file: BinaryIO) -> "InodeMetadataV0":
        data = input_file.read(cls.FORMAT.size)
        if len(data) != cls.FORMAT.size:
            raise Exception(f"short inode metadata table header: size={len(data)}")
        return cls.parse(data)


def update_ownership(metadata_path: Path, uid: int, gid: int) -> None:
    """Update an Eden inode metadata table file, replacing the UID and GID fields
    for all inodes with the specified values.
    """
    with typing.cast(BinaryIO, metadata_path.open("rb")) as input_file:
        header = MdvHeader.read(input_file)

        if header.magic != MdvHeader.MAGIC:
            raise Exception(
                "unsupported inode metadata table file format: "
                f"magic={header.magic!r}"
            )
        if header.version != MdvHeader.VERSION_1:
            raise Exception(
                "unsupported inode metadata table file format: "
                f"version={header.version}"
            )
        if header.record_version != InodeMetadataV0.VERSION:
            raise Exception(
                "unsupported inode metadata table file format: "
                f"record_version={header.record_version}"
            )
        if header.record_size != InodeMetadataV0.FORMAT.size:
            raise Exception(
                "unsupported inode metadata table file format: "
                f"record_size: {header.record_size} != {InodeMetadataV0.FORMAT.size}"
            )

        tmp_fd, tmp_file_name = tempfile.mkstemp(
            dir=str(metadata_path.parent), prefix=metadata_path.name + "."
        )
        tmp_file = os.fdopen(tmp_fd, "wb")
        try:
            tmp_file.write(header.serialize())
            _rewrite_ownership_v0(input_file, tmp_file, header, uid, gid)
            tmp_file.close()
            tmp_file = None
            os.rename(tmp_file_name, metadata_path)
        except Exception:
            try:
                os.unlink(tmp_file_name)
            except Exception:
                pass
            raise
        finally:
            if tmp_file is not None:
                tmp_file.close()


def _rewrite_ownership_v0(
    input_file: BinaryIO, new_file: BinaryIO, header: MdvHeader, uid: int, gid: int
) -> None:
    entries_processed = 0
    entry_size = InodeMetadataV0.FORMAT.size
    for _ in range(header.entry_count):
        entries_processed += 1

        entry_data = input_file.read(entry_size)
        if len(entry_data) != entry_size:
            raise Exception("inode metadata table appears truncated")

        entry = InodeMetadataV0.parse(entry_data)
        entry.uid = uid
        entry.gid = gid
        new_file.write(entry.serialize())

    # Copy the remaining file contents as is.  This is normally all 0-filled data
    # that provides space for new entries to be written in the future.
    padding = input_file.read()
    new_file.write(padding)
