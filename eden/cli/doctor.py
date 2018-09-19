#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import binascii
import collections
import errno
import io
import json
import logging
import os
import stat
import subprocess
import typing
from enum import IntEnum
from textwrap import dedent
from typing import Any, Dict, List, Optional, Set, Tuple

import eden.dirstate
import facebook.eden.ttypes as eden_ttypes

from . import config as config_mod, filesystem, mtab, ui, version
from .config import EdenInstance


log = logging.getLogger("eden.cli.doctor")

# working_directory_was_stale may be set to True by the CLI main module
# if the original working directory referred to a stale eden mount point.
working_directory_was_stale = False


class RemediationError(Exception):
    pass


class ProblemSeverity(IntEnum):
    # Note that we intentionally want to be able to compare severity values
    # using < and > operators.
    ADVICE = 3
    ERROR = 10


class ProblemBase(abc.ABC):
    @abc.abstractmethod
    def description(self) -> str:
        "Return the description of this problem."

    def severity(self) -> ProblemSeverity:
        """Return the problem severity.

        Defaults to ERROR if not overridden.
        """
        return ProblemSeverity.ERROR

    def get_manual_remediation_message(self) -> Optional[str]:
        "Return a message explaining how to manually fix this problem."
        return None


class FixableProblem(ProblemBase):
    @abc.abstractmethod
    def dry_run_msg(self) -> str:
        """Return a string to print for dry-run operations."""

    @abc.abstractmethod
    def start_msg(self) -> str:
        """Return a string to print when starting the remediation."""

    @abc.abstractmethod
    def perform_fix(self) -> None:
        """Attempt to automatically fix the problem."""


class Problem(ProblemBase):
    def __init__(
        self,
        description: str,
        remediation: Optional[str] = None,
        severity: ProblemSeverity = ProblemSeverity.ERROR,
    ) -> None:
        self._description = description
        self._remediation = remediation
        self._severity = severity

    def description(self) -> str:
        return self._description

    def severity(self) -> ProblemSeverity:
        return self._severity

    def get_manual_remediation_message(self) -> Optional[str]:
        return self._remediation


class ProblemTracker(abc.ABC):
    def add_problem(self, problem: ProblemBase) -> None:
        """Record a new problem"""


class ProblemFixer(ProblemTracker):
    def __init__(self, out: ui.Output) -> None:
        self._out = out
        self.num_problems = 0
        self.num_fixed_problems = 0
        self.num_failed_fixes = 0
        self.num_manual_fixes = 0

    def add_problem(self, problem: ProblemBase) -> None:
        self.num_problems += 1
        self._out.writeln("- Found problem:", fg=self._out.YELLOW)
        self._out.writeln(problem.description())
        if isinstance(problem, FixableProblem):
            self.fix_problem(problem)
        else:
            self.num_manual_fixes += 1
            msg = problem.get_manual_remediation_message()
            if msg:
                self._out.write(msg, end="\n\n")

    def fix_problem(self, problem: FixableProblem) -> None:
        self._out.write(f"{problem.start_msg()}...", flush=True)
        try:
            problem.perform_fix()
            self._out.write("fixed", fg=self._out.GREEN, end="\n\n", flush=True)
            self.num_fixed_problems += 1
        except Exception as ex:
            self._out.writeln("error", fg=self._out.RED)
            self._out.write(f"Failed to fix problem: {ex}", end="\n\n", flush=True)
            self.num_failed_fixes += 1


class DryRunFixer(ProblemFixer):
    def fix_problem(self, problem: FixableProblem) -> None:
        self._out.write(problem.dry_run_msg(), end="\n\n")


