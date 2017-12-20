#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import json
import os
import subprocess
import sys
import eden.dirstate
from enum import Enum, auto
from textwrap import dedent
from typing import Dict, List, Set, TextIO
from . import config as config_mod


class CheckResultType(Enum):
    NO_ISSUE = auto()
    FIXED = auto()
    NOT_FIXED_BECAUSE_DRY_RUN = auto()
    FAILED_TO_FIX = auto()
    NO_CHECK_BECAUSE_EDEN_WAS_NOT_RUNNING = auto()


class CheckResult:
    __slots__ = ('result_type', 'message')

    def __init__(self, result_type: CheckResultType, message: str):
        self.result_type = result_type
        self.message = message


def cure_what_ails_you(
    config: config_mod.Config, dry_run: bool, out: TextIO
) -> int:
    mount_paths = config.get_mount_paths()
    if len(mount_paths) == 0:
        out.write('No mounts points to assess.\n')
        return 1

    is_healthy = config.check_health().is_healthy()
    if not is_healthy:
        out.write(
            dedent(
                '''\
        Eden is not running: cannot perform all checks.
        To start Eden, run:

            eden daemon

        '''
            )
        )

    # This list is a mix of messages to print to stdout and checks to perform.
    checks_and_messages = []
    if is_healthy:
        checks_and_messages.append(EdenfsIsLatest(config))
    else:
        out.write(
            'Cannot check if running latest edenfs because '
            'the daemon is not running.\n'
        )

    watchman_roots = _get_watch_roots_for_watchman()
    for mount_path in mount_paths:
        # For now, we assume that each mount_path is actively mounted. We should
        # update the listMounts() Thrift API to return information that notes
        # whether a mount point is active and use it here.
        checks = []
        checks.append(
            WatchmanUsingEdenSubscriptionCheck(
                mount_path, watchman_roots, is_healthy
            )
        )
        checks.append(
            NuclideHasExpectedWatchmanSubscriptions(
                mount_path, watchman_roots, is_healthy
            )
        )

        client_info = config.get_client_info(mount_path)
        if client_info['scm_type'] == 'hg':
            snapshot_hex = client_info['snapshot']
            checks.append(
                SnapshotDirstateConsistencyCheck(
                    mount_path, snapshot_hex, is_healthy
                )
            )

        checks_and_messages.append(
            f'Performing {len(checks)} checks for {mount_path}.\n'
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
        out.write(
            'Number of issues discovered during --dry-run: '
            f'{num_not_fixed_because_dry_run}.\n'
        )
    if num_fixes:
        out.write(f'Number of fixes made: {num_fixes}.\n')
    if num_failed_fixes:
        out.write(
            'Number of issues that '
            f'could not be fixed: {num_failed_fixes}.\n'
        )

    if num_failed_fixes == 0 and num_not_fixed_because_dry_run == 0:
        out.write('All is well.\n')

    if num_failed_fixes:
        return 1
    else:
        return 0


class WatchmanUsingEdenSubscriptionCheck:
    def __init__(self, path: str, watchman_roots: Set[str],
                 is_healthy: bool) -> None:
        self._path = path
        self._watchman_roots = watchman_roots
        self._is_healthy = is_healthy
        self._watcher = None

    def do_check(self, dry_run: bool) -> CheckResult:
        if not self._is_healthy:
            return self._report(
                CheckResultType.NO_CHECK_BECAUSE_EDEN_WAS_NOT_RUNNING
            )
        if self._path not in self._watchman_roots:
            return self._report(CheckResultType.NO_ISSUE)

        watch_details = _call_watchman(['watch-project', self._path])
        self._watcher = watch_details.get('watcher')
        if self._watcher == 'eden':
            return self._report(CheckResultType.NO_ISSUE)

        # At this point, we know there is an issue that needs to be fixed.
        if dry_run:
            return self._report(CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN)

        # Delete the old watch and try to re-establish it. Hopefully it will be
        # an Eden watch this time.
        _call_watchman(['watch-del', self._path])
        watch_details = _call_watchman(['watch-project', self._path])
        if watch_details.get('watcher') == 'eden':
            return self._report(CheckResultType.FIXED)
        else:
            return self._report(CheckResultType.FAILED_TO_FIX)

    def _report(self, result_type: CheckResultType) -> CheckResult:
        old_watcher = self._watcher or '(unknown)'
        if result_type == CheckResultType.FIXED:
            msg = (
                f'Previous Watchman watcher for {self._path} was '
                f'"{old_watcher}" but is now "eden".\n'
            )
        elif result_type == CheckResultType.FAILED_TO_FIX:
            msg = (
                f'Watchman Watcher for {self._path} was {old_watcher} '
                'and we failed to replace it with an "eden" watcher.\n'
            )
        elif result_type == CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN:
            msg = (
                f'Watchman Watcher for {self._path} was {old_watcher} '
                'but nothing was done because --dry-run was specified.\n'
            )
        else:
            msg = ''
        return CheckResult(result_type, msg)


class NuclideHasExpectedWatchmanSubscriptions:
    def __init__(self, path: str, watchman_roots: Set[str],
                 is_healthy: bool) -> None:
        self._path = path
        self._watchman_roots = watchman_roots
        self._is_healthy = is_healthy

    def do_check(self, dry_run: bool) -> CheckResult:
        if not self._is_healthy:
            return self._report(
                CheckResultType.NO_CHECK_BECAUSE_EDEN_WAS_NOT_RUNNING
            )
        if self._path not in self._watchman_roots:
            return self._report(CheckResultType.NO_ISSUE)

        subscriptions = _call_watchman(['debug-get-subscriptions', self._path])
        subscribers = subscriptions.get('subscribers', [])
        names = set()
        for subscriber in subscribers:
            info = subscriber.get('info', {})
            name = info.get('name')
            names.add(name)

        # We use the presence of this in `names` as a heuristic as to whether
        # files from self._path are being edited in Nuclide.
        if 'hg-repository-watchman-subscription-primary' not in names:
            return self._report(CheckResultType.NO_ISSUE)

        whole_repo_subscription = f'filewatcher-{self._path}'
        if whole_repo_subscription in names:
            has_proper_watchman_subscription = True
        else:
            subrepo_subscription_prefix = whole_repo_subscription + '/'
            has_proper_watchman_subscription = any(
                s.startswith(subrepo_subscription_prefix) for s in names
            )

        if has_proper_watchman_subscription:
            return self._report(CheckResultType.NO_ISSUE)
        elif dry_run:
            return self._report(CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN)
        else:
            return self._report(CheckResultType.FAILED_TO_FIX)

    def _report(self, result_type: CheckResultType) -> CheckResult:
        if result_type in [
            CheckResultType.FAILED_TO_FIX,
            CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN
        ]:
            msg = (
                f'Nuclide appears to be used to edit {self._path},\n'
                'but a key Watchman subscription appears to be missing.\n'
                'This can cause file changes to fail to show up in Nuclide.\n'
                'Currently, the only workaround this is to run\n'
                '"Nuclide Remote Projects: Kill And Restart" from the\n'
                'command palette in Atom.\n'
            )
        else:
            msg = ''
        return CheckResult(result_type, msg)


class SnapshotDirstateConsistencyCheck:
    def __init__(self, path: str, snapshot_hex: str, is_healthy: bool) -> None:
        self._path = path
        self._snapshot_hex = snapshot_hex
        self._is_healthy = is_healthy

    def do_check(self, dry_run: bool) -> CheckResult:
        if not self._is_healthy:
            return self._report(
                CheckResultType.NO_CHECK_BECAUSE_EDEN_WAS_NOT_RUNNING
            )

        dirstate = os.path.join(self._path, '.hg', 'dirstate')
        with open(dirstate, 'rb') as f:
            parents, _tuples_dict, _copymap = eden.dirstate.read(f, dirstate)
        p1 = parents[0]
        self._p1_hex = binascii.hexlify(p1).decode('utf-8')

        if self._snapshot_hex == self._p1_hex:
            return self._report(CheckResultType.NO_ISSUE)
        else:
            return self._report(CheckResultType.FAILED_TO_FIX)

    def _report(self, result_type: CheckResultType) -> CheckResult:
        if result_type == CheckResultType.FAILED_TO_FIX:
            msg = (
                f'p1 for {self._path} is {self._p1_hex}, but Eden\'s internal\n'
                f'hash in its SNAPSHOT file is {self._snapshot_hex}.\n'
            )
        else:
            msg = ''
        return CheckResult(result_type, msg)


class EdenfsIsLatest:
    def __init__(self, config) -> None:
        self._config = config

    def do_check(self, dry_run: bool) -> CheckResult:
        build_info = self._config.get_server_build_info()
        version = build_info.get('build_package_version')
        release = build_info.get('build_package_release')
        if not version or not release:
            # This could be a dev build that returns the empty string for both
            # of these values.
            return CheckResult(CheckResultType.NO_ISSUE, '')

        running_version = f'{version}-{release}'
        installed_version = _call_rpm_q()
        if running_version == installed_version:
            return CheckResult(CheckResultType.NO_ISSUE, '')
        else:
            return CheckResult(
                CheckResultType.FAILED_TO_FIX,
                dedent(
                    f'''\
                    The version of Eden that is installed on your machine is:
                        fb-eden-{installed_version}.x86_64
                    but the version of Eden that is currently running is:
                        fb-eden-{running_version}.x86_64
                    Consider running `eden shutdown` followed by `eden daemon`
                    to restart with the installed version, which may have
                    important bug fixes or performance improvements.
                    '''
                )
            )


def _get_watch_roots_for_watchman() -> Set[str]:
    js = _call_watchman(['watch-list'])
    roots = set()
    for root in js['roots']:
        roots.add(root)
    return roots


def _call_watchman(args: List[str]) -> Dict:
    full_args = ['watchman']
    full_args.extend(args)
    try:
        output = subprocess.check_output(full_args)
        return json.loads(output)
    except OSError as e:
        sys.stderr.write(
            f'Calling `{" ".join(full_args)}`'
            f' failed with: {os.sterror(e.errno)}\n'
        )
        return {'error': str(e)}


def _call_rpm_q() -> str:
    return subprocess.check_output(
        ['rpm', '-q', 'fb-eden', '--queryformat', '%{version}-%{release}']
    ).decode('utf-8')
