#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import collections
import errno
import io
import json
import logging
import os
import platform
import re
import shlex
import stat
import subprocess
import typing
from textwrap import dedent
from typing import Any, Dict, List, Optional, Set, Tuple

import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
from eden.cli import config as config_mod, filesystem, mtab, process_finder, ui, version
from eden.cli.config import EdenInstance
from thrift.Thrift import TApplicationException

from . import check_rogue_edenfs, check_watchman
from .problem import (
    DryRunFixer,
    FixableProblem,
    Problem,
    ProblemFixer,
    ProblemSeverity,
    ProblemTracker,
    RemediationError,
)


log = logging.getLogger("eden.cli.doctor")

# working_directory_was_stale may be set to True by the CLI main module
# if the original working directory referred to a stale eden mount point.
working_directory_was_stale = False


def cure_what_ails_you(
    instance: EdenInstance,
    dry_run: bool,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
    process_finder: process_finder.ProcessFinder,
    out: Optional[ui.Output] = None,
) -> int:
    if out is None:
        out = ui.get_output()

    if not dry_run:
        fixer = ProblemFixer(out)
    else:
        fixer = DryRunFixer(out)

    if working_directory_was_stale:
        fixer.add_problem(StaleWorkingDirectory())

    # check OS type, kernel version etc.
    run_operating_system_checks(fixer, instance, out)

    # check multiple edenfs running with some rogue stale PIDs
    check_rogue_edenfs.check_many_edenfs_are_running(fixer, process_finder)

    status = instance.check_health()
    if not status.is_healthy():
        run_edenfs_not_healthy_checks(
            fixer, instance, out, status, mount_table, fs_util
        )
        if fixer.num_problems == 0:
            out.writeln("Eden is not in use.")
            return 0
    else:
        run_normal_checks(fixer, instance, out, mount_table, fs_util)

    if fixer.num_problems == 0:
        out.writeln("No issues detected.", fg=out.GREEN)
        return 0

    def problem_count(num: int) -> str:
        if num == 1:
            return "1 problem"
        return f"{num} problems"

    if dry_run:
        out.writeln(
            f"Discovered {problem_count(fixer.num_problems)} during --dry-run",
            fg=out.YELLOW,
        )
        return 1

    if fixer.num_fixed_problems:
        out.writeln(
            f"Successfully fixed {problem_count(fixer.num_fixed_problems)}.",
            fg=out.YELLOW,
        )
    if fixer.num_failed_fixes:
        out.writeln(
            f"Failed to fix {problem_count(fixer.num_failed_fixes)}.", fg=out.RED
        )
    if fixer.num_manual_fixes:
        if fixer.num_manual_fixes == 1:
            msg = f"1 issue requires manual attention."
        else:
            msg = f"{fixer.num_manual_fixes} issues require manual attention."
        out.writeln(msg, fg=out.YELLOW)

    if fixer.num_fixed_problems == fixer.num_problems:
        return 0

    out.write(
        "Ask in the Eden Users group if you need help fixing issues with Eden:\n"
        "https://fb.facebook.com/groups/eden.users/\n"
    )
    return 1


class OSProblem(Problem):
    pass


def _parse_os_kernel_version(version: str) -> Tuple[int, ...]:
    """Parses kernel version string.
    Example version string: 4.11.3-67_fbk17_4093_g2bf19e7a0b95
    Returns integer representations of the version, eg. (4, 11, 3, 67).
    """
    version = re.sub(r"[_-]", ".", version)
    split_version = version.split(".")[:4]
    parsed_kernel_version = tuple(map(int, split_version))
    if len(parsed_kernel_version) < 4:
        # right pad with zeros if the kernel version isn't 4 numbers
        parsed_kernel_version = (
            *parsed_kernel_version,
            *[0] * (4 - len(parsed_kernel_version)),
        )
    return parsed_kernel_version


def _os_is_kernel_version_too_old(instance: EdenInstance, release: str) -> bool:
    try:
        min_kernel_version = instance.get_config_value("doctor.minimum-kernel-version")
    except KeyError:
        return False
    if min_kernel_version is None:
        return False
    return _parse_os_kernel_version(release) < _parse_os_kernel_version(
        min_kernel_version
    )


def _os_is_bad_release(instance: EdenInstance, release: str) -> bool:
    try:
        known_bad_kernel_versions = instance.get_config_value(
            "doctor.known-bad-kernel-versions"
        )
    except KeyError:
        return False
    if known_bad_kernel_versions is None:
        return False
    for regex in known_bad_kernel_versions.split(","):
        if re.search(regex, release):
            return True  # matched known bad release
    return False  # no match to bad release


