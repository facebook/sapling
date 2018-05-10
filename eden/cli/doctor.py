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
import errno
import json
import logging
import os
import subprocess
import sys
from enum import Enum, auto
from textwrap import dedent
from typing import Dict, List, Optional, Set, TextIO, Union

import eden.dirstate

from . import config as config_mod, mtab, version
from .stdout_printer import StdoutPrinter


log = logging.getLogger("eden.cli.doctor")


class CheckResultType(Enum):
    NO_ISSUE = auto()
    FIXED = auto()
    NOT_FIXED_BECAUSE_DRY_RUN = auto()
    FAILED_TO_FIX = auto()
    NO_CHECK_BECAUSE_EDEN_WAS_NOT_RUNNING = auto()


class CheckResult:
    __slots__ = ("result_type", "message")

    def __init__(self, result_type: CheckResultType, message: str) -> None:
        self.result_type = result_type
        self.message = message


class Check(abc.ABC):

    @abc.abstractmethod
    def do_check(self, dry_run: bool) -> CheckResult:
        pass


def cure_what_ails_you(
    config: config_mod.Config,
    dry_run: bool,
    out: TextIO,
    mount_table: mtab.MountTable,
    printer: Optional[StdoutPrinter] = None,
) -> int:
    if printer is None:
        printer = StdoutPrinter()

    is_healthy = config.check_health().is_healthy()
    if not is_healthy:
        out.write(
            dedent(
                """\
        Eden is not running: cannot perform all checks.
        To start Eden, run:

            eden daemon

        """
            )
        )
        active_mount_points: List[str] = []
    else:
        with config.get_thrift_client() as client:
            active_mount_points = [
                mount.mountPoint
                for mount in client.listMounts()
                if mount.mountPoint is not None
            ]

    # This list is a mix of messages to print to stdout and checks to perform.
    checks_and_messages: List[Union[str, Check]] = [
        StaleMountsCheck(active_mount_points, mount_table)
    ]
    if is_healthy:
        checks_and_messages.append(EdenfsIsLatest(config))
    else:
        out.write(
            "Cannot check if running latest edenfs because "
            "the daemon is not running.\n"
        )

    watchman_roots = _get_watch_roots_for_watchman()
    nuclide_roots = _get_roots_for_nuclide()
    for mount_path in active_mount_points:
        if mount_path not in config.get_mount_paths():
            # TODO: if there are mounts in active_mount_points that aren't in
            # config.get_mount_paths(), should we try to add them to the config?
            # I've only seen this happen in the wild if a clone fails partway,
            # for example, if a post-clone hook fails.
            continue

        # For now, we assume that each mount_path is actively mounted. We should
        # update the listMounts() Thrift API to return information that notes
        # whether a mount point is active and use it here.
        checks: List[Union[str, Check]] = []
        checks.append(
            WatchmanUsingEdenSubscriptionCheck(mount_path, watchman_roots, is_healthy)
        )
        if nuclide_roots is not None:
            checks.append(
                NuclideHasExpectedWatchmanSubscriptions(
                    mount_path, watchman_roots, nuclide_roots
                )
            )

        client_info = config.get_client_info(mount_path)
        if client_info["scm_type"] == "hg":
            snapshot_hex = client_info["snapshot"]
            checks.append(
                SnapshotDirstateConsistencyCheck(mount_path, snapshot_hex, is_healthy)
            )

        checks_and_messages.append(
            f"Performing {len(checks)} checks for {mount_path}.\n"
        )
        checks_and_messages.extend(checks)

    num_fixes = 0
    num_failed_fixes = 0
    num_not_fixed_because_dry_run = 0
    for item in checks_and_messages:
        if isinstance(item, str):
            out.write(item)
            continue
        result = item.do_check(dry_run)
        result_type = result.result_type
        if result_type == CheckResultType.FIXED:
            num_fixes += 1
        elif result_type == CheckResultType.FAILED_TO_FIX:
            num_failed_fixes += 1
        elif result_type == CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN:
            num_not_fixed_because_dry_run += 1
        out.write(result.message)

    if num_not_fixed_because_dry_run:
        msg = (
            "Number of issues discovered during --dry-run: "
            f"{num_not_fixed_because_dry_run}."
        )
        out.write(f"{printer.yellow(msg)}\n")
    if num_fixes:
        msg = f"Number of fixes made: {num_fixes}."
        out.write(f"{printer.yellow(msg)}\n")
    if num_failed_fixes:
        msg = ("Number of issues that " f"could not be fixed: {num_failed_fixes}.")
        out.write(f"{printer.red(msg)}\n")

    if num_failed_fixes == 0 and num_not_fixed_because_dry_run == 0:
        out.write(f'{printer.green("All is well.")}\n')

    if num_failed_fixes:
        return 1
    else:
        return 0


