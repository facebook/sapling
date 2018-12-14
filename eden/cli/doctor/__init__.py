#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
from textwrap import dedent
from typing import List, Optional, Set

from eden.cli import config as config_mod, filesystem, mtab, process_finder, ui, version
from eden.cli.config import EdenInstance

from . import (
    check_bind_mounts,
    check_hg,
    check_os,
    check_rogue_edenfs,
    check_stale_mounts,
    check_watchman,
)
from .problem import (
    DryRunFixer,
    FixableProblem,
    Problem,
    ProblemFixer,
    ProblemSeverity,
    ProblemTracker,
)


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
    check_os.run_operating_system_checks(fixer, instance, out)

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


def run_edenfs_not_healthy_checks(
    tracker: ProblemTracker,
    instance: EdenInstance,
    out: ui.Output,
    status: config_mod.HealthStatus,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
) -> None:
    check_stale_mounts.check_for_stale_mounts(tracker, mount_table)

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
    check_stale_mounts.check_for_stale_mounts(tracker, mount_table)
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
        check_bind_mounts.check_bind_mounts(
            tracker, mount_path, instance, client_info, mount_table, fs_util
        )

        if client_info["scm_type"] == "hg":
            snapshot_hex = client_info["snapshot"]
            check_hg.check_snapshot_dirstate_consistency(
                tracker, instance, mount_path, snapshot_hex
            )


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
