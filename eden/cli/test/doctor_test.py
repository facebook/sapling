#!/usr/bin/env python3
#
# Copyright (c) 2017-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import io
import os
import shutil
import tempfile
import unittest
from collections import OrderedDict
from textwrap import dedent
from typing import Any, Dict, Iterable, List, Optional
from unittest.mock import call, patch
import eden.cli.doctor as doctor
import eden.cli.config as config_mod
from eden.cli.doctor import CheckResultType
from fb303.ttypes import fb_status
import eden.dirstate


class DoctorTest(unittest.TestCase):
    # The diffs for what is written to stdout can be large.
    maxDiff = None

    @patch('eden.cli.doctor._call_watchman')
    def test_end_to_end_test_with_various_scenarios(self, mock_watchman):
        side_effects: List[Dict[str, Any]] = []
        calls = []
        tmp_dir = tempfile.mkdtemp(prefix='eden_test.')
        try:
            # In edenfs_path1, we will break the snapshot check.
            edenfs_path1 = os.path.join(tmp_dir, 'path1')
            # In edenfs_path2, we will break the inotify check and the Nuclide
            # subscriptions check.
            edenfs_path2 = os.path.join(tmp_dir, 'path2')

            calls.append(call(['watch-list']))
            side_effects.append({'roots': [edenfs_path1, edenfs_path2]})

            calls.append(call(['watch-project', edenfs_path1]))
            side_effects.append({'watcher': 'eden'})

            calls.append(call(['debug-get-subscriptions', edenfs_path1]))
            side_effects.append(
                _create_watchman_subscription(
                    filewatcher_subscription=f'filewatcher-{edenfs_path1}',
                )
            )

            calls.append(call(['watch-project', edenfs_path2]))
            side_effects.append({'watcher': 'inotify'})
            calls.append(call(['watch-del', edenfs_path2]))
            side_effects.append({'watch-del': True, 'root': edenfs_path2})
            calls.append(call(['watch-project', edenfs_path2]))
            side_effects.append({'watcher': 'eden'})

            calls.append(call(['debug-get-subscriptions', edenfs_path2]))
            side_effects.append(
                _create_watchman_subscription(filewatcher_subscription=None)
            )

            mock_watchman.side_effect = side_effects

            out = io.StringIO()
            dry_run = False
            mount_paths = OrderedDict()
            edenfs_path1_snapshot_hex = 'abcd' * 10
            mount_paths[edenfs_path1] = {
                'bind-mounts': {},
                'mount': edenfs_path1,
                'scm_type': 'hg',
                'snapshot': edenfs_path1_snapshot_hex,
                'client-dir': '/I_DO_NOT_EXIST1'
            }
            mount_paths[edenfs_path2] = {
                'bind-mounts': {},
                'mount': edenfs_path2,
                'scm_type': 'git',
                'snapshot': 'dcba' * 10,
                'client-dir': '/I_DO_NOT_EXIST2'
            }
            config = FakeConfig(mount_paths, is_healthy=True)

            os.mkdir(edenfs_path1)
            hg_dir = os.path.join(edenfs_path1, '.hg')
            os.mkdir(hg_dir)
            dirstate = os.path.join(hg_dir, 'dirstate')
            dirstate_hash = b'\x12\x34\x56\x78' * 5
            parents = (dirstate_hash, b'\x00' * 20)
            with open(dirstate, 'wb') as f:
                eden.dirstate.write(f, parents, tuples_dict={}, copymap={})

            exit_code = doctor.cure_what_ails_you(config, dry_run, out)
        finally:
            shutil.rmtree(tmp_dir)

        self.assertEqual(
            f'''\
Performing 3 checks for {edenfs_path1}.
p1 for {edenfs_path1} is {'12345678' * 5}, but Eden's internal
hash in its SNAPSHOT file is {edenfs_path1_snapshot_hex}.
Performing 2 checks for {edenfs_path2}.
Previous Watchman watcher for {edenfs_path2} was "inotify" but is now "eden".
Nuclide appears to be used to edit {edenfs_path2},
but a key Watchman subscription appears to be missing.
This can cause file changes to fail to show up in Nuclide.
Currently, the only workaround this is to run
"Nuclide Remote Projects: Kill And Restart" from the
command palette in Atom.
Number of fixes made: 1.
Number of issues that could not be fixed: 2.
''', out.getvalue()
        )
        mock_watchman.assert_has_calls(calls)
        self.assertEqual(1, exit_code)

    @patch('eden.cli.doctor._call_watchman')
    def test_not_all_mounts_have_watchman_watcher(self, mock_watchman):
        edenfs_path = '/path/to/eden-mount'
        edenfs_path_not_watched = '/path/to/eden-mount-not-watched'
        side_effects: List[Dict[str, Any]] = []
        calls = []

        calls.append(call(['watch-list']))
        side_effects.append({'roots': [edenfs_path]})
        calls.append(call(['watch-project', edenfs_path]))
        side_effects.append({'watcher': 'eden'})
        calls.append(call(['debug-get-subscriptions', edenfs_path]))
        side_effects.append({})
        mock_watchman.side_effect = side_effects

        out = io.StringIO()
        dry_run = False
        mount_paths = OrderedDict()
        mount_paths[edenfs_path] = {
            'bind-mounts': {},
            'mount': edenfs_path,
            'scm_type': 'git',
            'snapshot': 'abcd' * 10,
            'client-dir': '/I_DO_NOT_EXIST'
        }
        mount_paths[edenfs_path_not_watched] = {
            'bind-mounts': {},
            'mount': edenfs_path_not_watched,
            'scm_type': 'git',
            'snapshot': 'abcd' * 10,
            'client-dir': '/I_DO_NOT_EXIST'
        }
        config = FakeConfig(mount_paths, is_healthy=True)
        exit_code = doctor.cure_what_ails_you(config, dry_run, out)

        self.assertEqual(
            'Performing 2 checks for /path/to/eden-mount.\n'
            'Performing 2 checks for /path/to/eden-mount-not-watched.\n'
            'All is well.\n', out.getvalue()
        )
        mock_watchman.assert_has_calls(calls)
        self.assertEqual(0, exit_code)

    @patch('eden.cli.doctor._call_watchman')
    def test_not_much_to_do_when_eden_is_not_running(self, mock_watchman):
        edenfs_path = '/path/to/eden-mount'
        side_effects: List[Dict[str, Any]] = []
        calls = []

        calls.append(call(['watch-list']))
        side_effects.append({'roots': [edenfs_path]})
        mock_watchman.side_effect = side_effects

        out = io.StringIO()
        dry_run = False
        mount_paths = {
            edenfs_path: {
                'bind-mounts': {},
                'mount': edenfs_path,
                'scm_type': 'hg',
                'snapshot': 'abcd' * 10,
                'client-dir': '/I_DO_NOT_EXIST'
            }
        }
        config = FakeConfig(mount_paths, is_healthy=False)
        exit_code = doctor.cure_what_ails_you(config, dry_run, out)

        self.assertEqual(
            'Eden is not running: cannot perform all checks.\n'
            'Performing 3 checks for /path/to/eden-mount.\n'
            'All is well.\n', out.getvalue()
        )
        mock_watchman.assert_has_calls(calls)
        self.assertEqual(0, exit_code)

    def test_fails_if_no_mount_points(self):
        out = io.StringIO()
        dry_run = False
        mount_paths = {}
        config = FakeConfig(mount_paths, is_healthy=False)

        exit_code = doctor.cure_what_ails_you(config, dry_run, out)
        self.assertEqual('No mounts points to assess.\n', out.getvalue())
        self.assertEqual(1, exit_code)

    @patch('eden.cli.doctor._call_watchman')
    def test_no_issue_when_watchman_using_eden_watcher(self, mock_watchman):
        self._test_watchman_watcher_check(
            mock_watchman,
            CheckResultType.NO_ISSUE,
            initial_watcher='eden',
            dry_run=False
        )

    @patch('eden.cli.doctor._call_watchman')
    def test_fix_when_watchman_using_inotify_watcher(self, mock_watchman):
        self._test_watchman_watcher_check(
            mock_watchman,
            CheckResultType.FIXED,
            initial_watcher='inotify',
            new_watcher='eden',
            dry_run=False
        )

    @patch('eden.cli.doctor._call_watchman')
    def test_dry_run_identifies_inotify_watcher_issue(self, mock_watchman):
        self._test_watchman_watcher_check(
            mock_watchman,
            CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN,
            initial_watcher='inotify',
            dry_run=True
        )

    @patch('eden.cli.doctor._call_watchman')
    def test_doctor_reports_failure_if_cannot_replace_inotify_watcher(
        self, mock_watchman
    ):
        self._test_watchman_watcher_check(
            mock_watchman,
            CheckResultType.FAILED_TO_FIX,
            initial_watcher='inotify',
            new_watcher='inotify',
            dry_run=False
        )

    def _test_watchman_watcher_check(
        self,
        mock_watchman,
        expected_check_result: Optional[CheckResultType],
        initial_watcher: str,
        new_watcher: Optional[str] = None,
        dry_run: bool = True,
    ):
        edenfs_path = '/path/to/eden-mount'
        side_effects: List[Dict[str, Any]] = []
        calls = []

        calls.append(call(['watch-project', edenfs_path]))
        side_effects.append({'watch': edenfs_path, 'watcher': initial_watcher})

        if initial_watcher != 'eden' and not dry_run:
            calls.append(call(['watch-del', edenfs_path]))
            side_effects.append({'watch-del': True, 'root': edenfs_path})

            self.assertIsNotNone(
                new_watcher,
                msg='Must specify new_watcher when initial_watcher is "eden".'
            )
            calls.append(call(['watch-project', edenfs_path]))
            side_effects.append({'watch': edenfs_path, 'watcher': new_watcher})
        mock_watchman.side_effect = side_effects

        watchman_roots = set([edenfs_path])
        watcher_check = doctor.WatchmanUsingEdenSubscriptionCheck(
            edenfs_path,
            watchman_roots,
            True  # is_healthy
        )

        check_result = watcher_check.do_check(dry_run)
        self.assertEqual(expected_check_result, check_result.result_type)
        mock_watchman.assert_has_calls(calls)

    @patch('eden.cli.doctor._call_watchman')
    def test_no_issue_when_expected_nuclide_subscriptions_present(
        self, mock_watchman
    ):
        self._test_nuclide_check(
            mock_watchman=mock_watchman,
            expected_check_result=CheckResultType.NO_ISSUE,
            include_filewatcher_subscription=True
        )

    @patch('eden.cli.doctor._call_watchman')
    def test_no_issue_when_marker_nuclide_subscription_not_present(
        self, mock_watchman
    ):
        self._test_nuclide_check(
            mock_watchman=mock_watchman,
            expected_check_result=CheckResultType.NO_ISSUE,
            include_primary_subscription=False
        )

    @patch('eden.cli.doctor._call_watchman')
    def test_filewatcher_subscription_is_missing(self, mock_watchman):
        self._test_nuclide_check(
            mock_watchman=mock_watchman,
            expected_check_result=CheckResultType.FAILED_TO_FIX,
            dry_run=False,
        )

    @patch('eden.cli.doctor._call_watchman')
    def test_filewatcher_subscription_is_missing_dry_run(self, mock_watchman):
        self._test_nuclide_check(
            mock_watchman=mock_watchman,
            expected_check_result=CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN
        )

    def _test_nuclide_check(
        self,
        mock_watchman,
        expected_check_result: CheckResultType,
        dry_run: bool = True,
        include_filewatcher_subscription: bool = False,
        include_primary_subscription: bool = True,
    ) -> None:
        edenfs_path = '/path/to/eden-mount'
        side_effects: List[Dict[str, Any]] = []
        calls = []

        calls.append(call(['debug-get-subscriptions', edenfs_path]))
        if include_filewatcher_subscription:
            # Note that a "filewatcher-" subscription in a subdirectory of the
            # Eden mount should signal that the proper Watchman subscription is
            # set up.
            filewatcher_subscription: Optional[
                str
            ] = f'filewatcher-{os.path.join(edenfs_path, "subdirectory")}'
        else:
            filewatcher_subscription = None

        side_effects.append(
            _create_watchman_subscription(
                filewatcher_subscription=filewatcher_subscription,
                include_primary_subscription=include_primary_subscription,
            )
        )
        mock_watchman.side_effect = side_effects

        watchman_roots = set([edenfs_path])
        nuclide_check = doctor.NuclideHasExpectedWatchmanSubscriptions(
            edenfs_path,
            watchman_roots,
            True  # is_healthy
        )

        check_result = nuclide_check.do_check(dry_run)
        self.assertEqual(expected_check_result, check_result.result_type)
        mock_watchman.assert_has_calls(calls)

    def test_snapshot_and_dirstate_file_match(self):
        dirstate_hash = b'\x12\x34\x56\x78' * 5
        snapshot_hex = '12345678' * 5
        self._test_hash_check(
            dirstate_hash, snapshot_hex, CheckResultType.NO_ISSUE
        )

    def test_snapshot_and_dirstate_file_differ(self):
        dirstate_hash = b'\x12\x00\x00\x00' * 5
        snapshot_hex = '12345678' * 5
        self._test_hash_check(
            dirstate_hash, snapshot_hex, CheckResultType.FAILED_TO_FIX
        )

    def _test_hash_check(
        self, dirstate_hash: bytes, snapshot_hex: str,
        expected_check_result: CheckResultType
    ):
        mount_path = tempfile.mkdtemp(prefix='eden_test.')
        try:
            hg_dir = os.path.join(mount_path, '.hg')
            os.mkdir(hg_dir)
            dirstate = os.path.join(hg_dir, 'dirstate')
            parents = (dirstate_hash, b'\x00' * 20)
            with open(dirstate, 'wb') as f:
                eden.dirstate.write(f, parents, tuples_dict={}, copymap={})

            is_healthy = True
            hash_check = doctor.SnapshotDirstateConsistencyCheck(
                mount_path, snapshot_hex, is_healthy
            )
            dry_run = True
            check_result = hash_check.do_check(dry_run)
            self.assertEqual(expected_check_result, check_result.result_type)
        finally:
            shutil.rmtree(mount_path)

    @patch('eden.cli.doctor._call_rpm_q')
    def test_edenfs_when_installed_and_running_match(self, mock_rpm_q):
        self._test_edenfs_version(
            mock_rpm_q, '20171213-165642',
            CheckResultType.NO_ISSUE, ''
        )

    @patch('eden.cli.doctor._call_rpm_q')
    def test_edenfs_when_installed_and_running_differ(self, mock_rpm_q):
        self._test_edenfs_version(
            mock_rpm_q, '20171120-246561',
            CheckResultType.FAILED_TO_FIX,
            dedent(
                '''\
                The version of Eden that is installed on your machine is:
                    fb-eden-20171120-246561.x86_64
                but the version of Eden that is currently running is:
                    fb-eden-20171213-165642.x86_64
                Consider running `eden shutdown` followed by `eden daemon`
                to restart with the installed version, which may have
                important bug fixes or performance improvements.
                '''
            )
        )

    def _test_edenfs_version(
        self,
        mock_rpm_q,
        rpm_value: str,
        expected_check_result: CheckResultType,
        expected_message: str,
    ):
        side_effects: List[str] = []
        calls = []
        calls.append(call())
        side_effects.append(rpm_value)
        mock_rpm_q.side_effect = side_effects

        config = FakeConfig(
            mount_paths={},
            is_healthy=True,
            build_info={
                'build_package_version': '20171213',
                'build_package_release': '165642',
            }
        )
        version_check = doctor.EdenfsIsLatest(config)
        check_result = version_check.do_check(dry_run=False)
        self.assertEqual(expected_check_result, check_result.result_type)
        self.assertEqual(expected_message, check_result.message)

        mock_rpm_q.assert_has_calls(calls)