def printable_bytes(b: bytes) -> str:
    return b.decode("utf-8", "backslashreplace")


class StaleMountsCheck(Check):

    def __init__(
        self, active_mount_points: List[str], mount_table: mtab.MountTable
    ) -> None:
        self._active_mount_points = active_mount_points
        self._mount_table = mount_table

    def do_check(self, dry_run: bool) -> CheckResult:
        for amp in self._active_mount_points:
            try:
                self._mount_table.lstat(amp).st_dev
            except OSError as e:
                # If dry_run, should this return NOT_FIXED_BECAUSE_DRY_RUN?
                return CheckResult(
                    CheckResultType.FAILED_TO_FIX,
                    f"Failed to lstat active eden mount {amp}\n",
                )

        stale_mounts = self.get_all_stale_eden_mount_points()
        if not stale_mounts:
            return CheckResult(CheckResultType.NO_ISSUE, "")

        if dry_run:
            message = (
                f"Found {len(stale_mounts)} stale edenfs mount "
                f'point{"s" if len(stale_mounts) != 1 else ""}:\n'
            )
            for mp in stale_mounts:
                message += f"  {printable_bytes(mp)}\n"
            message += "Not unmounting because dry run.\n"

            return CheckResult(CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN, message)

        unmounted = []
        failed_to_unmount = []

        # Attempt to lazy unmount all of them first. For some reason,
        # lazy unmount can sometimes release any bind mounts inside.
        for mp in stale_mounts:
            if self._mount_table.unmount_lazy(mp):
                unmounted.append(mp)

        # Use a refreshed list -- it's possible MNT_DETACH succeeded on some of
        # the points.
        for mp in self.get_all_stale_eden_mount_points():
            if self._mount_table.unmount_force(mp):
                unmounted.append(mp)
            else:
                failed_to_unmount.append(mp)

        if failed_to_unmount:
            message = ""
            if len(unmounted):
                message += (
                    f"Successfully unmounted {len(unmounted)} mount "
                    f'point{"s" if len(unmounted) != 1 else ""}:\n'
                )
                for mp in sorted(unmounted):
                    message += f"  {printable_bytes(mp)}\n"
            message += (
                f"Failed to unmount {len(failed_to_unmount)} mount "
                f'point{"s" if len(failed_to_unmount) != 1 else ""}:\n'
            )
            for mp in sorted(failed_to_unmount):
                message += f"  {printable_bytes(mp)}\n"
            return CheckResult(CheckResultType.FAILED_TO_FIX, message)
        else:
            message = (
                f"Unmounted {len(stale_mounts)} stale edenfs mount "
                f'point{"s" if len(stale_mounts) != 1 else ""}:\n'
            )
            for mp in sorted(unmounted):
                message += f"  {printable_bytes(mp)}\n"
            return CheckResult(CheckResultType.FIXED, message)

    def get_all_stale_eden_mount_points(self) -> List[bytes]:
        stale_eden_mount_points: Set[bytes] = set()
        for mount_point in self.get_all_eden_mount_points():
            try:
                # All eden mounts should have a .eden directory.
                # If the edenfs daemon serving this mount point has died we
                # will get ENOTCONN when trying to access it.  (Simply calling
                # lstat() on the root directory itself can succeed even in this
                # case.)
                eden_dir = os.path.join(mount_point, b".eden")
                self._mount_table.lstat(eden_dir)
            except OSError as e:
                if e.errno == errno.ENOTCONN:
                    stale_eden_mount_points.add(mount_point)
                else:
                    log.warning(
                        f"Unclear whether {printable_bytes(mount_point)} "
                        f"is stale or not. lstat() failed: {e}"
                    )

        return sorted(stale_eden_mount_points)

    def get_all_eden_mount_points(self) -> Set[bytes]:
        all_system_mounts = self._mount_table.read()
        return {
            mount.mount_point
            for mount in all_system_mounts
            if mount.device == b"edenfs" and mount.vfstype == b"fuse"
        }


