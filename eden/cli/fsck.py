#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import enum
import logging
import os
import stat
import sys
import time
import types
from pathlib import Path
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

    def compute_path(self, parent: "InodeInfo") -> str:
        if parent.inode_number == overlay_mod.Overlay.ROOT_INODE_NUMBER:
            return self.name
        return parent.compute_path() + os.path.sep + self.name


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
            return f"[unlinked({self.inode_number})]"

        parent, child_entry = self.parents[0]
        return child_entry.compute_path(parent)


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

    def repair(
        self, log: logging.Logger, overlay: overlay_mod.Overlay, fsck_dir: Path
    ) -> bool:
        log.info("no automatic remediation available for this error")
        return False


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
            f"{self.child.inode_number} ({self.compute_path()}) "
            f"with file mode {self.child.mode:#o}"
        )

    def compute_path(self) -> str:
        return self.child.compute_path(self.inode)

    def repair(
        self, log: logging.Logger, overlay: overlay_mod.Overlay, fsck_dir: Path
    ) -> bool:
        # TODO: It would be nice to try and get the contents of the
        # file/directory at this location in the current commit, rather than
        # just writing out an empty file or directory
        if stat.S_ISDIR(self.child.mode):
            log.info(
                f"replacing missing directory {self.compute_path()!r} with an "
                "empty directory"
            )
            overlay.write_empty_dir(self.child.inode_number)
        else:
            log.info(
                f"replacing missing file {self.compute_path()!r} with an empty file"
            )
            overlay.write_empty_file(self.child.inode_number)
        return True


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
            f"{self.inode.inode_number} ({self.compute_path()}){mtime_str}: "
            f"{self.inode.error}"
        )

    def compute_path(self) -> str:
        return self.inode.compute_path()

    def repair(
        self, log: logging.Logger, overlay: overlay_mod.Overlay, fsck_dir: Path
    ) -> bool:
        # TODO: It would be nice to try and get the contents of the
        # file/directory at this location in the current commit, rather than
        # just writing out an empty file or directory

        backup_dir = fsck_dir / "broken_inodes"
        backup_dir.mkdir(exist_ok=True)
        inode_data_path = Path(overlay.get_path(self.inode.inode_number))
        inode_backup_path = backup_dir / str(self.inode.inode_number)

        if self.expected_type == InodeType.DIR:
            log.info(
                f"replacing corrupt directory inode {self.compute_path()!r} with an "
                "empty directory"
            )
            os.rename(inode_data_path, inode_backup_path)
            overlay.write_empty_dir(self.inode.inode_number)
        else:
            log.info(
                f"replacing corrupt file inode {self.compute_path()!r} with an "
                "empty file"
            )
            os.rename(inode_data_path, inode_backup_path)
            overlay.write_empty_file(self.inode.inode_number)

        return True


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
            for inode in inode_list:
                mtime_str = _get_mtime_str(inode.mtime)
                entries.append(f"  {inode.inode_number}{mtime_str}")

        return "\n".join(entries)

    def repair(
        self, log: logging.Logger, overlay: overlay_mod.Overlay, fsck_dir: Path
    ) -> bool:
        lost_n_found = fsck_dir / "lost+found"
        lost_n_found.mkdir(exist_ok=True)
        log.info(f"moving orphan inodes to {lost_n_found}")

        for inode in self.orphan_directories:
            log.info(
                f"moving contents of orphan directory {inode.inode_number} "
                f"to lost+found"
            )
            inode_lnf_path = lost_n_found / str(inode.inode_number)
            overlay.extract_dir(inode.inode_number, inode_lnf_path, remove=True)

        file_mode = stat.S_IFREG | 0o644
        for inode in self.orphan_files:
            log.info(f"moving orphan file {inode.inode_number} to lost+found")
            inode_lnf_path = lost_n_found / str(inode.inode_number)
            overlay.extract_file(
                inode.inode_number, inode_lnf_path, file_mode, remove=True
            )

        return True


class HardLinkedInode(Error):
    def __init__(self, inode: InodeInfo) -> None:
        super().__init__(ErrorLevel.WARNING)
        self.inode = inode

    def __str__(self) -> str:
        paths = [
            child_info.compute_path(parent) for parent, child_info in self.inode.parents
        ]
        return f"inode {self.inode.inode_number} exists in multiple locations: {paths}"