def run_operating_system_checks(
    tracker: ProblemTracker, instance: EdenInstance, out: ui.Output
) -> None:
    if platform.system() != "Linux":
        return

    # get kernel version string; same as "uname -r"
    current_kernel_release = platform.release()

    # check if version too low
    result = _os_is_kernel_version_too_old(instance, current_kernel_release)
    if result:
        tracker.add_problem(
            OSProblem(
                # TODO: Reword these messages prior to public release
                description=f"Kernel version {current_kernel_release} too low.",
                remediation=f"Reboot to upgrade kernel version.",
            )
        )
        # if the kernel version is too low, return here as continuing to
        # further checks has no benefit
        return

    # check against known bad versions
    result = _os_is_bad_release(instance, current_kernel_release)
    if result:
        tracker.add_problem(
            OSProblem(
                # TODO: Reword these messages prior to public release
                description=f"Kernel {current_kernel_release} is a known "
                + "bad kernel.",
                remediation="Reboot to upgrade kernel version.",
            )
        )
        return


def run_edenfs_not_healthy_checks(
    tracker: ProblemTracker,
    instance: EdenInstance,
    out: ui.Output,
    status: config_mod.HealthStatus,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
) -> None:
    check_for_stale_mounts(tracker, mount_table)

    configured_mounts = instance.get_mount_paths()
    if configured_mounts:
        tracker.add_problem(EdenfsNotHealthy())


class EdenfsNotHealthy(Problem):
    def __init__(self) -> None:
        super().__init__(
            "Eden is not running.", remediation="To start Eden, run:\n\n    eden start"
        )


def run_normal_checks(
    tracker: ProblemTracker,
    instance: EdenInstance,
    out: ui.Output,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
) -> None:
    with instance.get_thrift_client() as client:
        active_mount_points: List[str] = [
            os.fsdecode(mount.mountPoint)
            for mount in client.listMounts()
            if mount.mountPoint is not None
        ]

    check_active_mounts(tracker, active_mount_points, mount_table)
    check_for_stale_mounts(tracker, mount_table)
    check_edenfs_version(tracker, instance)

    watchman_info = check_watchman.pre_check()

    configured_mounts = list(instance.get_mount_paths())
    configured_mounts.sort()
    for mount_path in configured_mounts:
        if mount_path not in active_mount_points:
            tracker.add_problem(CheckoutNotMounted(instance, mount_path))

    for mount_path in sorted(active_mount_points):
        if mount_path not in configured_mounts:
            # TODO: if there are mounts in active_mount_points that aren't in
            # configured_mounts, should we try to add them to the config?
            # I've only seen this happen in the wild if a clone fails partway,
            # for example, if a post-clone hook fails.
            continue

        out.writeln(f"Checking {mount_path}")
        client_info = instance.get_client_info(mount_path)
        check_watchman.check_active_mount(tracker, mount_path, watchman_info)
        check_bind_mounts(
            tracker, mount_path, instance, client_info, mount_table, fs_util
        )

        if client_info["scm_type"] == "hg":
            snapshot_hex = client_info["snapshot"]
            check_snapshot_dirstate_consistency(
                tracker, instance, mount_path, snapshot_hex
            )


def printable_bytes(b: bytes) -> str:
    return b.decode("utf-8", "backslashreplace")


class CheckoutNotMounted(FixableProblem):
    def __init__(self, instance: EdenInstance, mount_path: str) -> None:
        self._instance = instance
        self._mount_path = mount_path

    def description(self) -> str:
        return f"{self._mount_path} is not currently mounted"

    def dry_run_msg(self) -> str:
        return f"Would remount {self._mount_path}"

    def start_msg(self) -> str:
        return f"Remounting {self._mount_path}"

    def perform_fix(self) -> None:
        try:
            self._instance.mount(self._mount_path)
        except Exception as ex:
            if "is too short for header" in str(ex):
                raise Exception(
                    f"""\
{ex}

{self._mount_path} appears to have been corrupted.
This can happen if your devserver was hard-rebooted.
To recover, you will need to remove and reclone the repo.
You will lose uncommitted work or shelves, but all your local
commits are safe.
If you have non-trivial uncommitted work that you need to recover
you may be able to restore it from your system backup.

To remove the corrupted repo, run: `eden rm {self._mount_path}`"""
                )
            raise


def check_active_mounts(
    tracker: ProblemTracker,
    active_mount_points: List[str],
    mount_table: mtab.MountTable,
) -> None:
    for amp in active_mount_points:
        try:
            mount_table.lstat(amp).st_dev
        except OSError as ex:
            tracker.add_problem(
                Problem(f"Failed to lstat active eden mount {amp}: {ex}")
            )