class WatchmanUsingEdenSubscriptionCheck(Check):

    def __init__(self, path: str, watchman_roots: Set[str], is_healthy: bool) -> None:
        self._path = path
        self._watchman_roots = watchman_roots
        self._is_healthy = is_healthy
        self._watcher = None

    def do_check(self, dry_run: bool) -> CheckResult:
        if not self._is_healthy:
            return self._report(CheckResultType.NO_CHECK_BECAUSE_EDEN_WAS_NOT_RUNNING)
        if self._path not in self._watchman_roots:
            return self._report(CheckResultType.NO_ISSUE)

        watch_details = _call_watchman(["watch-project", self._path])
        self._watcher = watch_details.get("watcher")
        if self._watcher == "eden":
            return self._report(CheckResultType.NO_ISSUE)

        # At this point, we know there is an issue that needs to be fixed.
        if dry_run:
            return self._report(CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN)

        # Delete the old watch and try to re-establish it. Hopefully it will be
        # an Eden watch this time.
        _call_watchman(["watch-del", self._path])
        watch_details = _call_watchman(["watch-project", self._path])
        if watch_details.get("watcher") == "eden":
            return self._report(CheckResultType.FIXED)
        else:
            return self._report(CheckResultType.FAILED_TO_FIX)

    def _report(self, result_type: CheckResultType) -> CheckResult:
        old_watcher = self._watcher or "(unknown)"
        if result_type == CheckResultType.FIXED:
            msg = (
                f"Previous Watchman watcher for {self._path} was "
                f'"{old_watcher}" but is now "eden".\n'
            )
        elif result_type == CheckResultType.FAILED_TO_FIX:
            msg = (
                f"Watchman Watcher for {self._path} was {old_watcher} "
                'and we failed to replace it with an "eden" watcher.\n'
            )
        elif result_type == CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN:
            msg = (
                f"Watchman Watcher for {self._path} was {old_watcher} "
                "but nothing was done because --dry-run was specified.\n"
            )
        else:
            msg = ""
        return CheckResult(result_type, msg)


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


class NuclideHasExpectedWatchmanSubscriptions(Check):

    def __init__(
        self, path: str, watchman_roots: Set[str], nuclide_roots: Set[str]
    ) -> None:
        self._path = path
        self._watchman_roots = watchman_roots
        self._nuclide_roots = nuclide_roots
        self._missing_subscriptions = []
        self._connected_nuclide_roots = None

    def do_check(self, dry_run: bool) -> CheckResult:
        # Note that self._nuclide_roots is a set, but each entry in the set
        # could appear as a root folder multiple times if the user uses multiple
        # Atom windows.
        path_prefix = self._path + "/"
        connected_nuclide_roots = [
            nuclide_root
            for nuclide_root in self._nuclide_roots
            if self._path == nuclide_root or nuclide_root.startswith(path_prefix)
        ]
        self._connected_nuclide_roots = connected_nuclide_roots
        if not connected_nuclide_roots:
            # There do not appear to be any Nuclide connections for self._path.
            return self._report(CheckResultType.NO_ISSUE)

        subscriptions = _call_watchman(["debug-get-subscriptions", self._path])
        subscribers = subscriptions.get("subscribers", [])
        subscription_counts = {}
        for subscriber in subscribers:
            info = subscriber.get("info", {})
            name = info.get("name")
            if name is None:
                continue
            elif name in subscription_counts:
                subscription_counts[name] += 1
            else:
                subscription_counts[name] = 1

        for nuclide_root in connected_nuclide_roots:
            filewatcher_subscription = f"filewatcher-{nuclide_root}"
            # Note that even if the user has `nuclide_root` opened in multiple
            # Nuclide windows, the Nuclide server should not create the
            # "filewatcher-" subscription multiple times.
            if subscription_counts.get(filewatcher_subscription) != 1:
                self._missing_subscriptions.append(filewatcher_subscription)

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
                self._missing_subscriptions.append(hg_subscription)

        if not self._missing_subscriptions:
            return self._report(CheckResultType.NO_ISSUE)
        elif dry_run:
            return self._report(CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN)
        else:
            return self._report(CheckResultType.FAILED_TO_FIX)

    def _report(self, result_type: CheckResultType) -> CheckResult:
        if result_type in [
            CheckResultType.FAILED_TO_FIX, CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN
        ]:

            def format_paths(paths):
                return "\n".join(map(lambda x: f"  {x}", paths))

            msg = (
                "Nuclide appears to be used to edit the following directories\n"
                f"under {self._path}:\n\n"
                f"{format_paths(self._connected_nuclide_roots)}\n\n"
                "but the following Watchman subscriptions appear to be missing:\n\n"
                f"{format_paths(self._missing_subscriptions)}\n\n"
                "This can cause file changes to fail to show up in Nuclide.\n"
                "Currently, the only workaround for this is to run\n"
                '"Nuclide Remote Projects: Kill And Restart" from the\n'
                "command palette in Atom.\n"
            )
        else:
            msg = ""
        return CheckResult(result_type, msg)


