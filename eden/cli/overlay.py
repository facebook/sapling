#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import errno
import fcntl
import logging
import os
import shutil
import stat
import struct
import tempfile
import time
import typing
from pathlib import Path
from typing import BinaryIO, Iterator, Optional, Tuple

from facebook.eden.overlay.ttypes import OverlayDir, OverlayEntry


class InvalidOverlayFile(Exception):
    pass


class NoSuchOverlayFile(Exception):
    def __init__(self, inode_number: int) -> None:
        super().__init__(f"inode {inode_number} is not materialized in the overlay")
        self.inode_number = inode_number


class InodeLookupError(Exception):
    def __init__(self, msg: str, errnum: int) -> None:
        super().__init__(msg)
        self.errno = errnum


class OverlayHeader:
    LENGTH = 64
    VERSION_1 = 1

    TYPE_DIR = b"OVDR"
    TYPE_FILE = b"OVFL"

    STRUCT_FORMAT = ">4sIQQQQQQ8s"

    @classmethod
    def parse(cls, data: bytes, type: Optional[bytes] = None) -> "OverlayHeader":
        # A 0-length file is somewhat common on unclean reboot,
        # so use a separate exception message for this case.
        if len(data) == 0:
            raise InvalidOverlayFile("zero-sized overlay file")
        if len(data) < cls.LENGTH:
            raise InvalidOverlayFile(
                "overlay file is too short to contain a header: length={len(data)}"
            )

        (
            header_id,
            version,
            atime_sec,
            atime_nsec,
            ctime_sec,
            ctime_nsec,
            mtime_sec,
            mtime_nsec,
            padding,
        ) = struct.unpack(cls.STRUCT_FORMAT, data)
        if header_id not in (cls.TYPE_DIR, cls.TYPE_FILE):
            raise InvalidOverlayFile(
                "overlay file is too short to contain a header: length={len(data)}"
            )
        if version != cls.VERSION_1:
            raise InvalidOverlayFile(f"unsupported overlay file version {version}")

        return OverlayHeader(
            header_id,
            version,
            atime_sec,
            atime_nsec,
            ctime_sec,
            ctime_nsec,
            mtime_sec,
            mtime_nsec,
        )

    def __init__(
        self,
        type: bytes,
        version: int,
        atime_sec: int = 0,
        atime_nsec: int = 0,
        ctime_sec: int = 0,
        ctime_nsec: int = 0,
        mtime_sec: int = 0,
        mtime_nsec: int = 0,
        padding: bytes = b"\0\0\0\0\0\0\0\0",
    ) -> None:
        self.type = type
        self.version = version
        self.atime_sec = atime_sec
        self.atime_nsec = atime_nsec
        self.ctime_sec = ctime_sec
        self.ctime_nsec = ctime_nsec
        self.mtime_sec = mtime_sec
        self.mtime_nsec = mtime_nsec
        self.padding = padding

    @property
    def atime(self) -> float:
        return self.atime_sec + (self.atime_nsec / 1000000000.0)

    @atime.setter
    def atime(self, value: float) -> None:
        self.atime_sec = int(value)
        self.atime_nsec = int((value - self.atime_sec) * 1000000000)

    @property
    def ctime(self) -> float:
        return self.ctime_sec + (self.ctime_nsec / 1000000000.0)

    @ctime.setter
    def ctime(self, value: float) -> None:
        self.ctime_sec = int(value)
        self.ctime_nsec = int((value - self.ctime_sec) * 1000000000)

    @property
    def mtime(self) -> float:
        return self.mtime_sec + (self.mtime_nsec / 1000000000.0)

    @mtime.setter
    def mtime(self, value: float) -> None:
        self.mtime_sec = int(value)
        self.mtime_nsec = int((value - self.mtime_sec) * 1000000000)

    def serialize(self) -> bytes:
        return struct.pack(
            self.STRUCT_FORMAT,
            self.type,
            self.version,
            self.atime_sec,
            self.atime_nsec,
            self.ctime_sec,
            self.ctime_nsec,
            self.mtime_sec,
            self.mtime_nsec,
            self.padding,
        )


