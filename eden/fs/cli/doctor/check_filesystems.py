#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

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
from eden.fs.cli.filesystem import FsUtil
from eden.fs.cli.prjfs import PRJ_FILE_STATE
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


class MaterializedInodesAreInaccessible(Problem):
    def __init__(self, paths: List[Path]) -> None:
        super().__init__(
            "\n".join(
                [
                    f"{path} is inaccessible despite EdenFS believing it should be"
                    for path in paths
                ]
            ),
            severity=ProblemSeverity.ERROR,
        )


def check_materialized_are_accessible(
    tracker: ProblemTracker,
    instance: EdenInstance,
    checkout: EdenCheckout,
) -> None:
    mismatched_mode = []
    inaccessible_inodes = []

    with instance.get_thrift_client_legacy() as client:
        materialized = client.debugInodeStatus(
            bytes(checkout.path),
            b"",
            flags=DIS_REQUIRE_MATERIALIZED,
            sync=SyncBehavior(),
        )

    for materialized_dir in materialized:
        path = Path(os.fsdecode(materialized_dir.path))
        try:
            st = os.lstat(checkout.path / path)
        except OSError:
            inaccessible_inodes += [path]
            continue

        if not stat.S_ISDIR(st.st_mode):
            mismatched_mode += [(path, stat.S_IFDIR, st.st_mode)]

        for dirent in materialized_dir.entries:
            if dirent.materialized:
                dirent_path = path / Path(os.fsdecode(dirent.name))
                try:
                    dirent_stat = os.lstat(checkout.path / dirent_path)
                except OSError:
                    inaccessible_inodes += [dirent_path]
                    continue

                # TODO(xavierd): Symlinks are for now recognized as files.
                dirent_mode = (
                    stat.S_IFREG
                    if stat.S_ISLNK(dirent_stat.st_mode)
                    else stat.S_IFMT(dirent_stat.st_mode)
                )
                if dirent_mode != stat.S_IFMT(dirent.mode):
                    mismatched_mode += [(dirent_path, dirent_stat.st_mode, dirent.mode)]

    if inaccessible_inodes != []:
        tracker.add_problem(MaterializedInodesAreInaccessible(inaccessible_inodes))

    if mismatched_mode != []:
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


def check_loaded_content(
    tracker: ProblemTracker,
    instance: EdenInstance,
    checkout: EdenCheckout,
    query_prjfs_file: Callable[[Path], PRJ_FILE_STATE],
) -> None:
    with instance.get_thrift_client_legacy() as client:
        loaded = client.debugInodeStatus(
            bytes(checkout.path),
            b"",
            flags=DIS_REQUIRE_LOADED,
            sync=SyncBehavior(),
        )

        errors = []
        for loaded_dir in loaded:
            path = Path(os.fsdecode(loaded_dir.path))

            for dirent in loaded_dir.entries:
                if not stat.S_ISREG(dirent.mode) or dirent.materialized:
                    continue

                dirent_path = path / Path(os.fsdecode(dirent.name))
                filestate = query_prjfs_file(checkout.path / dirent_path)
                if (
                    filestate & PRJ_FILE_STATE.HydratedPlaceholder
                    != PRJ_FILE_STATE.HydratedPlaceholder
                ):
                    # We should only compute the sha1 of files that have been read.
                    continue

                def compute_file_sha1(file: Path) -> bytes:
                    hasher = hashlib.sha1()
                    with open(checkout.path / dirent_path, "rb") as f:
                        while True:
                            buf = f.read(1024 * 1024)
                            if buf == b"":
                                break
                            hasher.update(buf)
                    return hasher.digest()

                sha1 = client.getSHA1(
                    bytes(checkout.path), [bytes(dirent_path)], sync=SyncBehavior()
                )[0].get_sha1()
                on_disk_sha1 = compute_file_sha1(checkout.path / dirent_path)
                if sha1 != on_disk_sha1:
                    errors += [(dirent_path, sha1, on_disk_sha1)]

        if errors != []:
            tracker.add_problem(LoadedFileHasDifferentContentOnDisk(errors))
