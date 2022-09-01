#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import hashlib
import os
import platform
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Callable, List, Set, Tuple

from eden.fs.cli.config import EdenCheckout, EdenInstance
from eden.fs.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker
from eden.fs.cli.doctor.util import CheckoutInfo
from eden.fs.cli.filesystem import FsUtil
from eden.fs.cli.prjfs import PRJ_FILE_STATE
from eden.thrift.legacy import EdenClient
from facebook.eden.constants import DIS_REQUIRE_LOADED, DIS_REQUIRE_MATERIALIZED
from facebook.eden.ttypes import SyncBehavior


def check_using_nfs_path(tracker: ProblemTracker, mount_path: Path) -> None:
    check_shared_path(tracker, mount_path)


class StateDirOnNFS(Problem):
    def __init__(self, instance: EdenInstance) -> None:
        msg = (
            f"Eden's state directory is on an NFS file system: {instance.state_dir}\n"
            f"  This will likely cause performance problems and/or other errors."
        )

        # On FB devservers the default EdenFS state directory path is ~/local/.eden
        # Normally ~/local is expected to be a symlink to local disk (for users who are
        # still using NFS home directories in the first place).  The most common cause of
        # the EdenFS state directory being on NFS is for users that somehow have a regular
        # directory at ~/local rather than a symlink.  Suggest checking this as a
        # remediation.
        remediation = (
            "The most common cause for this is if your ~/local symlink does not point "
            "to local disk.  Make sure that ~/local is a symlink pointing to local disk "
            "and then run `eden restart`."
        )
        super().__init__(msg, remediation)


def check_eden_directory(tracker: ProblemTracker, instance: EdenInstance) -> None:
    if not is_nfs_mounted(str(instance.state_dir)):
        return

    tracker.add_problem(StateDirOnNFS(instance))


def get_shared_path(mount_path: Path) -> Path:
    return mount_path / ".hg" / "sharedpath"


class UnreadableSharedpath(Problem):
    def __init__(self, e: Exception) -> None:
        super().__init__(f"Failed to read .hg/sharedpath: {e}")


def read_shared_path(tracker: ProblemTracker, shared_path: Path) -> str:
    try:
        return shared_path.read_text()
    except (FileNotFoundError, IsADirectoryError):
        raise
    except Exception as e:
        tracker.add_problem(UnreadableSharedpath(e))
        raise


class MercurialDataOnNFS(Problem):
    def __init__(self, shared_path: Path, dst_shared_path: str) -> None:
        msg = (
            f"The Mercurial data directory for {shared_path} is at"
            f" {dst_shared_path} which is on a NFS filesystem."
            f" Accessing files and directories in this repository will be slow."
        )
        super().__init__(msg, severity=ProblemSeverity.ADVICE)


def check_shared_path(tracker: ProblemTracker, mount_path: Path) -> None:
    shared_path = get_shared_path(mount_path)
    try:
        dst_shared_path = read_shared_path(tracker, shared_path)
    except Exception:
        return

    if is_nfs_mounted(dst_shared_path):
        tracker.add_problem(MercurialDataOnNFS(shared_path, dst_shared_path))


def fstype_for_path(path: str) -> str:
    if platform.system() == "Linux":
        try:
            args = ["stat", "-fc", "%T", "--", path]
            return subprocess.check_output(args).decode("ascii").strip()
        except subprocess.CalledProcessError:
            return "unknown"

    return "unknown"


def is_nfs_mounted(path: str) -> bool:
    return fstype_for_path(path) == "nfs"


def get_mountpt(path: str) -> str:
    if not os.path.exists(path):
        return path
    path = os.path.realpath(path)
    path_stat = os.lstat(path)
    while True:
        parent = os.path.dirname(path)
        parent_stat = os.lstat(parent)
        if parent == path or parent_stat.st_dev != path_stat.st_dev:
            return path
        path, path_stat = parent, parent_stat


