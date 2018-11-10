#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import contextlib
import enum
import os
import stat
import time
import types
from typing import ContextManager, Dict, List, NamedTuple, Optional, Tuple, Type

from . import overlay as overlay_mod


class InodeType(enum.Enum):
    FILE = enum.auto()
    DIR = enum.auto()
    ERROR = enum.auto()
    DIR_ERROR = enum.auto()


class ChildInfo(NamedTuple):
    inode_number: int
    name: str
    mode: int
    hash: Optional[bytes]


class InodeInfo:
    __slots__ = ["inode_number", "type", "parents", "children", "mtime", "error"]

    def __init__(
        self,
        inode_number: int,
        type: InodeType,
        children: List[ChildInfo],
        mtime: Optional[float],
        error: Optional[Exception],
    ) -> None:
        self.inode_number = inode_number
        self.type = type

        # The mtime is the modification time on the overlay file itself, not the
        # value for the logical file represented by this overlay entry.
        # This is mainly present for helping identify when a problem was introduced in
        # the overlay.
        self.mtime = mtime

        self.error = error

        # The other inode(s) that list this as a child
        self.parents: List[Tuple[InodeInfo, ChildInfo]] = []
        self.children = children

    def compute_path(self) -> str:
        if not self.parents:
            if self.inode_number == overlay_mod.Overlay.ROOT_INODE_NUMBER:
                return "/"
            return "[unlinked]"

        parent, child_entry = self.parents[0]
        if parent.inode_number == overlay_mod.Overlay.ROOT_INODE_NUMBER:
            return child_entry.name
        return parent.compute_path() + os.path.sep + child_entry.name


class ErrorLevel(enum.IntEnum):
    # WARNING is for issues that do not affect our ability to show file contents
    # correctly.
    WARNING = 1

    # ERROR issues are problems that prevent us from being able to read file or
    # directory contents.
    ERROR = 2

    @staticmethod
    def get_label(level: int) -> str:
        if level == ErrorLevel.WARNING:
            return "warning"
        return "error"


class Error:
    def __init__(self, level: ErrorLevel) -> None:
        self.level = level

    def detailed_description(self) -> Optional[str]:
        return None


class UnexpectedOverlayFile(Error):
    def __init__(self, path: str) -> None:
        super().__init__(ErrorLevel.WARNING)
        self.path = path
        self.mtime = None
        with contextlib.suppress(OSError):
            self.mtime = os.lstat(path).st_mtime

    def __str__(self) -> str:
        mtime_str = _get_mtime_str(self.mtime)
        return f"unexpected file present in overlay: {self.path}{mtime_str}"


class MissingMaterializedInode(Error):
    def __init__(self, inode: InodeInfo, child: ChildInfo) -> None:
        super().__init__(ErrorLevel.ERROR)
        self.inode = inode
        self.child = child

    def __str__(self) -> str:
        if stat.S_ISDIR(self.child.mode):
            file_type = "directory"
        elif stat.S_ISLNK(self.child.mode):
            file_type = "symlink"
        else:
            file_type = "file"
        return (
            f"missing overlay file for materialized {file_type} inode "
            f"{self.child.inode_number} "
            f"({self.inode.compute_path()}/{self.child.name}) "
            f"with file mode {self.child.mode:#o}"
        )


class InvalidMaterializedInode(Error):
    def __init__(self, inode: InodeInfo) -> None:
        super().__init__(ErrorLevel.ERROR)
        self.inode = inode
        self.expected_type = self._compute_expected_type()

    def _compute_expected_type(self) -> Optional[InodeType]:
        # Look at the parents to see if this looks like it should be a file or directory
        if self.inode.parents:
            _parent_inode, child_entry = self.inode.parents[0]
            if stat.S_ISDIR(child_entry.mode):
                return InodeType.DIR
            else:
                return InodeType.FILE
        elif self.inode.type == InodeType.DIR_ERROR:
            return InodeType.DIR
        return None

    def __str__(self) -> str:
        if self.expected_type is None:
            type_str = "inode"
        elif self.expected_type == InodeType.DIR:
            type_str = "directory inode"
        else:
            type_str = "file inode"

        mtime_str = _get_mtime_str(self.inode.mtime)
        return (
            f"invalid overlay file for materialized {type_str} "
            f"{self.inode.inode_number} ({self.inode.compute_path()}){mtime_str}: "
            f"{self.inode.error}"
        )


class OrphanInodes(Error):
    def __init__(self, inodes: List[InodeInfo]) -> None:
        super().__init__(ErrorLevel.WARNING)

        self.orphan_directories: List[InodeInfo] = []
        self.orphan_files: List[InodeInfo] = []

        for inode in inodes:
            if inode.type == InodeType.DIR:
                self.orphan_directories.append(inode)
            else:
                self.orphan_files.append(inode)

    def __str__(self) -> str:
        if self.orphan_directories and self.orphan_files:
            return (
                f"found {len(self.orphan_directories)} orphan directory inodes and "
                f"{len(self.orphan_files)} orphan file inodes"
            )
        elif self.orphan_directories:
            return f"found {len(self.orphan_directories)} orphan directory inodes"
        else:
            return f"found {len(self.orphan_files)} orphan file inodes"

    def detailed_description(self) -> Optional[str]:
        entries = []

        for type, inode_list in (
            ("directory", self.orphan_directories),
            ("file", self.orphan_files),
        ):
            if not inode_list:
                continue
            entries.append(f"Orphan {type} inodes")
            for inode in self.orphan_directories:
                mtime_str = _get_mtime_str(inode.mtime)
                entries.append(f"  {inode.inode_number}{mtime_str}")

        return "\n".join(entries)


