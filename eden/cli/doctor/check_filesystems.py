#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, Set

from eden.cli.config import EdenInstance
from eden.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker
from eden.cli.filesystem import FsUtil


def check_using_nfs_path(tracker: ProblemTracker, mount_path: Path) -> None:
    check_shared_path(tracker, mount_path)


def check_eden_directory(tracker: ProblemTracker, instance: EdenInstance) -> None:
    if not is_nfs_mounted(str(instance.state_dir)):
        return

    msg = (
        f"Eden's state directory is on an NFS file system: {instance.state_dir}\n"
        f"  This will likely cause performance problems and/or other errors."
    )

    # On FB devservers the default Eden state directory path is ~/local/.eden
    # Normally ~/local is expected to be a symlink to local disk (for users who are
    # still using NFS home directories in the first place).  The most common cause of
    # the Eden state directory being on NFS is for users that somehow have a regular
    # directory at ~/local rather than a symlink.  Suggest checking this as a
    # remediation.
    remediation = (
        "The most common cause for this is if your ~/local symlink does not point "
        "to local disk.  Make sure that ~/local is a symlink pointing to local disk "
        "and then restart Eden."
    )
    tracker.add_problem(Problem(msg, remediation))


def get_shared_path(mount_path: Path) -> Path:
    return mount_path / ".hg" / "sharedpath"


def read_shared_path(tracker: ProblemTracker, shared_path: Path) -> str:
    try:
        return shared_path.read_text()
    except (FileNotFoundError, IsADirectoryError):
        raise
    except Exception as e:
        tracker.add_problem(Problem(f"Failed to read .hg/sharedpath: {e}"))
        raise


def check_shared_path(tracker: ProblemTracker, mount_path: Path) -> None:
    shared_path = get_shared_path(mount_path)
    try:
        dst_shared_path = read_shared_path(tracker, shared_path)
    except Exception:
        return

    if is_nfs_mounted(dst_shared_path):
        msg = (
            f"The Mercurial data directory for {shared_path} is at"
            f" {dst_shared_path} which is on a NFS filesystem."
            f" Accessing files and directories in this repository will be slow."
        )
        problem = Problem(msg, severity=ProblemSeverity.ADVICE)
        tracker.add_problem(problem)


def fstype_for_path(path: str) -> str:
    if sys.platform == "linux2":
        try:
            args = ["stat", "-fc", "%T", "--", path]
            return subprocess.check_output(args).decode("ascii").strip()
        except subprocess.CalledProcessError:
            return "unknown"

    return "unknown"


def is_nfs_mounted(path: str) -> bool:
    return fstype_for_path(path) == "nfs"


def get_mountpt(path) -> str:
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
            hg_cache_dir = subprocess.check_output(
                ["hg", "config", "remotefilelog.cachepath"],
                cwd=mount_path,
                env=dict(os.environ, HGPLAIN="1"),
            )
        except subprocess.CalledProcessError:
            continue

        eden_locations.append(hg_cache_dir.decode("utf-8").rstrip("\n"))

    # Set is used to skip duplicate mount folders
    return {get_mountpt(eden_location) for eden_location in eden_locations}


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
            disk_status = fs_util.statvfs(eden_mount_pt)

            avail = disk_status.f_frsize * disk_status.f_bavail
            size = disk_status.f_frsize * disk_status.f_blocks
            if size == 0:
                continue

            used = size - avail
            used_percent = float(used) / size

            message = (
                "Eden lazily loads your files and needs enough disk space to "
                "store these files when loaded."
            )
            extra_message = instance.get_config_value(
                "doctor.low-disk-space-message", ""
            )
            if extra_message:
                message = f"{message} {extra_message}"

            if avail <= prob_error_absolute_space_used_threshold:
                tracker.add_problem(
                    Problem(
                        f"{eden_mount_pt} "
                        f"has only {str(avail)} bytes available. "
                        f"{message}",
                        severity=ProblemSeverity.ERROR,
                    )
                )
            elif used_percent >= prob_advice_space_used_ratio_threshold:
                tracker.add_problem(
                    Problem(
                        f"{eden_mount_pt} "
                        f"is {used_percent:.2%} full. "
                        f"{message}",
                        severity=ProblemSeverity.ADVICE,
                    )
                )
