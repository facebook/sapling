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

from eden.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker


def check_using_nfs_path(
    tracker: ProblemTracker, mount_path: str, client_dir: str
) -> None:
    check_client_dir(tracker, mount_path, client_dir)
    check_shared_path(tracker, mount_path)


def check_client_dir(tracker: ProblemTracker, mount_path: str, client_dir: str) -> None:
    if is_nfs_mounted(client_dir):
        msg = (
            f"{client_dir} which is used for mounting {mount_path}"
            f" is on a NFS filesystem."
            f" Accessing files and directories in this repository will be slow."
        )
        problem = UsingNfs(msg)
        tracker.add_problem(problem)


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
        problem = UsingNfs(msg)
        tracker.add_problem(problem)


class UsingNfs(Problem):
    def __init__(self, description: str) -> None:
        self._description = description
        self._remediation = None

    def description(self) -> str:
        return self._description

    def severity(self) -> ProblemSeverity:
        return ProblemSeverity.ADVICE


def is_nfs_mounted(path: str) -> bool:
    args = ["stat", "-fc", "%T", "--", path]
    try:
        out = subprocess.check_output(args)
        return out == b"nfs\n"
    except subprocess.CalledProcessError:
        return False