def check_for_stale_mounts(
    tracker: ProblemTracker, mount_table: mtab.MountTable
) -> None:
    stale_mounts = get_all_stale_eden_mount_points(mount_table)
    if stale_mounts:
        tracker.add_problem(StaleMountsFound(stale_mounts, mount_table))


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
    stale_eden_mount_points: Set[bytes] = set()
    for mount_point in get_all_eden_mount_points(mount_table):
        try:
            # All eden mounts should have a .eden directory.
            # If the edenfs daemon serving this mount point has died we
            # will get ENOTCONN when trying to access it.  (Simply calling
            # lstat() on the root directory itself can succeed even in this
            # case.)
            eden_dir = os.path.join(mount_point, b".eden")
            mount_table.lstat(eden_dir)
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


def check_bind_mounts(
    tracker: ProblemTracker,
    mount_path: str,
    instance: EdenInstance,
    client_info: collections.OrderedDict,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
) -> None:
    """Check that bind mounts exist and have different device IDs than the top-level
    checkout mount path, to confirm that they are mounted."""
    try:
        checkout_path_stat = mount_table.lstat(mount_path)
    except OSError as ex:
        tracker.add_problem(Problem(f"Failed to stat eden mount: {mount_path}: {ex}"))
        return

    client_dir = client_info["client-dir"]
    client_bind_mount_dir = os.path.join(client_dir, "bind-mounts")
    bind_mounts = client_info["bind-mounts"]

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


def check_snapshot_dirstate_consistency(
    tracker: ProblemTracker, instance: EdenInstance, path: str, snapshot_hex: str
) -> None:
    dirstate = os.path.join(path, ".hg", "dirstate")
    try:
        with open(dirstate, "rb") as f:
            parents, tuples_dict, copymap = eden.dirstate.read(f, dirstate)
    except OSError as ex:
        if ex.errno == errno.ENOENT:
            tracker.add_problem(MissingHgDirectory(path))
        else:
            tracker.add_problem(Problem(f"Unable to access {path}/.hg/dirstate: {ex}"))
        return

    p1_hex = binascii.hexlify(parents[0]).decode("utf-8")
    p2_hex = binascii.hexlify(parents[1]).decode("utf-8")
    null_hash_hex = 40 * "0"
    is_p2_hex_valid = True
    current_hex = snapshot_hex
    try:
        is_snapshot_hex_valid = is_commit_hash_valid(instance, path, snapshot_hex)
        current_hex = p1_hex
        is_p1_hex_valid = is_commit_hash_valid(instance, path, p1_hex)
        if p2_hex != null_hash_hex:
            current_hex = p2_hex
            is_p2_hex_valid = is_commit_hash_valid(instance, path, p2_hex)
    except Exception as ex:
        tracker.add_problem(
            Problem(
                f"Failed to get scm status for mount {path} "
                f"at revision {current_hex}:\n {ex}"
            )
        )
        return

    if is_p2_hex_valid is not True:
        p2_hex = null_hash_hex

    if snapshot_hex != p1_hex:
        if is_p1_hex_valid:
            new_parents = (binascii.unhexlify(p1_hex), binascii.unhexlify(p2_hex))
            tracker.add_problem(
                SnapshotMismatchError(instance, path, snapshot_hex, parents)
            )
        elif is_snapshot_hex_valid:
            new_parents = (binascii.unhexlify(snapshot_hex), binascii.unhexlify(p2_hex))
            tracker.add_problem(
                DirStateInvalidError(  # type: ignore
                    instance, path, p1_hex, new_parents, tuples_dict, copymap
                )
            )

    if (not is_snapshot_hex_valid) and (not is_p1_hex_valid):
        last_valid_commit_hash = get_tip_commit_hash()
        new_parents = (
            binascii.unhexlify(last_valid_commit_hash),
            binascii.unhexlify(p2_hex),
        )
        tracker.add_problem(
            DirStateInvalidError(  # type: ignore
                instance, path, p1_hex, new_parents, tuples_dict, copymap
            )
        )