class MissingNextInodeNumber(Error):
    def __init__(self, next_inode_number: int) -> None:
        super().__init__(ErrorLevel.WARNING)
        self.next_inode_number = next_inode_number

    def __str__(self) -> str:
        # Eden deletes the next-inode-number while the checkout is mounted,
        # so if it is missing it just means that the checkout wasn't cleanly unmounted.
        # This is pretty common in situations where the user is running fsck...
        return f"edenfs appears to have been shut down uncleanly"

    def repair(
        self, log: logging.Logger, overlay: overlay_mod.Overlay, fsck_dir: Path
    ) -> bool:
        log.info(f"setting max inode number data to {self.next_inode_number}")
        overlay.write_next_inode_number(self.next_inode_number)
        return True


class BadNextInodeNumber(Error):
    def __init__(self, read_next_inode_number: int, next_inode_number: int) -> None:
        super().__init__(ErrorLevel.ERROR)
        self.read_next_inode_number = read_next_inode_number
        self.next_inode_number = next_inode_number

    def __str__(self) -> str:
        return (
            f"bad stored next inode number: read {self.read_next_inode_number} "
            f"but should be at least {self.next_inode_number}"
        )

    def repair(
        self, log: logging.Logger, overlay: overlay_mod.Overlay, fsck_dir: Path
    ) -> bool:
        log.info(f"replacing max inode number data with {self.next_inode_number}")
        overlay.write_next_inode_number(self.next_inode_number)
        return True


class CorruptNextInodeNumber(Error):
    def __init__(self, ex: Exception, next_inode_number: int) -> None:
        super().__init__(ErrorLevel.WARNING)
        self.error = ex
        self.next_inode_number = next_inode_number

    def __str__(self) -> str:
        return f"stored next-inode-number file is corrupt: {self.error}"

    def repair(
        self, log: logging.Logger, overlay: overlay_mod.Overlay, fsck_dir: Path
    ) -> bool:
        log.info(f"replacing max inode number data with {self.next_inode_number}")
        overlay.write_next_inode_number(self.next_inode_number)
        return True