class Overlay:
    ROOT_INODE_NUMBER = 1
    NEXT_INODE_NUMBER_PATH = "next-inode-number"

    def __init__(self, path: str) -> None:
        self.path = path

    @contextlib.contextmanager
    def try_lock(self) -> Iterator[bool]:
        info_path = os.path.join(self.path, "info")
        try:
            lock_file = open(info_path, "rb")
        except OSError:
            yield False
            return

        try:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            yield True
        except OSError:
            yield False
        finally:
            # Release the lock once the yield returns
            lock_file.close()

    def get_path(self, inode_number: int) -> str:
        dir_name = "{:02x}".format(inode_number % 256)
        return os.path.join(self.path, dir_name, str(inode_number))

    def open_overlay_file(self, inode_number: int) -> BinaryIO:
        try:
            return typing.cast(BinaryIO, open(self.get_path(inode_number), "rb"))
        except OSError as ex:
            if ex.errno == errno.ENOENT:
                raise NoSuchOverlayFile(inode_number)
            raise

    def read_header(self, f: BinaryIO) -> OverlayHeader:
        data = f.read(OverlayHeader.LENGTH)
        return OverlayHeader.parse(data)

    def check_header(
        self, f: BinaryIO, inode_number: int, expected_type: bytes
    ) -> OverlayHeader:
        data = f.read(OverlayHeader.LENGTH)
        header = OverlayHeader.parse(data)
        if header.type != expected_type:
            raise InvalidOverlayFile(
                f"unexpected type for inode {inode_number} in overlay: "
                f"expected {expected_type!r} but found {header.type!r}"
            )
        return header

    def read_dir_inode(self, inode_number: int) -> OverlayDir:
        return self.read_dir_inode_tuple(inode_number)[1]

    def read_dir_inode_tuple(
        self, inode_number: int
    ) -> Tuple[OverlayHeader, OverlayDir]:
        with self.open_overlay_file(inode_number) as f:
            header = self.check_header(f, inode_number, OverlayHeader.TYPE_DIR)
            data = f.read()

        return (header, self.parse_dir_inode_data(data))

    def parse_dir_inode_data(self, data: bytes) -> OverlayDir:
        from thrift.util import Serializer
        from thrift.protocol import TCompactProtocol

        # Initialize entries to the empty dictionary.
        # This value will be used if the serialized data does not have any value
        # for this field.
        tree_data = OverlayDir(entries={})
        protocol_factory = TCompactProtocol.TCompactProtocolFactory()
        Serializer.deserialize(protocol_factory, data, tree_data)
        return tree_data

    def open_file_inode(self, inode_number: int) -> BinaryIO:
        return self.open_file_inode_tuple(inode_number)[1]

    def open_file_inode_tuple(
        self, inode_number: int
    ) -> Tuple[OverlayHeader, BinaryIO]:
        """Open the overlay file for the specified inode number.

        Returns the header information and a file object opened to the start of the
        file inode contents.
        """
        f = self.open_overlay_file(inode_number)
        try:
            header = self.check_header(f, inode_number, OverlayHeader.TYPE_FILE)
        except Exception:
            f.close()
            raise
        return (header, f)

    def lookup_path(self, path: Path) -> Optional[int]:
        """
        Lookup a path in the overlay.

        Returns the inode number corresponding to the path, if the path is materialized.

        - If an inode number is found for this path, returns the inode number.
        - If one of the parent directories is not materialized, returns None.
          Without checking the source control data we cannot tell if this logical path
          exists or not.
        - If this path or one of its parent directories does not exist throws an
          InodeLookupError

        May throw other exceptions on error.
        """
        assert not path.is_absolute()
        if not path.parts:
            return self.ROOT_INODE_NUMBER

        parent_inode_number = self.ROOT_INODE_NUMBER
        index = 0
        while True:
            parent_dir = self.read_dir_inode(parent_inode_number)
            desired = path.parts[index]
            index += 1

            entries = [] if parent_dir.entries is None else parent_dir.entries.items()
            entry: Optional[OverlayEntry] = None
            for name, entry in entries:  # noqa: ignore=B007
                if name == desired:
                    break

            if entry is None:
                raise InodeLookupError(f"{path} does not exist", errno.ENOENT)

            if index >= len(path.parts):
                return entry.inodeNumber

            if entry.mode is None or stat.S_IFMT(entry.mode) != stat.S_IFDIR:
                non_dir_path = os.path.sep.join(path.parts[:index])
                raise InodeLookupError(
                    f"error looking up {path}: {non_dir_path} is not a directory",
                    errno.ENOTDIR,
                )
            if entry.hash:
                # This directory along the chain is not materialized
                return None

            parent_inode_number = entry.inodeNumber

    def extract_file(
        self, inode_number: int, output_path: Path, mode: int, remove: bool = False
    ) -> None:
        """Copy the specified file inode out of the overlay.

        If remove=True the data for this inode will be removed from the overlay after it
        has been extracted.
        """
        with self.open_overlay_file(inode_number) as inf:
            header = self.read_header(inf)
            if header.type != OverlayHeader.TYPE_FILE:
                raise Exception(
                    f"expected inode {inode_number} to be a regular file; "
                    f"found unexpected type {header.type!r}"
                )

            output_path.parent.mkdir(parents=True, exist_ok=True)
            file_type = stat.S_IFMT(mode)
            if file_type == stat.S_IFLNK:
                contents = inf.read()
                os.symlink(contents, bytes(output_path))
            elif file_type == stat.S_IFREG:
                with output_path.open("wb") as outf:
                    shutil.copyfileobj(inf, outf)  # type: ignore
                    # Note: the file permissions bits are now stored in the inode table
                    # rather than the overlay.  The mode bits in the overlay will
                    # reflect the correct file type only.  Always extract orphan inodes
                    # with permissions 0o600 (read+write to owner only).
                    os.fchmod(outf.fileno(), 0o600)
            else:
                # We don't copy out sockets, fifos, or other unusual file types.
                # These shouldn't have any actual file contents anyway.
                logging.debug(
                    f"skipping inode {inode_number} at {output_path} with "
                    f"unsupported file type {file_type:#o}"
                )

        path = Path(self.get_path(inode_number))
        path.unlink()

    def extract_dir(
        self, inode_number: int, output_path: Path, remove: bool = False
    ) -> None:
        """Recursively copy the specified directory inode out of the overlay.

        All of its materialized children will be copied out.  Children that still have
        the same contents as a committed source control object will not be copied out.

        If remove=True the data for the extracted inodes will be removed from the
        overlay after they have been extracted.
        """
        data = self.read_dir_inode(inode_number)
        for name, entry in data.entries.items():
            overlay_path = Path(self.get_path(entry.inodeNumber))
            if not overlay_path.exists():
                # Skip children that do not exist in the overlay.
                # Note that we explicitly check for existence of the child even if
                # entry.hash is set (i.e., if the inode is not materialized):
                #
                # - Non-materialized directories can have data in the overlay if they
                #   contain allocated inode numbers.  We still recurse into the
                #   directory in this case.  This makes sure we remove the overlay files
                #   when remove=True, and also ensures that we will find any
                #   materialized file data inside this subdirectory if Eden crashed in
                #   the middle of trying to materialize a file but before it marked the
                #   parent directories materialized.
                # - Even for files we can have the same race on crash: eden may have
                #   crashed while materializing a file before it could mark the parent
                #   directories materialized.  (In theory the file contents should still
                #   be the same as the source control state in this case, but it seems
                #   better to err on the safe side and extract it anyway.)
                continue

            entry_output_path = output_path.joinpath(name)
            file_type = stat.S_IFMT(entry.mode)
            if file_type == stat.S_IFDIR:
                self.extract_dir(entry.inodeNumber, entry_output_path, remove=remove)
            else:
                self.extract_file(
                    entry.inodeNumber, entry_output_path, entry.mode, remove=remove
                )

        if remove:
            path = Path(self.get_path(inode_number))
            path.unlink()

    def write_empty_file(self, inode_number: int) -> None:
        self._write_inode(inode_number, OverlayHeader.TYPE_FILE, b"")

    def write_empty_dir(self, inode_number: int) -> None:
        from thrift.util import Serializer
        from thrift.protocol import TCompactProtocol

        empty_tree = OverlayDir()
        protocol_factory = TCompactProtocol.TCompactProtocolFactory()
        contents = typing.cast(
            bytes, Serializer.serialize(protocol_factory, empty_tree)
        )

        self._write_inode(inode_number, OverlayHeader.TYPE_DIR, contents)

    def _write_inode(self, inode_number: int, header_type: bytes, body: bytes) -> None:
        now_sec = int(time.time())
        header = OverlayHeader(
            type=header_type,
            version=OverlayHeader.VERSION_1,
            atime_sec=now_sec,
            mtime_sec=now_sec,
            ctime_sec=now_sec,
        )

        header_data = header.serialize()
        path = Path(self.get_path(inode_number))
        path.write_bytes(header_data + body)

    def read_next_inode_number(self) -> Optional[int]:
        file_path = os.path.join(self.path, self.NEXT_INODE_NUMBER_PATH)
        try:
            with open(file_path, "rb") as f:
                data = f.read()
        except FileNotFoundError:
            return None

        if len(data) != 8:
            raise Exception(
                f"invalid data in {self.NEXT_INODE_NUMBER_PATH} file: "
                f"expected file to contain 8 bytes, but is {len(data)} "
                f"bytes"
            )
        return struct.unpack("@Q", data)[0]

    def write_next_inode_number(self, next_inode: int) -> None:
        contents = struct.pack("@Q", next_inode)
        file_path = os.path.join(self.path, self.NEXT_INODE_NUMBER_PATH)
        fd, tmp_path = tempfile.mkstemp(
            prefix=self.NEXT_INODE_NUMBER_PATH, dir=self.path
        )
        try:
            os.write(fd, contents)
            os.fdatasync(fd)
            os.fchmod(fd, 0o644)
            os.rename(tmp_path, file_path)
        except Exception:
            try:
                os.unlink(tmp_path)
            except Exception:
                pass
            raise