def get_mount_pts_set(
    tracker: ProblemTracker, mount_paths: List[str], instance: EdenInstance
) -> Set[str]:
    eden_locations = [str(instance.state_dir), tempfile.gettempdir()]
    for mount_path in mount_paths:
        try:
            eden_repo_path = read_shared_path(
                tracker, get_shared_path(Path(mount_path))
            )
        except Exception:
            continue

        eden_locations.append(eden_repo_path)

        try:
            proc = subprocess.run(
                ["hg", "config", "remotefilelog.cachepath"],
                cwd=mount_path,
                env=dict(os.environ, HGPLAIN="1"),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
        except subprocess.CalledProcessError:
            # hg config may fail if the repo is corrupted.
            # We don't log any output about this here.
            # The check_hg() logic will detect and perform error handling for this case.
            continue

        hg_cache_dir = proc.stdout
        eden_locations.append(hg_cache_dir.decode("utf-8").rstrip("\n"))

    # Set is used to skip duplicate mount folders
    return {get_mountpt(eden_location) for eden_location in eden_locations}


class LowDiskSpace(Problem):
    def __init__(self, message: str, severity: ProblemSeverity) -> None:
        super().__init__(message, severity=severity)


def check_disk_usage(
    tracker: ProblemTracker,
    mount_paths: List[str],
    instance: EdenInstance,
    fs_util: FsUtil,
) -> None:
    prob_advice_space_used_ratio_threshold = 0.90
    prob_error_absolute_space_used_threshold = 1024 * 1024 * 1024  # 1GB

    eden_mount_pts_set = get_mount_pts_set(tracker, mount_paths, instance)

    for eden_mount_pt in eden_mount_pts_set:
        if eden_mount_pt and os.path.exists(eden_mount_pt):
            disk_usage = fs_util.disk_usage(eden_mount_pt)

            size = disk_usage.total
            if size == 0:
                continue

            avail = disk_usage.free
            used = disk_usage.used
            used_percent = float(used) / size

            message = (
                "EdenFS lazily loads your files and needs enough disk space to "
                "store these files when loaded."
            )
            extra_message = instance.get_config_value(
                "doctor.low-disk-space-message", ""
            )
            if extra_message:
                message = f"{message} {extra_message}"

            if avail <= prob_error_absolute_space_used_threshold:
                tracker.add_problem(
                    LowDiskSpace(
                        f"{eden_mount_pt} "
                        f"has only {str(avail)} bytes available. "
                        f"{message}",
                        severity=ProblemSeverity.ERROR,
                    )
                )
            elif used_percent >= prob_advice_space_used_ratio_threshold:
                tracker.add_problem(
                    LowDiskSpace(
                        f"{eden_mount_pt} "
                        f"is {used_percent:.2%} full. "
                        f"{message}",
                        severity=ProblemSeverity.ADVICE,
                    )
                )


def mode_to_str(mode: int) -> str:
    if stat.S_ISDIR(mode):
        return "directory"
    elif stat.S_ISREG(mode):
        return "file"
    elif stat.S_ISLNK(mode):
        return "symlink"
    else:
        return "unknown"


class MaterializedInodesHaveDifferentModeOnDisk(Problem):
    def __init__(self, errors: List[Tuple[Path, int, int]]) -> None:
        super().__init__(
            "\n".join(
                [
                    f"{error[0]} is known to EdenFS as a {mode_to_str(error[2])}, "
                    f"but is a {mode_to_str(error[1])} on disk"
                    for error in errors
                ]
            ),
            severity=ProblemSeverity.ERROR,
        )


class PathsProblem(Problem):
    @staticmethod
    def omitPathsDescription(paths: List[Path], pathSuffix: str) -> str:
        pathDescriptions = [str(path) + pathSuffix for path in paths[:10]]
        if len(paths) > 10:
            pathDescriptions.append("{len(paths) - 10} more paths omitted")
        return "\n".join(pathDescriptions)

    @staticmethod
    def omitPathsDescriptionWithException(
        paths: List[Tuple[Path, str]], pathSuffix: str
    ) -> str:
        pathDescriptions = [
            f"{path}{pathSuffix}: {error}" for path, error in paths[:10]
        ]
        if len(paths) > 10:
            pathDescriptions.append("{len(paths) - 10} more paths omitted")
        return "\n".join(pathDescriptions)


class MaterializedInodesAreInaccessible(PathsProblem):
    def __init__(self, paths: List[Tuple[Path, str]]) -> None:
        super().__init__(
            self.omitPathsDescriptionWithException(
                paths, " is inaccessible despite EdenFS believing it should be"
            ),
            severity=ProblemSeverity.ERROR,
        )


class MissingInodesForFiles(PathsProblem):
    def __init__(self, paths: List[Path]) -> None:
        super().__init__(
            self.omitPathsDescription(
                paths, " is not known to EdenFS but is accessible on disk"
            ),
            severity=ProblemSeverity.ERROR,
        )


class MissingFilesForInodes(PathsProblem):
    def __init__(self, paths: List[Path]) -> None:
        super().__init__(
            self.omitPathsDescription(
                paths, " is not present on disk despite EdenFS believing it should be"
            ),
            severity=ProblemSeverity.ERROR,
        )


class DuplicateInodes(PathsProblem):
    def __init__(self, paths: List[Path]) -> None:
        super().__init__(
            self.omitPathsDescription(paths, " is duplicated in EdenFS"),
            severity=ProblemSeverity.ERROR,
        )


def check_materialized_are_accessible(
    tracker: ProblemTracker,
    instance: EdenInstance,
    checkout: EdenCheckout,
    get_mode: Callable[[Path], int],
) -> None:
    # {path | path is a materialized directory or one of its entries whose mode does not match on the filesystem}
    mismatched_mode = []
    # {path | path is a materialized file or directory inside EdenFS, and can not be read on the filesystem}
    inaccessible_inodes = []
    # {path | path is a materialized file or directory inside EdenFS, and does not exist on the filesystem}
    nonexistent_inodes = []
    # {path | path is a child of a directory on disk where that directory is materialized inside EdenFS and the child does not exist inside EdenFS}
    missing_inodes = []
    # {path | path is a child of a directory that contains two children with the same name inside of EdenFS}
    # This generally always should be [], EdenFS directories should not be able to contain duplicates.
    duplicate_inodes = []

    with instance.get_thrift_client_legacy() as client:
        materialized = client.debugInodeStatus(
            bytes(checkout.path),
            b"",
            flags=DIS_REQUIRE_MATERIALIZED,
            sync=SyncBehavior(),
        )

    for materialized_dir in materialized:
        materialized_name = os.fsdecode(materialized_dir.path)
        path = Path(materialized_name)
        osPath = checkout.path / path
        try:
            mode = get_mode(osPath)
        except FileNotFoundError:
            nonexistent_inodes.append(path)
            continue
        except OSError as ex:
            inaccessible_inodes.append((path, str(ex)))
            continue

        if not stat.S_ISDIR(mode):
            mismatched_mode += [(path, stat.S_IFDIR, mode)]

        # A None missing_path_names avoids the listdir and missing inodes check
        missing_path_names = None
        # We will ignore special '.eden' checkout path
        if materialized_name != ".eden":
            missing_path_names = set(os.listdir(osPath))
        visited_path_names = set()

        for dirent in materialized_dir.entries:
            name = os.fsdecode(dirent.name)
            dirent_path = path / Path(name)
            if name in visited_path_names:
                duplicate_inodes.append(dirent_path)
                continue
            visited_path_names.add(name)
            if missing_path_names is not None:
                if name not in missing_path_names:
                    nonexistent_inodes.append(dirent_path)
                    continue
                missing_path_names.remove(name)
            if dirent.materialized:
                try:
                    dirent_mode = get_mode(checkout.path / dirent_path)
                except FileNotFoundError:
                    nonexistent_inodes.append(dirent_path)
                    continue
                except OSError as ex:
                    inaccessible_inodes.append((dirent_path, str(ex)))
                    continue

                # TODO(xavierd): Symlinks are for now recognized as files.
                dirent_mode = (
                    stat.S_IFREG
                    if stat.S_ISLNK(dirent_mode)
                    else stat.S_IFMT(dirent_mode)
                )
                if dirent_mode != stat.S_IFMT(dirent.mode):
                    mismatched_mode += [(dirent_path, dirent_mode, dirent.mode)]

        if missing_path_names:
            missing_inodes += [path / name for name in missing_path_names]

    if duplicate_inodes:
        tracker.add_problem(DuplicateInodes(duplicate_inodes))

    if missing_inodes:
        tracker.add_problem(MissingInodesForFiles(missing_inodes))

    if nonexistent_inodes:
        tracker.add_problem(MissingFilesForInodes(nonexistent_inodes))

    if inaccessible_inodes:
        tracker.add_problem(MaterializedInodesAreInaccessible(inaccessible_inodes))

    if mismatched_mode:
        tracker.add_problem(MaterializedInodesHaveDifferentModeOnDisk(mismatched_mode))


class LoadedFileHasDifferentContentOnDisk(Problem):
    def __init__(self, errors: List[Tuple[Path, bytes, bytes]]) -> None:
        super().__init__(
            "\n".join(
                [
                    f"The on-disk file at {error[0]} is out of sync from EdenFS. Expected SHA1: {error[1].hex()}, on-disk SHA1: {error[2].hex()}"
                    for error in errors
                ]
            ),
            severity=ProblemSeverity.ERROR,
        )


def _compute_file_sha1(path: Path) -> bytes:
    hasher = hashlib.sha1()
    with open(path, "rb") as f:
        while True:
            buf = f.read(1024 * 1024)
            if buf == b"":
                break
            hasher.update(buf)
    return hasher.digest()


def _validate_loaded_content(
    client: EdenClient,
    checkout_path: Path,
    query_prjfs_file: Callable[[Path], PRJ_FILE_STATE],
) -> Tuple[List[Tuple[Path, bytes, bytes]], List[Path]]:
    # {path | path is a child of a directory on disk where that directory is a loaded inode inside EdenFS and the child does not exist inside EdenFS}
    missing_inodes = []

    loaded = client.debugInodeStatus(
        bytes(checkout_path),
        b"",
        flags=DIS_REQUIRE_LOADED,
        sync=SyncBehavior(),
    )

    errors = []
    for loaded_dir in loaded:
        path = Path(os.fsdecode(loaded_dir.path))

        osPath = checkout_path / path
        missing_path_names = set()
        refcount = loaded_dir.refcount or 0
        if not loaded_dir.materialized and refcount > 0:
            missing_path_names = set(os.listdir(osPath))

        for dirent in loaded_dir.entries:
            name = os.fsdecode(dirent.name)
            if name in missing_path_names:
                missing_path_names.remove(name)
            if not stat.S_ISREG(dirent.mode) or dirent.materialized:
                continue

            dirent_path = path / Path(name)
            filestate = query_prjfs_file(checkout_path / dirent_path)
            if (
                filestate & PRJ_FILE_STATE.HydratedPlaceholder
            ) != PRJ_FILE_STATE.HydratedPlaceholder:
                # We should only compute the sha1 of files that have been read.
                continue

            sha1 = client.getSHA1(
                bytes(checkout_path), [bytes(dirent_path)], sync=SyncBehavior()
            )[0].get_sha1()
            on_disk_sha1 = _compute_file_sha1(checkout_path / dirent_path)
            if sha1 != on_disk_sha1:
                errors += [(dirent_path, sha1, on_disk_sha1)]

        missing_inodes += [path / name for name in missing_path_names]

    return errors, missing_inodes


def check_loaded_content(
    tracker: ProblemTracker,
    instance: EdenInstance,
    checkout: EdenCheckout,
    query_prjfs_file: Callable[[Path], PRJ_FILE_STATE],
) -> None:
    with instance.get_thrift_client_legacy() as client:
        errors, missing_inodes = _validate_loaded_content(
            client, checkout.path, query_prjfs_file
        )

    if errors:
        tracker.add_problem(LoadedFileHasDifferentContentOnDisk(errors))

    if missing_inodes:
        tracker.add_problem(MissingInodesForFiles(missing_inodes))


class HighInodeCountProblem(Problem):
    def __init__(self, path: Path, inode_count: int) -> None:
        super().__init__(
            description=f"Mount point {path} has {inode_count} files on disk, which may impact EdenFS performance",
            # TODO(T94186741): Change remediation instructions once we can unload inodes on demand.
            remediation="Reclone your repository to improve performance, if needed: https://fburl.com/wiki/ji8ik51v",
            severity=ProblemSeverity.ADVICE,
        )


class UnknownInodeCountProblem(Problem):
    def __init__(self, path: Path) -> None:
        super().__init__(
            description=f"Unable to determine the number of inodes loaded for mount point {path}",
            severity=ProblemSeverity.ERROR,
        )


def check_inode_counts(
    tracker: ProblemTracker, instance: EdenInstance, checkout: CheckoutInfo
) -> None:
    # This check is specific to the Windows implementation.
    if sys.platform != "win32":
        return

    threshold = instance.get_config_int(
        "doctor.windows-inode-count-problem-threshold", 1_000_000
    )

    inode_info = checkout.mount_inode_info
    if inode_info is None:
        tracker.add_problem(UnknownInodeCountProblem(checkout.path))
        return

    inode_count = (
        inode_info.loadedFileCount
        + inode_info.loadedTreeCount
        + inode_info.unloadedInodeCount
    )
    if inode_count > threshold:
        tracker.add_problem(HighInodeCountProblem(checkout.path, inode_count))
