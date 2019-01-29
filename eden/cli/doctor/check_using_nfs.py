#!/usr/bin/env python3
#
# Copyright (c) 2018-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import subprocess
from pathlib import Path

from eden.cli.config import EdenInstance
from eden.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker


def check_using_nfs_path(tracker: ProblemTracker, mount_path: str) -> None:
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


def check_shared_path(tracker: ProblemTracker, mount_path: str) -> None:
    shared_path = Path(mount_path) / ".hg" / "sharedpath"
    try:
        dst_shared_path = shared_path.read_text()
    except (FileNotFoundError, IsADirectoryError):
        return
    except Exception as e:
        tracker.add_problem(Problem(f"Failed to read .hg/sharedpath: {e}"))
        return

    if is_nfs_mounted(dst_shared_path):
        msg = (
            f"The Mercurial data directory for {shared_path} is at"
            f" {dst_shared_path} which is on a NFS filesystem."
            f" Accessing files and directories in this repository will be slow."
        )
        problem = Problem(msg, severity=ProblemSeverity.ADVICE)
        tracker.add_problem(problem)


def is_nfs_mounted(path: str) -> bool:
    args = ["stat", "-fc", "%T", "--", path]
    try:
        out = subprocess.check_output(args)
        return out == b"nfs\n"
    except subprocess.CalledProcessError:
        return False