class FilesystemChecker:
    def __init__(self, checkout_state_dir: Path) -> None:
        self._state_dir = checkout_state_dir
        self.overlay = overlay_mod.Overlay(str(checkout_state_dir / "local"))
        self.errors: List[Error] = []
        self._overlay_locked: Optional[bool] = None
        self._overlay_lock: Optional[ContextManager[bool]] = None
        self._orphan_inodes: List[InodeInfo] = []
        self._max_inode_number = 0

    def __enter__(self) -> "FilesystemChecker":
        self._overlay_lock = self.overlay.try_lock()
        # pyre-fixme[16]: `Optional` has no attribute `__enter__`.
        self._overlay_locked = self._overlay_lock.__enter__()
        return self

    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_value: Optional[BaseException],
        exc_traceback: Optional[types.TracebackType],
    ) -> Optional[bool]:
        assert self._overlay_lock is not None
        # pyre-fixme[16]: `Optional` has no attribute `__exit__`.
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

        expected_next_inode_number = self._max_inode_number + 1
        try:
            read_next_inode_number = self.overlay.read_next_inode_number()
            if read_next_inode_number is None:
                if self._overlay_locked:
                    self._add_error(MissingNextInodeNumber(expected_next_inode_number))
                else:
                    # If we couldn't get the overlay lock then Eden is probably still
                    # running, so it's normal that the max inode number file does not
                    # exist.
                    pass
            elif read_next_inode_number < expected_next_inode_number:
                self._add_error(
                    BadNextInodeNumber(
                        read_next_inode_number, expected_next_inode_number
                    )
                )
        except Exception as ex:
            self._add_error(CorruptNextInodeNumber(ex, expected_next_inode_number))

    def _read_inodes(self) -> Dict[int, InodeInfo]:
        inodes: Dict[int, InodeInfo] = {}

        for subdir_num in range(256):
            dir_name = "{:02x}".format(subdir_num)
            dir_path = os.path.join(self.overlay.path, dir_name)

            # TODO: Handle the error if os.listdir() fails
            for entry in os.listdir(dir_path):
                try:
                    inode_number = int(entry, 10)
                except ValueError:
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

    def _update_max_inode_number(self, inode_number: int) -> None:
        if inode_number > self._max_inode_number:
            self._max_inode_number = inode_number

    def _load_inode_info(self, inode_number: int) -> InodeInfo:
        self._update_max_inode_number(inode_number)
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
                if entry.inodeNumber:
                    self._update_max_inode_number(entry.inodeNumber)
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

    def fix_errors(self, fsck_dir: Optional[Path] = None) -> Optional[Path]:
        """Fix errors found by a previous call to scan_for_errors().

        Returns the path to the directory containing the fsck log and backups of
        corrupted & orphan inodes.

        Returns None if there were no errors to fix.
        """
        if not self._overlay_locked:
            raise Exception("cannot repair errors without holding the overlay lock")

        if not self.errors:
            return None

        # Create a directory to store logs and any data backups we may
        # create while trying to perform repairs
        if not fsck_dir:
            fsck_dir = _create_fsck_dir(self._state_dir)

        print(f"Beginning repairs.  Putting logs and backup data in {fsck_dir}")

        # Create a log file
        log_path = fsck_dir / "fsck.log"
        log = logging.getLogger("eden.fsck.repair_log")
        log.propagate = False
        log.setLevel(logging.DEBUG)
        log_file_handler = logging.FileHandler(log_path)
        log_file_handler.setFormatter(logging.Formatter("%(asctime)s %(message)s"))

        # ui_log logs both to the log file and to stdout
        ui_log = logging.getLogger("eden.fsck.repair_log.ui")
        ui_handler = logging.StreamHandler(sys.stdout)
        ui_handler.setFormatter(logging.Formatter("%(message)s"))

        log.addHandler(log_file_handler)
        ui_log.addHandler(ui_handler)
        try:
            num_fixed = 0
            try:
                log.info("Beginning fsck repair run")
                log.info(f"{len(self.errors)} issues were detected")

                for error in self.errors:
                    if self._fix_error(fsck_dir, ui_log, error):
                        num_fixed += 1
            except Exception as ex:
                log.exception(f"unhandled error: {ex}")
                raise

            ui_log.info(f"Fixed {num_fixed} of {len(self.errors)} issues")
        finally:
            # Remove the log handlers we added.
            # Logger objects are global, so otherwise the handlers will persist and
            # continue to be used for subsequent calls to fix_errors(), even if it is on
            # a different FilesystemChecker object.  This is mainly just an issue for
            # the fsck tests, which can call fix_errors() multiple times.
            ui_log.removeHandler(ui_handler)
            log.removeHandler(log_file_handler)
            log_file_handler.close()

        return fsck_dir

    def _fix_error(self, fsck_dir: Path, log: logging.Logger, error: Error) -> bool:
        log.info(f"Processing error: {error}")
        detail = error.detailed_description()
        if detail:
            log.debug(detail)
        return error.repair(log=log, overlay=self.overlay, fsck_dir=fsck_dir)


def _get_mtime_str(mtime: Optional[float]) -> str:
    if mtime is None:
        return ""
    return f", with mtime {time.ctime(mtime)}"


def _create_fsck_dir(state_dir: Path) -> Path:
    fsck_base_dir = state_dir / "fsck"
    fsck_base_dir.mkdir(exist_ok=True)
    timestamp_str = time.strftime("%Y%m%d_%H%M%S")

    # There probably shouldn't be multiple directories for the same second
    # normally, but just in case support adding a numeric suffix number to make the
    # path unique.
    for n in range(20):
        if n == 0:
            fsck_run_dir = fsck_base_dir / timestamp_str
        else:
            fsck_run_dir = fsck_base_dir / f"{timestamp_str}.{n}"
        try:
            fsck_run_dir.mkdir()
            return fsck_run_dir
        except FileExistsError:
            continue

    # We set a limit on the above loop just to guard against infinite loops if we
    # have any sort of programming bug.
    # This code path probably shouldn't be hit in normal circumstances.
    raise Exception("too many fsck run directories for the current time")