class DirStateInvalidError(FixableProblem):
    def __init__(
        self,
        instance: EdenInstance,
        mount_path: str,
        invalid_commit_hash: str,
        hg_parents: Tuple[bytes, bytes],
        tuples_dict: Dict[bytes, Tuple[str, int, int]],
        copymap: Dict[bytes, bytes],
    ) -> None:
        self._instance = instance
        self._mount_path = mount_path
        self._invalid_commit_hash = invalid_commit_hash
        self._hg_parents = hg_parents
        self._tuples_dict = tuples_dict
        self._copymap = copymap

    def dirstate(self) -> str:
        return os.path.join(self._mount_path, ".hg", "dirstate")

    def p1_hex(self) -> str:
        return binascii.hexlify(self._hg_parents[0]).decode("utf-8")

    def description(self) -> str:
        return (
            f"mercurial's parent commit {self._invalid_commit_hash}"
            f" in {self.dirstate()} is invalid\n"
        )

    def dry_run_msg(self) -> str:
        return f"Would fix Eden to point to parent commit {self.p1_hex()}"

    def start_msg(self) -> str:
        return f"Fixing Eden to point to parent commit {self.p1_hex()}"

    def perform_fix(self) -> None:
        with open(self.dirstate(), "wb") as f:
            eden.dirstate.write(f, self._hg_parents, self._tuples_dict, self._copymap)

        parents = eden_ttypes.WorkingDirectoryParents(parent1=self._hg_parents[0])
        if self._hg_parents[1] != (20 * b"\0"):
            parents.parent2 = self._hg_parents[1]

        with self._instance.get_thrift_client() as client:
            client.resetParentCommits(self._mount_path.encode("utf-8"), parents)


def get_tip_commit_hash() -> str:
    args = ["hg", "log", "-T", "{node}\n", "-r", "tip"]
    env = dict(os.environ, HGPLAIN="1")
    stdout = subprocess.check_output(args, universal_newlines=True, env=env)
    lines: List[str] = list(filter(None, stdout.split("\n")))
    return lines[-1]


def is_commit_hash_valid(
    instance: EdenInstance, mount_path: str, commit_hash: str
) -> bool:
    try:
        with instance.get_thrift_client() as client:
            client.getScmStatus(
                os.fsencode(mount_path), False, commit_hash.encode("utf-8")
            )
            return True
    except TApplicationException as ex:
        if "RepoLookupError: unknown revision" in str(ex):
            return False
        raise


class SnapshotMismatchError(FixableProblem):
    def __init__(
        self,
        instance: EdenInstance,
        path: str,
        snapshot_hex: str,
        hg_parents: Tuple[bytes, bytes],
    ) -> None:
        self._instance = instance
        self._path = path
        self._snapshot_hex = snapshot_hex
        self._hg_parents = hg_parents

    def p1_hex(self) -> str:
        return binascii.hexlify(self._hg_parents[0]).decode("utf-8")

    def description(self) -> str:
        return (
            f"mercurial's parent commit for {self._path} is {self.p1_hex()},\n"
            f"but Eden's internal hash in its SNAPSHOT file is {self._snapshot_hex}.\n"
        )

    def dry_run_msg(self) -> str:
        return f"Would fix Eden to point to parent commit {self.p1_hex()}"

    def start_msg(self) -> str:
        return f"Fixing Eden to point to parent commit {self.p1_hex()}"

    def perform_fix(self) -> None:
        parents = eden_ttypes.WorkingDirectoryParents(parent1=self._hg_parents[0])
        if self._hg_parents[1] != (20 * b"\0"):
            parents.parent2 = self._hg_parents[1]

        with self._instance.get_thrift_client() as client:
            client.resetParentCommits(self._path.encode("utf-8"), parents)


class MissingHgDirectory(Problem):
    def __init__(self, path: str) -> None:
        remediation = f"""\
The most common cause of this is if you previously tried to manually remove this eden
mount with "rm -rf".  You should instead remove it using "eden rm {path}",
and can re-clone the checkout afterwards if desired."""
        super().__init__(f"{path}/.hg/dirstate is missing", remediation)
        self._path = path


class StaleWorkingDirectory(Problem):
    def __init__(self) -> None:
        remediation = f"""\
Run "cd / && cd -" to update your shell's working directory."""
        super().__init__(
            f"Your current working directory appears to be a stale Eden mount point",
            remediation,
        )


def check_edenfs_version(tracker: ProblemTracker, instance: EdenInstance) -> None:
    rver, release = version.get_running_eden_version_parts(instance)
    if not rver or not release:
        # This could be a dev build that returns the empty
        # string for both of these values.
        return

    running_version = version.format_running_eden_version((rver, release))
    installed_version = version.get_installed_eden_rpm_version()
    if running_version == installed_version:
        return

    tracker.add_problem(
        Problem(
            dedent(
                f"""\
The version of Eden that is installed on your machine is:
    fb-eden-{installed_version}.x86_64
but the version of Eden that is currently running is:
    fb-eden-{running_version}.x86_64

Consider running `eden restart` to migrate to the newer version, which
may have important bug fixes or performance improvements.
"""
            ),
            severity=ProblemSeverity.ADVICE,
        )
    )