class HardLinkedInode(Error):
    def __init__(self, inode: InodeInfo) -> None:
        super().__init__(ErrorLevel.WARNING)
        self.inode = inode


class FilesystemChecker:
    def __init__(self, overlay: overlay_mod.Overlay) -> None:
        self.overlay = overlay
        self.errors: List[Error] = []
        self._overlay_locked: Optional[bool] = None
        self._overlay_lock: Optional[ContextManager[bool]] = None
        self._orphan_inodes: List[InodeInfo] = []

    def __enter__(self) -> "FilesystemChecker":
        self._overlay_lock = self.overlay.try_lock()
        self._overlay_locked = self._overlay_lock.__enter__()
        return self

    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_value: Optional[BaseException],
        exc_traceback: Optional[types.TracebackType],
    ) -> Optional[bool]:
        assert self._overlay_lock is not None
        return self._overlay_lock.__exit__(exc_type, exc_value, exc_traceback)

    def _add_error(self, error: Error) -> None:
        # Note that _add_error() may be called before all inode relationships
        # have been computed.  If we try printing errors here some inode paths may
        # incorrectly be printed as "unlinked" if we haven't finished setting their
        # parent yet.
        self.errors.append(error)

    def scan_for_errors(self) -> None:
        print("Reading materialized inodes...")
        inodes = self._read_inodes()

        print(f"Found {len(inodes)} materialized inodes")
        print(f"Computing directory relationships...")
        self._link_inode_children(inodes)

        print(f"Scanning for inconsistencies...")
        self._scan_inodes_for_errors(inodes)

        if self._orphan_inodes:
            self._add_error(OrphanInodes(self._orphan_inodes))

        # TODO: Check that the stored max inode number is valid

    def _read_inodes(self) -> Dict[int, InodeInfo]:
        inodes: Dict[int, InodeInfo] = {}

        for subdir_num in range(256):
            dir_name = "{:02x}".format(subdir_num)
            dir_path = os.path.join(self.overlay.path, dir_name)

            # TODO: Handle the error if os.listdir() fails
            for entry in os.listdir(dir_path):
                try:
                    inode_number = int(entry, 10)
                except ValueError as ex:
                    entry_path = os.path.join(dir_path, entry)
                    self._add_error(UnexpectedOverlayFile(entry_path))
                    continue

                # TODO: check if inode_number is actually in the correct subdirectory.
                # Handle the error if it is in the wrong directory, and if we found
                # multiple files with the same inode number in different subdirectories

                inode_info = self._load_inode_info(inode_number)
                inodes[inode_number] = inode_info

        return inodes

    def _link_inode_children(self, inodes: Dict[int, InodeInfo]) -> None:
        for inode in inodes.values():
            for child_info in inode.children:
                if child_info.inode_number == 0:
                    # Older versions of edenfs would leave the inode number set to 0
                    # if the child inode has never been loaded.  The child can't be
                    # present in the overlay if it doesn't have an inode number
                    # allocated for it yet.
                    #
                    # Newer versions of edenfs always allocate an inode number for all
                    # children, even if they haven't been loaded yet.
                    continue
                child_inode = inodes.get(child_info.inode_number, None)
                if child_inode is None:
                    if child_info.hash is None:
                        # This child is materialized (since it doesn't have a hash
                        # linking it to a source control object).  It's a problem if the
                        # materialized data isn't actually present in the overlay.
                        self._add_error(MissingMaterializedInode(inode, child_info))
                else:
                    child_inode.parents.append((inode, child_info))

    def _scan_inodes_for_errors(self, inodes: Dict[int, InodeInfo]) -> None:
        for inode in inodes.values():
            if inode.type in (InodeType.ERROR, InodeType.DIR_ERROR):
                self._add_error(InvalidMaterializedInode(inode))

            num_parents = len(inode.parents)
            if (
                num_parents == 0
                and inode.inode_number != overlay_mod.Overlay.ROOT_INODE_NUMBER
            ):
                self._orphan_inodes.append(inode)
            elif num_parents > 1:
                self._add_error(HardLinkedInode(inode))

    def _load_inode_info(self, inode_number: int) -> InodeInfo:
        dir_data = None
        stat_info = None
        error = None
        try:
            with self.overlay.open_overlay_file(inode_number) as f:
                stat_info = os.fstat(f.fileno())
                header = self.overlay.read_header(f)
                if header.type == overlay_mod.OverlayHeader.TYPE_DIR:
                    dir_data = f.read()
                    type = InodeType.DIR
                elif header.type == overlay_mod.OverlayHeader.TYPE_FILE:
                    type = InodeType.FILE
                else:
                    type = InodeType.ERROR
        except Exception as ex:
            # If anything goes wrong trying to open or parse the overlay file
            # report this as an error, regardless of what type of error it is.
            type = InodeType.ERROR
            error = ex

        dir_entries = None
        children: List[ChildInfo] = []
        if dir_data is not None:
            try:
                parsed_data = self.overlay.parse_dir_inode_data(dir_data)
                dir_entries = parsed_data.entries
            except Exception as ex:
                type = InodeType.DIR_ERROR
                error = ex

        if dir_entries is not None:
            for name, entry in dir_entries.items():
                children.append(
                    ChildInfo(
                        inode_number=entry.inodeNumber or 0,
                        name=name,
                        mode=entry.mode,
                        hash=entry.hash,
                    )
                )

        mtime = None
        if stat_info is not None:
            mtime = stat_info.st_mtime
        return InodeInfo(inode_number, type, children, mtime, error)


def _get_mtime_str(mtime: Optional[float]) -> str:
    if mtime is None:
        return ""
    return f", with mtime {time.ctime(mtime)}"
