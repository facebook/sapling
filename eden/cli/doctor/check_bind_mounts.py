#!/usr/bin/env python3
#
# Copyright (c) 2018-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
import os
import stat

from eden.cli import filesystem, mtab
from eden.cli.config import EdenCheckout
from eden.cli.doctor.problem import FixableProblem, Problem, ProblemTracker


def check_bind_mounts(
    tracker: ProblemTracker,
    checkout: EdenCheckout,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
) -> None:
    """Check that bind mounts exist and have different device IDs than the top-level
    checkout mount path, to confirm that they are mounted."""
    mount_path = str(checkout.path)
    try:
        checkout_path_stat = mount_table.lstat(mount_path)
    except OSError as ex:
        tracker.add_problem(Problem(f"Failed to stat eden mount: {mount_path}: {ex}"))
        return

    client_bind_mount_dir = str(checkout.state_dir / "bind-mounts")
    bind_mounts = checkout.get_config().bind_mounts

    # Create a dictionary of client paths : mount paths
    # Client directory eg. /data/users/bob/.eden/clients/fbsource-eden/bind-mounts
    # Mount directory eg. /data/users/bob/fbsource/
    client_mount_path_dict = {}
    for client_suffix, mount_suffix in bind_mounts.items():
        path_in_client_dir = os.path.join(client_bind_mount_dir, client_suffix)
        path_in_mount_dir = os.path.join(mount_path, mount_suffix)
        client_mount_path_dict[path_in_client_dir] = path_in_mount_dir

    for path_in_client_dir, path_in_mount_dir in client_mount_path_dict.items():
        _check_bind_mount_client_path(tracker, path_in_client_dir, mount_table, fs_util)
        _check_bind_mount_path(
            tracker,
            path_in_client_dir,
            path_in_mount_dir,
            checkout_path_stat,
            mount_table,
            fs_util,
        )


def _check_bind_mount_client_path(
    tracker: ProblemTracker,
    path: str,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
) -> None:
    # Identify missing or non-directory client paths
    try:
        client_stat = mount_table.lstat(path)
        if not stat.S_ISDIR(client_stat.st_mode):
            tracker.add_problem(NonDirectoryFile(path))
    except OSError as ex:
        if ex.errno == errno.ENOENT:
            tracker.add_problem(MissingBindMountClientDir(path, fs_util))
        else:
            tracker.add_problem(
                Problem(f"Failed to lstat bind mount source directory: {path}: {ex}")
            )


def _check_bind_mount_path(
    tracker: ProblemTracker,
    mount_source: str,
    mount_point: str,
    checkout_path_stat: mtab.MTStat,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
) -> None:
    # Identify missing or not mounted bind mounts
    try:
        bind_mount_stat = mount_table.lstat(mount_point)
        if not stat.S_ISDIR(bind_mount_stat.st_mode):
            tracker.add_problem(NonDirectoryFile(mount_point))
            return
        if bind_mount_stat.st_dev == checkout_path_stat.st_dev:
            tracker.add_problem(
                BindMountNotMounted(
                    mount_source,
                    mount_point,
                    mkdir=False,
                    fs_util=fs_util,
                    mount_table=mount_table,
                )
            )
    except OSError as ex:
        if ex.errno == errno.ENOENT:
            tracker.add_problem(
                BindMountNotMounted(
                    mount_source,
                    mount_point,
                    mkdir=True,
                    fs_util=fs_util,
                    mount_table=mount_table,
                )
            )
        else:
            tracker.add_problem(Problem(f"Failed to lstat mount path: {mount_point}"))


class NonDirectoryFile(Problem):
    def __init__(self, path: str) -> None:
        super().__init__(
            f"Expected {path} to be a directory",
            remediation=f"Please remove the file at {path}",
        )
        self._path = path


class MissingBindMountClientDir(FixableProblem):
    def __init__(self, path: str, fs_util: filesystem.FsUtil) -> None:
        self._path = path
        self._fs_util = fs_util

    def description(self) -> str:
        return f"Missing client directory for bind mount {self._path}"

    def dry_run_msg(self) -> str:
        return f"Would create directory {self._path}"

    def start_msg(self) -> str:
        return f"Creating directory {self._path}"

    def perform_fix(self) -> None:
        self._fs_util.mkdir_p(self._path)


class BindMountNotMounted(FixableProblem):
    def __init__(
        self,
        client_dir_path: str,
        mount_path: str,
        mkdir: bool,
        fs_util: filesystem.FsUtil,
        mount_table: mtab.MountTable,
    ) -> None:
        self._client_dir_path = client_dir_path
        self._mount_path = mount_path
        self._mkdir = mkdir
        self._fs_util = fs_util
        self._mount_table = mount_table

    def description(self) -> str:
        return f"Bind mount at {self._mount_path} is not mounted"

    def dry_run_msg(self) -> str:
        return f"Would remount bind mount at {self._mount_path}"

    def start_msg(self) -> str:
        return f"Remounting bind mount at {self._mount_path}"

    def perform_fix(self) -> None:
        if self._mkdir:
            self._fs_util.mkdir_p(self._mount_path)
        self._mount_table.create_bind_mount(self._client_dir_path, self._mount_path)
