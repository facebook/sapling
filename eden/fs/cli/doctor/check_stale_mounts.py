#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import errno
import logging
import os
import subprocess
import sys
from typing import List, Set, Tuple

from eden.fs.cli import mtab
from eden.fs.cli.doctor.problem import (
    FixableProblem,
    Problem,
    ProblemTracker,
    RemediationError,
)
from eden.fs.cli.util import (
    get_environment_suitable_for_subprocess,
    is_edenfs_mount_device,
)


def check_for_stale_mounts(
    tracker: ProblemTracker, mount_table: mtab.MountTable
) -> None:
    sudo_perms = get_sudo_perms()
    [stale_mounts, hanging_mounts, unknown_status_mounts] = (
        get_all_stale_eden_mount_points(mount_table)
    )
    if stale_mounts:
        tracker.add_problem(StaleMountsFound(stale_mounts, mount_table, sudo_perms))
    if hanging_mounts:
        tracker.add_problem(HangingMountFound(hanging_mounts))


def printable_bytes(b: bytes) -> str:
    return b.decode("utf-8", "backslashreplace")


def get_sudo_perms() -> bool:
    """
    Checks if the user can run sudo. This will return False on Windows
    """
    if sys.platform == "win32":
        return False
    try:
        env = get_environment_suitable_for_subprocess()
        # Sudo -l with a command will list out the
        # path to the binary for that command if you have
        # permissions to run it. Otherwise it will return nothing.
        result = subprocess.run(
            ["sudo", "-l", "umount"],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            text=True,
        )
        if "umount" in result.stdout:
            return True
        return False
    except Exception as ex:
        print(
            f"Sudo is not available to the current user: {ex} \n"
            "Note: this is not necessarily an error. Some platforms restrict users to have super user permissions, Eg: On Demand"
        )
        return False


class HangingMountFound(Problem):
    def __init__(self, mounts: List[bytes]) -> None:
        mounts_str = "\n  ".join(printable_bytes(mount) for mount in mounts)
        super().__init__(
            f"Found hanging mounts: \n {mounts_str}",
            "You can try restarting EdenFS by running `eden restart`.",
        )


class StaleMountsFound(FixableProblem):
    def __init__(
        self, mounts: List[bytes], mount_table: mtab.MountTable, sudo_perms: bool
    ) -> None:
        self._mounts = mounts
        self._mount_table = mount_table
        self.sudo_perms = sudo_perms

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
        if not self.sudo_perms:
            raise RemediationError(
                "Unable to unmount stale mounts due to missing sudo permissions."
            )
        unmounted = []
        failed_to_unmount = []

        # Attempt to lazy unmount all of them first. For some reason,
        # lazy unmount can sometimes release any bind mounts inside.
        for mp in self._mounts:
            if self._mount_table.unmount_lazy(mp):
                unmounted.append(mp)

        # Use a refreshed list -- it's possible MNT_DETACH succeeded on some of
        # the points.
        for mp in get_all_stale_eden_mount_points(self._mount_table)[0]:
            if self._mount_table.unmount_force(mp):
                unmounted.append(mp)
            else:
                failed_to_unmount.append(mp)

        if failed_to_unmount:
            message = (
                f"Failed to unmount {len(failed_to_unmount)} mount "
                f"point{'s' if len(failed_to_unmount) != 1 else ''}:\n  "
            )
            message += "\n  ".join(printable_bytes(mp) for mp in failed_to_unmount)
            raise RemediationError(message)

    def check_fix(self) -> bool:
        mount_points = get_all_stale_eden_mount_points(self._mount_table)
        # check that there are no stale or unknown mount points. We have a different check for hanging mounts
        return mount_points[0] == [] and mount_points[2] == []


def get_stale_eden_mount_points(
    mount_table: mtab.MountTable, mount_points: Set[Tuple[bytes, bytes]]
) -> Tuple[List[bytes], List[bytes], List[bytes]]:
    """
    Check listed eden mount points queried
    Return [stale mount points, hanging mount points, unknown status mount points]
    """
    log = logging.getLogger("eden.fs.cli.doctor.stale_mounts")
    stale_eden_mount_points: Set[bytes] = set()
    hung_eden_mount_points: Set[bytes] = set()
    unknown_status_eden_mount_points: Set[bytes] = set()
    for mount_point, mount_type in mount_points:
        # All eden mounts should have a .eden directory.
        # If the edenfs daemon serving this mount point has died we
        # will get ENOTCONN when trying to access it.  (Simply calling
        # lstat() on the root directory itself can succeed even in this
        # case.)
        eden_dir = os.path.join(mount_point, b".eden")

        try:
            mount_table.check_path_access(eden_dir, mount_type)
        except OSError as e:
            if e.errno == errno.ENOTCONN or e.errno == errno.ENXIO:
                stale_eden_mount_points.add(mount_point)
            elif e.errno == errno.ETIMEDOUT:
                hung_eden_mount_points.add(mount_point)
            else:
                log.warning(
                    f"Unclear whether {printable_bytes(mount_point)} "
                    f"is stale or not. lstat() failed: {e}"
                )
                unknown_status_eden_mount_points.add(mount_point)

    return (
        sorted(stale_eden_mount_points),
        sorted(hung_eden_mount_points),
        sorted(unknown_status_eden_mount_points),
    )


def get_all_stale_eden_mount_points(
    mount_table: mtab.MountTable,
) -> Tuple[List[bytes], List[bytes], List[bytes]]:
    """
    Check all eden mount points queried
    Return [stale mount points, hanging mount points, unknown status mount points]
    """
    return get_stale_eden_mount_points(
        mount_table, get_all_eden_mount_points(mount_table)
    )


def get_all_eden_mount_points(mount_table: mtab.MountTable) -> Set[Tuple[bytes, bytes]]:
    """
    Returns a set of mount point path, mount point type pairs of all of the
    mounts which seem to be EdenFS mounts.
    """
    all_system_mounts = mount_table.read()
    eden_mounts = set()
    for mount in all_system_mounts:
        if is_edenfs_mount_device(mount.device):
            if (
                mount.vfstype == b"fuse"
                or mount.vfstype == b"macfuse_eden"
                or mount.vfstype == b"fuse.edenfs"
            ):
                eden_mounts.add((mount.mount_point, b"fuse"))
            elif mount.vfstype == b"nfs" or mount.vfstype == b"edenfs:":
                eden_mounts.add((mount.mount_point, b"nfs"))

    return eden_mounts
