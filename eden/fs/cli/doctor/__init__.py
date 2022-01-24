#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import shlex
import sys
from datetime import datetime, date
from pathlib import Path
from textwrap import dedent
from typing import Dict, Optional

from eden.fs.cli import (
    config as config_mod,
    filesystem,
    mtab,
    proc_utils as proc_utils_mod,
    ui,
    version,
)
from eden.fs.cli.config import EdenCheckout, EdenInstance
from eden.fs.cli.doctor.util import CheckoutInfo
from facebook.eden.ttypes import MountState
from fb303_core.ttypes import fb303_status

from . import (
    check_filesystems,
    check_hg,
    check_kerberos,
    check_os,
    check_redirections,
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

try:
    from .facebook import check_facebook
except ImportError:
    from typing import Any

    def check_facebook(*_args: Any, **_kwargs: Any) -> None:
        pass


# working_directory_was_stale may be set to True by the CLI main module
# if the original working directory referred to a stale eden mount point.
working_directory_was_stale = False


def cure_what_ails_you(
    instance: EdenInstance,
    dry_run: bool,
    mount_table: Optional[mtab.MountTable] = None,
    fs_util: Optional[filesystem.FsUtil] = None,
    proc_utils: Optional[proc_utils_mod.ProcUtils] = None,
    kerberos_checker: Optional[check_kerberos.KerberosChecker] = None,
    out: Optional[ui.Output] = None,
) -> int:
    return EdenDoctor(
        instance, dry_run, mount_table, fs_util, proc_utils, kerberos_checker, out
    ).cure_what_ails_you()


class EdenDoctorChecker:
    """EdenDoctorChecker is a base class for EdenDoctor, and only supports
    running checks, without reporting or fixing problems.
    """

    instance: EdenInstance
    mount_table: mtab.MountTable
    fs_util: filesystem.FsUtil
    proc_utils: proc_utils_mod.ProcUtils
    kerberos_checker: check_kerberos.KerberosChecker
    tracker: ProblemTracker
    out: ui.Output
    # Setting run_system_wide_checks to False causes EdenDoctor to skip checks that
    # try to detect system-wide problems (e.g., stale mount points, old OS/kernel
    # version, low disk space, etc.).  It is often desirable to disable this during
    # integration tests, since tests normally only want to report issues with their
    # specific EdenFS instance under test.
    run_system_wide_checks: bool = True

    def __init__(
        self,
        instance: EdenInstance,
        tracker: ProblemTracker,
        mount_table: Optional[mtab.MountTable] = None,
        fs_util: Optional[filesystem.FsUtil] = None,
        proc_utils: Optional[proc_utils_mod.ProcUtils] = None,
        kerberos_checker: Optional[check_kerberos.KerberosChecker] = None,
        out: Optional[ui.Output] = None,
    ) -> None:
        self.instance = instance
        self.tracker = tracker
        self.mount_table = mount_table if mount_table is not None else mtab.new()
        self.fs_util = fs_util if fs_util is not None else filesystem.new()
        self.proc_utils = proc_utils if proc_utils is not None else proc_utils_mod.new()
        self.kerberos_checker = (
            kerberos_checker
            if kerberos_checker is not None
            else check_kerberos.KerberosChecker()
        )
        self.out = out if out is not None else ui.get_output()

    def run_checks(self) -> None:
        self.check_working_directory()

        if self.run_system_wide_checks:
            # check OS type, kernel version etc.
            check_os.run_operating_system_checks(self.tracker, self.instance, self.out)

            # check multiple edenfs running with some rogue stale PIDs
            check_rogue_edenfs.check_many_edenfs_are_running(
                self.tracker, self.proc_utils
            )

            self.kerberos_checker.run_kerberos_certificate_checks(
                self.instance, self.tracker
            )

        status = self.instance.check_health()
        if status.status == fb303_status.ALIVE:
            self.run_normal_checks()
        elif status.status == fb303_status.STARTING:
            self.tracker.add_problem(EdenfsStarting())
        elif status.status == fb303_status.STOPPING:
            self.tracker.add_problem(EdenfsStopping())
        elif status.status == fb303_status.DEAD:
            self.run_edenfs_not_healthy_checks()
        else:
            self.tracker.add_problem(EdenfsUnexpectedStatus(status))

    def check_working_directory(self) -> None:
        problem = check_for_working_directory_problem()
        if problem:
            self.tracker.add_problem(problem)

    def run_edenfs_not_healthy_checks(self) -> None:
        configured_mounts = self.instance.get_mount_paths()
        if configured_mounts:
            self.tracker.add_problem(EdenfsNotHealthy())
        else:
            self.tracker.using_edenfs = False
            return

        if self.run_system_wide_checks:
            check_stale_mounts.check_for_stale_mounts(self.tracker, self.mount_table)
            check_filesystems.check_disk_usage(
                self.tracker,
                list(configured_mounts),
                self.instance,
                fs_util=self.fs_util,
            )

    def _get_checkouts_info(self) -> Dict[Path, CheckoutInfo]:
        checkouts: Dict[Path, CheckoutInfo] = {}
        # Get information about the checkouts currently known to the running
        # edenfs process
        with self.instance.get_thrift_client_legacy() as client:
            for mount in client.listMounts():
                # Old versions of edenfs did not return a mount state field.
                # These versions only listed running mounts, so treat the mount state
                # as running in this case.
                mount_state = (
                    mount.state if mount.state is not None else MountState.RUNNING
                )
                path = Path(os.fsdecode(mount.mountPoint))
                checkout = CheckoutInfo(
                    self.instance,
                    path,
                    running_state_dir=Path(os.fsdecode(mount.edenClientPath)),
                    state=mount_state,
                )
                checkouts[path] = checkout

        # Get information about the checkouts listed in the config file
        for configured_checkout in self.instance.get_checkouts():
            checkout_info = checkouts.get(configured_checkout.path, None)
            if checkout_info is None:
                checkout_info = CheckoutInfo(self.instance, configured_checkout.path)
                checkout_info.configured_state_dir = configured_checkout.state_dir
                checkouts[checkout_info.path] = checkout_info

            checkout_info.configured_state_dir = configured_checkout.state_dir

        return checkouts

    def check_privhelper(self) -> None:
        try:
            connected = self.instance.check_privhelper_connection()
            if not connected:
                self.tracker.add_problem(EdenfsPrivHelperNotHealthy())
        except Exception as ex:
            # This check is only reached after we've determined that the EdenFS
            # daemon is healthy, so any error thrown while calling into Thrift
            # would be unexpected.
            self.tracker.add_problem(
                Problem(f"Unexpected error while checking PrivHelper: {ex}")
            )

    def run_normal_checks(self) -> None:
        check_edenfs_version(self.tracker, self.instance)
        checkouts = self._get_checkouts_info()

        if sys.platform != "win32":
            self.check_privhelper()

        if self.run_system_wide_checks:
            check_filesystems.check_eden_directory(self.tracker, self.instance)
            check_stale_mounts.check_for_stale_mounts(self.tracker, self.mount_table)
            check_filesystems.check_disk_usage(
                self.tracker,
                list(self.instance.get_mount_paths()),
                self.instance,
                fs_util=self.fs_util,
            )
            check_facebook(
                self.tracker,
                check_fuse=any(
                    checkout.get_checkout().get_config().mount_protocol == "fuse"
                    for checkout in checkouts.values()
                ),
            )

        watchman_info = check_watchman.pre_check()

        for path, checkout in sorted(checkouts.items()):
            self.out.writeln(f"Checking {path}")
            try:
                check_mount(
                    self.tracker,
                    self.instance,
                    checkout,
                    self.mount_table,
                    watchman_info,
                )
            except Exception as ex:
                self.tracker.add_problem(
                    Problem(f"unexpected error while checking {path}: {ex}")
                )


class EdenDoctor(EdenDoctorChecker):
    fixer: ProblemFixer
    dry_run: bool

    def __init__(
        self,
        instance: EdenInstance,
        dry_run: bool,
        mount_table: Optional[mtab.MountTable] = None,
        fs_util: Optional[filesystem.FsUtil] = None,
        proc_utils: Optional[proc_utils_mod.ProcUtils] = None,
        kerberos_checker: Optional[check_kerberos.KerberosChecker] = None,
        out: Optional[ui.Output] = None,
    ) -> None:
        self.dry_run = dry_run
        out = out if out is not None else ui.get_output()
        if dry_run:
            self.fixer = DryRunFixer(out)
        else:
            self.fixer = ProblemFixer(out)

        super().__init__(
            instance,
            tracker=self.fixer,
            mount_table=mount_table,
            fs_util=fs_util,
            proc_utils=proc_utils,
            kerberos_checker=kerberos_checker,
            out=out,
        )

    def cure_what_ails_you(self) -> int:
        self.run_checks()
        return self._report_problems()

    def _report_problems(self) -> int:
        fixer = self.fixer
        out = self.out

        if not self.dry_run:
            self.instance.log_sample(
                "eden_doctor",
                num_problems=fixer.num_problems,
                problems=fixer.problem_types,
            )

        if fixer.num_problems == 0:
            if not fixer.using_edenfs:
                out.writeln("EdenFS is not in use.")
            else:
                out.writeln("No issues detected.", fg=out.GREEN)
            return 0

        def problem_count(num: int) -> str:
            if num == 1:
                return "1 problem"
            return f"{num} problems"

        if self.dry_run:
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
                msg = "1 issue requires manual attention."
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


class EdenfsNotHealthy(Problem):
    def __init__(self) -> None:
        super().__init__(
            "Eden is not running.", remediation="To start Eden, run:\n\n    eden start"
        )


class EdenfsPrivHelperNotHealthy(Problem):
    def __init__(self) -> None:
        super().__init__(
            "The PrivHelper process is not accessible.",
            remediation="To restore the connection to the PrivHelper, run `eden restart`",
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


def check_mount(
    tracker: ProblemTracker,
    instance: EdenInstance,
    checkout: CheckoutInfo,
    mount_table: mtab.MountTable,
    watchman_info: check_watchman.WatchmanCheckInfo,
) -> None:
    if sys.platform == "win32":
        check_mount_overlay_type(tracker, checkout)

    if checkout.state is None:
        # This checkout is configured but not currently running.
        tracker.add_problem(CheckoutNotMounted(checkout))
    elif checkout.state == MountState.RUNNING:
        check_running_mount(tracker, instance, checkout, mount_table, watchman_info)
    elif checkout.state in (
        MountState.UNINITIALIZED,
        MountState.INITIALIZING,
        MountState.INITIALIZED,
        MountState.STARTING,
    ):
        tracker.add_problem(
            Problem(
                f"Checkout {checkout.path} is currently starting up.",
                "If this checkout does not successfully finish starting soon, "
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
                "If this checkout does not successfully finish shutting down soon, "
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
    # Check if this checkout is nested inside another one
    existing_checkout, rel_path = config_mod.detect_nested_checkout(
        checkout.path,
        instance,
    )
    if existing_checkout is not None and rel_path is not None:
        tracker.add_problem(
            Problem(
                f"""\
edenfs reports that checkout {checkout.path} is nested within an existing checkout {existing_checkout.path}
Nested checkouts are usually not intended and can cause spurious behavior.\n"""
            )
        )


def check_mount_overlay_type(
    tracker: ProblemTracker, checkout_info: CheckoutInfo
) -> None:
    config = checkout_info.get_checkout().get_config()
    if not config.enable_tree_overlay:
        tracker.add_problem(CheckoutLegacyOverlayType(checkout_info))


def check_running_mount(
    tracker: ProblemTracker,
    instance: EdenInstance,
    checkout_info: CheckoutInfo,
    mount_table: mtab.MountTable,
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
    check_redirections.check_redirections(tracker, instance, checkout, mount_table)
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
        remediation = """\
Running `eden restart` will cause Eden to restart and use the data from the
on-disk configuration."""
        super().__init__(msg, remediation)


class CheckoutLegacyOverlayType(Problem):
    def __init__(self, checkout_info: CheckoutInfo) -> None:
        msg = f"""\
Your checkout '{checkout_info.path}' is still using the legacy version of
overlay which will be deprecated soon.
"""
        remediation = f"""\
Please reclone your repository. You can do so by running `fbclone <repo_type>
{checkout_info.path} --eden --reclone` or do it manually."""
        super().__init__(msg, remediation)


class CheckoutNotMounted(FixableProblem):
    _instance: EdenInstance
    _mount_path: Path

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
            self._instance.mount(str(self._mount_path), False)
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
    def date_from_version(version: str) -> date:
        return datetime.strptime(version, "%Y%m%d").date()

    rver, release = instance.get_running_version_parts()
    if not rver or not release:
        # This could be a dev build that returns the empty
        # string for both of these values.
        return

    # get installed version parts
    iversion, irelease = version.get_current_version_parts()
    if not iversion or not irelease:
        # dev build of eden client returns empty strings here
        return

    # check if the runnig version is more than two weeks old
    daysgap = date_from_version(iversion) - date_from_version(rver)
    if daysgap.days < 14:
        return

    running_version = version.format_eden_version((rver, release))
    installed_version = version.format_eden_version((iversion, irelease))

    if sys.platform == "win32":
        help_string = f"""\
The version of EdenFS that is installed on your machine is:
    fb.eden {installed_version}
but the version of EdenFS that is currently running is:
    fb.eden {running_version}

Consider running `edenfsctl restart` to migrate to the newer version,
which may have important bug fixes or performance improvements.
"""
    else:
        help_string = f"""\
The version of EdenFS that is installed on your machine is:
    fb-eden-{installed_version}.x86_64
but the version of EdenFS that is currently running is:
    fb-eden-{running_version}.x86_64

Consider running `edenfsctl restart --graceful` to migrate to the newer version,
which may have important bug fixes or performance improvements.
"""
    tracker.add_problem(Problem(dedent(help_string), severity=ProblemSeverity.ADVICE))
