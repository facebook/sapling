#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import logging
import os
from typing import List, Set

from eden.cli import mtab
from eden.cli.doctor.problem import FixableProblem, ProblemTracker, RemediationError


def check_for_stale_mounts(
    tracker: ProblemTracker, mount_table: mtab.MountTable
) -> None:
    stale_mounts = get_all_stale_eden_mount_points(mount_table)
    if stale_mounts:
        tracker.add_problem(StaleMountsFound(stale_mounts, mount_table))


def printable_bytes(b: bytes) -> str:
    return b.decode("utf-8", "backslashreplace")


class StaleMountsFound(FixableProblem):
    def __init__(self, mounts: List[bytes], mount_table: mtab.MountTable) -> None:
        self._mounts = mounts
        self._mount_table = mount_table

    def description(self) -> str:
        mounts_str = "\n  ".join(printable_bytes(mount) for mount in self._mounts)
        return f"Found {self._mounts_str()}:\n  {mounts_str}"

    def _mounts_str(self) -> str:
        if len(self._mounts) == 1:
            return "1 stale edenfs mount"
        return f"{len(self._mounts)} stale edenfs mounts"

    def dry_run_msg(self) -> str:
        return f"Would unmount {self._mounts_str()}"

    def start_msg(self) -> str:
        return f"Unmounting {self._mounts_str()}"

    def perform_fix(self) -> None:
        unmounted = []
        failed_to_unmount = []

        # Attempt to lazy unmount all of them first. For some reason,
        # lazy unmount can sometimes release any bind mounts inside.
        for mp in self._mounts:
            if self._mount_table.unmount_lazy(mp):
                unmounted.append(mp)

        # Use a refreshed list -- it's possible MNT_DETACH succeeded on some of
        # the points.
        for mp in get_all_stale_eden_mount_points(self._mount_table):
            if self._mount_table.unmount_force(mp):
                unmounted.append(mp)
            else:
                failed_to_unmount.append(mp)

        if failed_to_unmount:
            message = (
                f"Failed to unmount {len(failed_to_unmount)} mount "
                f'point{"s" if len(failed_to_unmount) != 1 else ""}:\n  '
            )
            message += "\n  ".join(printable_bytes(mp) for mp in failed_to_unmount)
            raise RemediationError(message)


def get_all_stale_eden_mount_points(mount_table: mtab.MountTable) -> List[bytes]:
    log = logging.getLogger("eden.cli.doctor.stale_mounts")
    stale_eden_mount_points: Set[bytes] = set()
    for mount_point in get_all_eden_mount_points(mount_table):
        # All eden mounts should have a .eden directory.
        # If the edenfs daemon serving this mount point has died we
        # will get ENOTCONN when trying to access it.  (Simply calling
        # lstat() on the root directory itself can succeed even in this
        # case.)
        eden_dir = os.path.join(mount_point, b".eden")

        try:
            mount_table.check_path_access(eden_dir)
        except OSError as e:
            if e.errno == errno.ENOTCONN:
                stale_eden_mount_points.add(mount_point)
            else:
                log.warning(
                    f"Unclear whether {printable_bytes(mount_point)} "
                    f"is stale or not. lstat() failed: {e}"
                )

    return sorted(stale_eden_mount_points)


def get_all_eden_mount_points(mount_table: mtab.MountTable) -> Set[bytes]:
    all_system_mounts = mount_table.read()
    return {
        mount.mount_point
        for mount in all_system_mounts
        if mount.device == b"edenfs" and mount.vfstype == b"fuse"
    }