def cure_what_ails_you(
    instance: EdenInstance,
    dry_run: bool,
    mount_table: mtab.MountTable,
    fs_util: filesystem.FsUtil,
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

    watchman_roots = _get_watch_roots_for_watchman()
    nuclide_roots = _get_roots_for_nuclide()

    configured_mounts = instance.get_mount_paths()
    for mount_path in sorted(configured_mounts):
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
        check_watchman_subscriptions(tracker, mount_path, watchman_roots)
        check_bind_mounts(
            tracker, mount_path, instance, client_info, mount_table, fs_util
        )

        if nuclide_roots is not None:
            check_nuclide_watchman_subscriptions(
                tracker, mount_path, watchman_roots, nuclide_roots
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


def check_watchman_subscriptions(
    tracker: ProblemTracker, path: str, watchman_roots: Set[str]
) -> None:
    if path not in watchman_roots:
        return

    watch_details = _call_watchman(["watch-project", path])
    watcher = watch_details.get("watcher")
    if watcher == "eden":
        return

    tracker.add_problem(IncorrectWatchmanWatch(path, watcher))


class IncorrectWatchmanWatch(FixableProblem):
    def __init__(self, path: str, watcher: Any) -> None:
        self._path = path
        self._watcher = watcher

    def description(self) -> str:
        return (
            f"Watchman is watching {self._path} with the wrong watcher type: "
            f'"{self._watcher}" instead of "eden"'
        )

    def dry_run_msg(self) -> str:
        return f"Would fix watchman watch for {self._path}"

    def start_msg(self) -> str:
        return f"Fixing watchman watch for {self._path}"

    def perform_fix(self) -> None:
        # Delete the old watch and try to re-establish it. Hopefully it will be
        # an Eden watch this time.
        _call_watchman(["watch-del", self._path])
        watch_details = _call_watchman(["watch-project", self._path])
        if watch_details.get("watcher") != "eden":
            raise RemediationError(
                f"Failed to replace watchman watch for {self._path} "
                'with an "eden" watcher'
            )


# Watchman subscriptions that Nuclide creates for an Hg repository.
NUCLIDE_HG_SUBSCRIPTIONS = [
    "hg-repository-watchman-subscription-primary",
    "hg-repository-watchman-subscription-conflicts",
    "hg-repository-watchman-subscription-hgbookmark",
    "hg-repository-watchman-subscription-hgbookmarks",
    "hg-repository-watchman-subscription-dirstate",
    "hg-repository-watchman-subscription-progress",
    "hg-repository-watchman-subscription-lock-files",
]


def check_nuclide_watchman_subscriptions(
    tracker: ProblemTracker,
    path: str,
    watchman_roots: Set[str],
    nuclide_roots: Set[str],
) -> None:
    # Note that nuclide_roots is a set, but each entry in the set
    # could appear as a root folder multiple times if the user uses multiple
    # Atom windows.
    path_prefix = path + "/"
    connected_nuclide_roots = [
        nuclide_root
        for nuclide_root in nuclide_roots
        if path == nuclide_root or nuclide_root.startswith(path_prefix)
    ]
    if not connected_nuclide_roots:
        # There do not appear to be any Nuclide connections for path.
        return

    subscriptions = _call_watchman(["debug-get-subscriptions", path])
    subscribers = subscriptions.get("subscribers", [])
    subscription_counts: Dict[str, int] = {}
    for subscriber in subscribers:
        info = subscriber.get("info", {})
        name = info.get("name")
        if name is None:
            continue
        elif name in subscription_counts:
            subscription_counts[name] += 1
        else:
            subscription_counts[name] = 1

    missing_or_duplicate_subscriptions = []
    for nuclide_root in connected_nuclide_roots:
        filewatcher_subscription = f"filewatcher-{nuclide_root}"
        # Note that even if the user has `nuclide_root` opened in multiple
        # Nuclide windows, the Nuclide server should not create the
        # "filewatcher-" subscription multiple times.
        if subscription_counts.get(filewatcher_subscription) != 1:
            missing_or_duplicate_subscriptions.append(filewatcher_subscription)

    # Today, Nuclide creates a number of Watchman subscriptions per root
    # folder that is under an Hg working copy. (It should probably
    # consolidate these subscriptions, though it will take some work to
    # refactor things to do that.) Because each of connected_nuclide_roots
    # is a root folder in at least one Atom window, there must be at least
    # as many instances of each subscription as there are
    # connected_nuclide_roots.
    #
    # TODO(mbolin): Come up with a more stable contract than including a
    # hardcoded list of Nuclide subscription names in here because Eden and
    # Nuclide releases are not synced. This is admittedly a stopgap measure:
    # the primary objective is to figure out how Eden/Nuclide gets into
    # this state to begin with and prevent it.
    #
    # Further, Nuclide should probably rename these subscriptions so that:
    # (1) It is clear that Nuclide is the one who created the subscription.
    # (2) The subscription can be ascribed to an individual Nuclide client
    #     if we are going to continue to create the same subscription
    #     multiple times.
    num_roots = len(connected_nuclide_roots)
    for hg_subscription in NUCLIDE_HG_SUBSCRIPTIONS:
        if subscription_counts.get(hg_subscription, 0) < num_roots:
            missing_or_duplicate_subscriptions.append(hg_subscription)

    if missing_or_duplicate_subscriptions:

        def format_paths(paths: List[str]) -> str:
            return "\n  ".join(paths)

        missing_subscriptions = [
            sub
            for sub in missing_or_duplicate_subscriptions
            if 0 == subscription_counts.get(sub, 0)
        ]
        duplicate_subscriptions = [
            sub
            for sub in missing_or_duplicate_subscriptions
            if 1 < subscription_counts.get(sub, 0)
        ]

        output = io.StringIO()
        output.write(
            "Nuclide appears to be used to edit the following directories\n"
            f"under {path}:\n\n"
            f"  {format_paths(connected_nuclide_roots)}\n\n"
        )
        if missing_subscriptions:
            output.write(
                "but the following Watchman subscriptions appear to be missing:\n\n"
                f"  {format_paths(missing_subscriptions)}\n\n"
            )
        if duplicate_subscriptions:
            conj = "and" if missing_subscriptions else "but"
            output.write(
                f"{conj} the following Watchman subscriptions have duplicates:\n\n"
                f"  {format_paths(duplicate_subscriptions)}\n\n"
            )
        output.write(
            "This can cause file changes to fail to show up in Nuclide.\n"
            "Currently, the only workaround for this is to run\n"
            '"Nuclide Remote Projects: Kill And Restart" from the\n'
            "command palette in Atom.\n"
        )
        tracker.add_problem(Problem(output.getvalue()))


def check_snapshot_dirstate_consistency(
    tracker: ProblemTracker, instance: EdenInstance, path: str, snapshot_hex: str
) -> None:
    dirstate = os.path.join(path, ".hg", "dirstate")
    try:
        with open(dirstate, "rb") as f:
            parents, _tuples_dict, _copymap = eden.dirstate.read(f, dirstate)
    except OSError as ex:
        if ex.errno == errno.ENOENT:
            tracker.add_problem(MissingHgDirectory(path))
        else:
            tracker.add_problem(Problem(f"Unable to access {path}/.hg/dirstate: {ex}"))
        return

    p1_hex = binascii.hexlify(parents[0]).decode("utf-8")
    if snapshot_hex != p1_hex:
        tracker.add_problem(
            SnapshotMismatchError(instance, path, snapshot_hex, parents)
        )


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


def _get_watch_roots_for_watchman() -> Set[str]:
    js = _call_watchman(["watch-list"])
    roots = set(js.get("roots", []))
    return roots


def _call_watchman(args: List[str]) -> Dict:
    full_args = ["watchman"]
    full_args.extend(args)
    return _check_json_output(full_args)


def _get_roots_for_nuclide() -> Optional[Set[str]]:
    connections = _check_json_output(["nuclide-connections"])
    if isinstance(connections, list):
        return set(connections)
    else:
        # connections should be a dict with an "error" property.
        return None


def _check_json_output(args: List[str]) -> Dict[str, Any]:
    """Calls subprocess.check_output() and returns the output parsed as JSON.
    If the call fails, it will write the error to stderr and return a dict with
    a single property named "error".
    """
    try:
        output = subprocess.check_output(args)
        return typing.cast(Dict[str, Any], json.loads(output))
    except Exception as e:
        # FileNotFoundError if the command is not found.
        # CalledProcessError if the command exits unsuccessfully.
        # ValueError if `output` is not valid JSON.
        errstr = getattr(e, "strerror", str(e))
        log.warning(f'Calling `{" ".join(args)}` failed with: {errstr}')
        return {"error": str(e)}