class SnapshotDirstateConsistencyCheck(Check):

    def __init__(self, path: str, snapshot_hex: str, is_healthy: bool) -> None:
        self._path = path
        self._snapshot_hex = snapshot_hex
        self._is_healthy = is_healthy

    def do_check(self, dry_run: bool) -> CheckResult:
        if not self._is_healthy:
            return self._report(CheckResultType.NO_CHECK_BECAUSE_EDEN_WAS_NOT_RUNNING)

        dirstate = os.path.join(self._path, ".hg", "dirstate")
        with open(dirstate, "rb") as f:
            parents, _tuples_dict, _copymap = eden.dirstate.read(f, dirstate)
        p1 = parents[0]
        self._p1_hex = binascii.hexlify(p1).decode("utf-8")

        if self._snapshot_hex == self._p1_hex:
            return self._report(CheckResultType.NO_ISSUE)
        else:
            return self._report(CheckResultType.FAILED_TO_FIX)

    def _report(self, result_type: CheckResultType) -> CheckResult:
        if result_type == CheckResultType.FAILED_TO_FIX:
            msg = (
                f"p1 for {self._path} is {self._p1_hex}, but Eden's internal\n"
                f"hash in its SNAPSHOT file is {self._snapshot_hex}.\n"
            )
        else:
            msg = ""
        return CheckResult(result_type, msg)


class EdenfsIsLatest(Check):

    def __init__(self, config) -> None:
        self._config = config

    def do_check(self, dry_run: bool) -> CheckResult:
        rver, release = version.get_running_eden_version_parts(self._config)
        if not rver or not release:
            # This could be a dev build that returns the empty
            # string for both of these values.
            return CheckResult(CheckResultType.NO_ISSUE, "")

        running_version = version.format_running_eden_version((rver, release))
        installed_version = version.get_installed_eden_rpm_version()
        if running_version == installed_version:
            return CheckResult(CheckResultType.NO_ISSUE, "")
        else:
            return CheckResult(
                CheckResultType.FAILED_TO_FIX,
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
            )


def _get_watch_roots_for_watchman() -> Set[str]:
    js = _call_watchman(["watch-list"])
    roots = set(js["roots"])
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


def _check_json_output(args: List[str]) -> Dict:
    """Calls subprocess.check_output() and returns the output parsed as JSON.
    If the call fails, it will write the error to stderr and return a dict with
    a single property named "error".
    """
    try:
        output = subprocess.check_output(args)
        return json.loads(output)
    except (subprocess.CalledProcessError, ValueError) as e:
        # CalledProcessError if check_output() fails.
        # ValueError if `output` is not valid JSON.
        sys.stderr.write(
            f'Calling `{" ".join(args)}`'
            f" failed with: {str(e) if e.strerror is None else e.strerror}\n"
        )
        return {"error": str(e)}
