#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import shlex
from pathlib import Path
from textwrap import dedent
from typing import Dict, Optional

from eden.cli import config as config_mod, filesystem, mtab, process_finder, ui, version
from eden.cli.config import EdenCheckout, EdenInstance
from facebook.eden.ttypes import MountState
from fb303_core.ttypes import fb303_status

from . import (
    check_bind_mounts,
    check_filesystems,
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

    check_working_directory(fixer)

    # check OS type, kernel version etc.
    check_os.run_operating_system_checks(fixer, instance, out)

    # check multiple edenfs running with some rogue stale PIDs
    check_rogue_edenfs.check_many_edenfs_are_running(fixer, process_finder)

    status = instance.check_health()
    if status.status == fb303_status.ALIVE:
        run_normal_checks(fixer, instance, out, mount_table, fs_util)
    elif status.status == fb303_status.STARTING:
        fixer.add_problem(EdenfsStarting())
    elif status.status == fb303_status.STOPPING:
        fixer.add_problem(EdenfsStopping())
    elif status.status == fb303_status.DEAD:
        run_edenfs_not_healthy_checks(
            fixer, instance, out, status, mount_table, fs_util
        )
        if fixer.num_problems == 0:
            out.writeln("Eden is not in use.")
            return 0
    else:
        fixer.add_problem(EdenfsUnexpectedStatus(status))

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
    check_filesystems.check_disk_usage(tracker, list(configured_mounts), instance)
    if configured_mounts:
        tracker.add_problem(EdenfsNotHealthy())


class EdenfsNotHealthy(Problem):
    def __init__(self) -> None:
        super().__init__(
            "Eden is not running.", remediation="To start Eden, run:\n\n    eden start"
        )


class EdenfsStarting(Problem):
    def __init__(self) -> None:
        remediation = '''\
Please wait for edenfs to finish starting.
If Eden seems to be taking too long to start you can try restarting it
with "eden restart"'''
        super().__init__("Eden is currently still starting.", remediation=remediation)


class EdenfsStopping(Problem):
    def __init__(self) -> None:
        remediation = """\
Either wait for edenfs to exit, or to forcibly kill Eden, run:

    eden stop --kill"""
        super().__init__("Eden is currently shutting down.", remediation=remediation)


class EdenfsUnexpectedStatus(Problem):
    def __init__(self, status: config_mod.HealthStatus) -> None:
        msg = f"Unexpected health status reported by edenfs: {status}"
        remediation = 'You can try restarting Eden by running "eden restart"'
        super().__init__(msg, remediation=remediation)


class CheckoutInfo:
    def __init__(
        self,
        instance: EdenInstance,
        path: Path,
        running_state_dir: Optional[Path] = None,
        configured_state_dir: Optional[Path] = None,
        state: Optional[MountState] = None,
    ):
        self.instance = instance
        self.path = path
        self.running_state_dir = running_state_dir
        self.configured_state_dir = configured_state_dir
        self.state = state

    def get_checkout(self) -> EdenCheckout:
        state_dir = (
            self.running_state_dir
            if self.running_state_dir is not None
            else self.configured_state_dir
        )
        assert state_dir is not None
        return EdenCheckout(self.instance, self.path, state_dir)


def run_normal_checks(
    tracker: ProblemTracker,
    instance: EdenInstance,
    out: ui.Output,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
) -> None:
    checkouts: Dict[Path, CheckoutInfo] = {}
    # Get information about the checkouts currently known to the running edenfs process
    with instance.get_thrift_client() as client:
        for mount in client.listMounts():
            # Old versions of edenfs did not return a mount state field.
            # These versions only listed running mounts, so treat the mount state
            # as running in this case.
            mount_state = mount.state if mount.state is not None else MountState.RUNNING
            path = Path(os.fsdecode(mount.mountPoint))
            checkout = CheckoutInfo(
                instance,
                path,
                running_state_dir=Path(os.fsdecode(mount.edenClientPath)),
                state=mount_state,
            )
            checkouts[path] = checkout

    # Get information about the checkouts listed in the config file
    for configured_checkout in instance.get_checkouts():
        checkout_info = checkouts.get(configured_checkout.path, None)
        if checkout_info is None:
            checkout_info = CheckoutInfo(instance, configured_checkout.path)
            checkout_info.configured_state_dir = configured_checkout.state_dir
            checkouts[checkout_info.path] = checkout_info

        checkout_info.configured_state_dir = configured_checkout.state_dir

    check_filesystems.check_eden_directory(tracker, instance)
    check_stale_mounts.check_for_stale_mounts(tracker, mount_table)
    check_edenfs_version(tracker, instance)
    check_filesystems.check_disk_usage(
        tracker, list(instance.get_mount_paths()), instance
    )

    watchman_info = check_watchman.pre_check()

    for path, checkout in sorted(checkouts.items()):
        out.writeln(f"Checking {path}")
        try:
            check_mount(tracker, checkout, mount_table, fs_util, watchman_info)
        except Exception as ex:
            tracker.add_problem(
                Problem(f"unexpected error while checking {path}: {ex}")
            )


def check_mount(
    tracker: ProblemTracker,
    checkout: CheckoutInfo,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
    watchman_info: check_watchman.WatchmanCheckInfo,
) -> None:
    if checkout.state is None:
        # This checkout is configured but not currently running.
        tracker.add_problem(CheckoutNotMounted(checkout))
    elif checkout.state == MountState.RUNNING:
        check_running_mount(tracker, checkout, mount_table, fs_util, watchman_info)
    elif checkout.state in (
        MountState.UNINITIALIZED,
        MountState.INITIALIZING,
        MountState.INITIALIZED,
        MountState.STARTING,
    ):
        tracker.add_problem(
            Problem(
                f"Checkout {checkout.path} is currently starting up.",
                f"If this checkout does not successfully finish starting soon, "
                'try running "eden restart"',
                severity=ProblemSeverity.ADVICE,
            )
        )
    elif checkout.state in (
        MountState.SHUTTING_DOWN,
        MountState.SHUT_DOWN,
        MountState.DESTROYING,
    ):
        tracker.add_problem(
            Problem(
                f"Checkout {checkout.path} is currently shutting down.",
                f"If this checkout does not successfully finish shutting down soon, "
                'try running "eden restart"',
            )
        )
    elif checkout.state == MountState.FUSE_ERROR:
        # TODO: We could potentially try automatically unmounting and remounting.
        # In general mounts shouldn't remain in this state for long, so we probably
        # don't need to worry too much about this case.
        tracker.add_problem(
            Problem(f"Checkout {checkout.path} encountered a FUSE error while mounting")
        )
    else:
        tracker.add_problem(
            Problem(
                f"edenfs reports that checkout {checkout.path} is in "
                "unknown state {checkout.state}"
            )
        )


def check_running_mount(
    tracker: ProblemTracker,
    checkout_info: CheckoutInfo,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
    watchman_info: check_watchman.WatchmanCheckInfo,
) -> None:
    if checkout_info.configured_state_dir is None:
        tracker.add_problem(CheckoutNotConfigured(checkout_info))
        return
    elif checkout_info.configured_state_dir != checkout_info.running_state_dir:
        tracker.add_problem(CheckoutConfigurationMismatch(checkout_info))
        return

    checkout = checkout_info.get_checkout()
    try:
        config = checkout.get_config()
    except Exception as ex:
        tracker.add_problem(
            Problem(f"error parsing the configuration for {checkout_info.path}: {ex}")
        )
        # Just skip the remaining checks.
        # Most of them rely on values from the configuration.
        return

    check_filesystems.check_using_nfs_path(tracker, checkout.path)
    check_watchman.check_active_mount(tracker, str(checkout.path), watchman_info)
    check_bind_mounts.check_bind_mounts(tracker, checkout, mount_table, fs_util)
    if config.scm_type == "hg":
        check_hg.check_hg(tracker, checkout)


class CheckoutNotConfigured(Problem):
    def __init__(self, checkout_info: CheckoutInfo) -> None:
        msg = (
            f"Checkout {checkout_info.path} is running but not "
            "listed in Eden's configuration file."
        )
        # TODO: Maybe we could use some better suggestions here, depending on
        # common cases that might lead to this situation.  (At the moment I believe this
        # can occur if `eden clone` fails to set up the .hg directory after mounting.)
        quoted_path = shlex.quote(str(checkout_info.path))
        remediation = (
            f'Running "eden unmount {quoted_path}" will unmount this checkout.'
        )
        super().__init__(msg, remediation)


class CheckoutConfigurationMismatch(Problem):
    def __init__(self, checkout_info: CheckoutInfo) -> None:
        msg = f"""\
The running configuration for {checkout_info.path} is different than "
the on-disk state in Eden's configuration file:
- Running state directory:    {checkout_info.running_state_dir}
- Configured state directory: {checkout_info.configured_state_dir}"""
        remediation = f"""\
Running `eden restart` will cause Eden to restart and use the data from the
on-disk configuration."""
        super().__init__(msg, remediation)


class CheckoutNotMounted(FixableProblem):
    def __init__(self, checkout_info: CheckoutInfo) -> None:
        self._instance = checkout_info.instance
        self._mount_path = checkout_info.path

    def description(self) -> str:
        return f"{self._mount_path} is not currently mounted"

    def dry_run_msg(self) -> str:
        return f"Would remount {self._mount_path}"

    def start_msg(self) -> str:
        return f"Remounting {self._mount_path}"

    def perform_fix(self) -> None:
        try:
            self._instance.mount(str(self._mount_path))
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


class StaleWorkingDirectory(Problem):
    def __init__(self, msg: str) -> None:
        remediation = f"""\
Run "cd / && cd -" to update your shell's working directory."""
        super().__init__(msg, remediation)


def check_working_directory(tracker: ProblemTracker) -> None:
    problem = check_for_working_directory_problem()
    if problem:
        tracker.add_problem(problem)


def check_for_working_directory_problem() -> Optional[Problem]:
    # Report an issue if the working directory points to a stale mount point
    if working_directory_was_stale:
        msg = "Your current working directory appears to be a stale Eden mount point"
        return StaleWorkingDirectory(msg)

    # If the $PWD environment variable is set, confirm that it points our current
    # working directory.
    #
    # This helps catch problems where the current working directory has been replaced
    # with a new mount point but the user hasn't cd'ed into the new mount yet.  For
    # instance this can happen if the user cd'ed into a checkout directory before Eden
    # was running, and then started Eden.  The user will still need to cd again to see
    # the Eden checkout contents.
    pwd = os.environ.get("PWD")
    if pwd is None:
        # If $PWD isn't set we can't check anything else
        return None

    try:
        pwd_stat = os.stat(pwd)
        cwd_stat = os.stat(".")
    except Exception:
        # If we fail to stat either directory just ignore the error and don't report a
        # problem for now.  If we ever see a real issue in practice hit this scenario we
        # can add an appropriate error message at that point in time.
        #
        # We've already handled stale mounts above, and `os.stat()` checks normally
        # succeed in this case anyway.
        #
        # Users can get into situations where they don't have permissions to
        # stat the current working directory in some cases, but this should be very
        # rare and they will probably be able to identify the issue themselves in this
        # case.
        return None

    if (pwd_stat.st_dev, pwd_stat.st_ino) == (cwd_stat.st_dev, cwd_stat.st_ino):
        # Everything looks okay
        return None

    msg = """\
Your current working directory is out-of-date.
This can happen if you have (re)started Eden but your shell is still pointing to
the old directory from before the Eden checkouts were mounted.
"""
    return StaleWorkingDirectory(msg)


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