def _create_watchman_subscription(
    filewatcher_subscription: Optional[str] = None,
    include_primary_subscription: bool = True,
) -> Dict:
    subscribers = []
    if filewatcher_subscription is not None:
        subscribers.append(
            {
                'info': {
                    'name': filewatcher_subscription,
                    'query': {
                        'empty_on_fresh_instance': True,
                        'defer_vcs': False,
                        'fields': ['name', 'new', 'exists', 'mode'],
                        'relative_root': 'fbcode',
                        'since': 'c:1511985586:2749065:2774073346:354'
                    },
                }
            }
        )
    if include_primary_subscription:
        subscribers.append(
            {
                'info': {
                    'name': 'hg-repository-watchman-subscription-primary',
                    'query': {
                        'empty_on_fresh_instance': True,
                        'fields': ['name', 'new', 'exists', 'mode'],
                    },
                }
            }
        )
    return {
        'subscribers': subscribers,
    }


class FakeConfig:
    def __init__(
        self,
        mount_paths: Dict[str, Dict[str, str]],
        is_healthy: bool = True,
        build_info: Optional[Dict[str, str]] = None,
    ) -> None:
        self._mount_paths = mount_paths
        self._is_healthy = is_healthy
        self._build_info = build_info if build_info else {}

    def get_mount_paths(self) -> Iterable[str]:
        return self._mount_paths.keys()

    def check_health(self) -> config_mod.HealthStatus:
        status = fb_status.ALIVE if self._is_healthy else fb_status.STOPPED
        return config_mod.HealthStatus(status, pid=None, detail='')

    def get_client_info(self, mount_path: str) -> Dict[str, str]:
        return self._mount_paths[mount_path]

    def get_server_build_info(self) -> Dict[str, str]:
        return dict(self._build_info)
