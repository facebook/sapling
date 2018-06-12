#!/usr/bin/env python3
#
# Copyright (c) 2017-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
import io
import os
import shutil
import tempfile
import unittest
from collections import OrderedDict
from textwrap import dedent
from typing import Any, Dict, Iterable, List, Optional, Set, Union
from unittest.mock import call, patch

import eden.cli.config as config_mod
import eden.cli.doctor as doctor
import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
from eden.cli import mtab
from eden.cli.doctor import CheckResult, CheckResultType
from eden.cli.stdout_printer import AnsiEscapeCodes, StdoutPrinter
from fb303.ttypes import fb_status


escape_codes = AnsiEscapeCodes(
    bold="<bold>", red="<red>", green="<green>", yellow="<yellow>", reset="<reset>"
)
printer = StdoutPrinter(escape_codes)


class DoctorTest(unittest.TestCase):
    # The diffs for what is written to stdout can be large.
    maxDiff = None

    @patch("eden.cli.doctor._call_watchman")
    @patch("eden.cli.doctor._get_roots_for_nuclide")
    def test_end_to_end_test_with_various_scenarios(
        self, mock_get_roots_for_nuclide, mock_watchman
    ):
        side_effects: List[Dict[str, Any]] = []
        calls = []
        tmp_dir = tempfile.mkdtemp(prefix="eden_test.")
        try:
            # In edenfs_path1, we will break the snapshot check.
            edenfs_path1 = os.path.join(tmp_dir, "path1")
            # In edenfs_path2, we will break the inotify check and the Nuclide
            # subscriptions check.
            edenfs_path2 = os.path.join(tmp_dir, "path2")
            # In edenfs_path3, we do not create the .hg directory
            edenfs_path3 = os.path.join(tmp_dir, "path3")

            # Assume all paths are used as root folders in a connected Nuclide.
            mock_get_roots_for_nuclide.return_value = {
                edenfs_path1,
                edenfs_path2,
                edenfs_path3,
            }

            calls.append(call(["watch-list"]))
            side_effects.append({"roots": [edenfs_path1, edenfs_path2, edenfs_path3]})

            calls.append(call(["watch-project", edenfs_path1]))
            side_effects.append({"watcher": "eden"})

            calls.append(call(["debug-get-subscriptions", edenfs_path1]))
            side_effects.append(
                _create_watchman_subscription(
                    filewatcher_subscription=f"filewatcher-{edenfs_path1}"
                )
            )

            calls.append(call(["watch-project", edenfs_path2]))
            side_effects.append({"watcher": "inotify"})
            calls.append(call(["watch-del", edenfs_path2]))
            side_effects.append({"watch-del": True, "root": edenfs_path2})
            calls.append(call(["watch-project", edenfs_path2]))
            side_effects.append({"watcher": "eden"})

            calls.append(call(["debug-get-subscriptions", edenfs_path2]))
            side_effects.append(
                _create_watchman_subscription(filewatcher_subscription=None)
            )

            calls.append(call(["watch-project", edenfs_path3]))
            side_effects.append({"watcher": "eden"})
            calls.append(call(["debug-get-subscriptions", edenfs_path3]))
            side_effects.append(
                _create_watchman_subscription(
                    filewatcher_subscription=f"filewatcher-{edenfs_path3}"
                )
            )

            mock_watchman.side_effect = side_effects

            out = io.StringIO()
            dry_run = False
            mount_paths = OrderedDict()
            edenfs_path1_snapshot_hex = "abcd" * 10
            mount_paths[edenfs_path1] = {
                "bind-mounts": {},
                "mount": edenfs_path1,
                "scm_type": "hg",
                "snapshot": edenfs_path1_snapshot_hex,
                "client-dir": "/I_DO_NOT_EXIST1",
            }
            mount_paths[edenfs_path2] = {
                "bind-mounts": {},
                "mount": edenfs_path2,
                "scm_type": "git",
                "snapshot": "dcba" * 10,
                "client-dir": "/I_DO_NOT_EXIST2",
            }
            edenfs_path3_snapshot_hex = "1234" * 10
            mount_paths[edenfs_path3] = {
                "bind-mounts": {},
                "mount": edenfs_path3,
                "scm_type": "hg",
                "snapshot": edenfs_path3_snapshot_hex,
                "client-dir": "/I_DO_NOT_EXIST3",
            }
            config = FakeConfig(mount_paths, is_healthy=True)
            config.get_thrift_client()._mounts = [
                eden_ttypes.MountInfo(mountPoint=edenfs_path1),
                eden_ttypes.MountInfo(mountPoint=edenfs_path2),
                eden_ttypes.MountInfo(mountPoint=edenfs_path3),
            ]

            os.mkdir(edenfs_path1)
            hg_dir = os.path.join(edenfs_path1, ".hg")
            os.mkdir(hg_dir)
            dirstate = os.path.join(hg_dir, "dirstate")
            dirstate_hash = b"\x12\x34\x56\x78" * 5
            parents = (dirstate_hash, b"\x00" * 20)
            with open(dirstate, "wb") as f:
                eden.dirstate.write(f, parents, tuples_dict={}, copymap={})

            os.mkdir(edenfs_path3)

            mount_table = FakeMountTable()
            mount_table.stats[edenfs_path1] = mtab.MTStat(st_uid=os.getuid(), st_dev=11)
            mount_table.stats[edenfs_path2] = mtab.MTStat(st_uid=os.getuid(), st_dev=12)
            mount_table.stats[edenfs_path3] = mtab.MTStat(st_uid=os.getuid(), st_dev=13)
            exit_code = doctor.cure_what_ails_you(
                config, dry_run, out, mount_table, printer=printer
            )
        finally:
            shutil.rmtree(tmp_dir)

        self.assertEqual(
            f"""\
Performing 3 checks for {edenfs_path1}.
p1 for {edenfs_path1} is {'12345678' * 5}, but Eden's internal
hash in its SNAPSHOT file is {edenfs_path1_snapshot_hex}.
Performing 2 checks for {edenfs_path2}.
Previous Watchman watcher for {edenfs_path2} was "inotify" but is now "eden".
Nuclide appears to be used to edit the following directories
under {edenfs_path2}:

  {edenfs_path2}

but the following Watchman subscriptions appear to be missing:

  filewatcher-{edenfs_path2}

This can cause file changes to fail to show up in Nuclide.
Currently, the only workaround for this is to run
"Nuclide Remote Projects: Kill And Restart" from the
command palette in Atom.
Performing 3 checks for {edenfs_path3}.
{edenfs_path3}/.hg/dirstate is missing
The most common cause of this is if you previously tried to manually remove this eden
mount with "rm -rf".  You should instead remove it using "eden rm {edenfs_path3}",
and can re-clone the checkout afterwards if desired.
<yellow>Number of fixes made: 1.<reset>
<red>Number of issues that could not be fixed: 3.<reset>
""",
            out.getvalue(),
        )
        mock_watchman.assert_has_calls(calls)
        self.assertEqual(1, exit_code)

    @patch("eden.cli.doctor._call_watchman")
    @patch("eden.cli.doctor._get_roots_for_nuclide", return_value=set())
    def test_not_all_mounts_have_watchman_watcher(
        self, mock_get_roots_for_nuclide, mock_watchman
    ):
        edenfs_path = "/path/to/eden-mount"
        edenfs_path_not_watched = "/path/to/eden-mount-not-watched"
        side_effects: List[Dict[str, Any]] = []
        calls = []

        calls.append(call(["watch-list"]))
        side_effects.append({"roots": [edenfs_path]})
        calls.append(call(["watch-project", edenfs_path]))
        side_effects.append({"watcher": "eden"})
        mock_watchman.side_effect = side_effects

        out = io.StringIO()
        dry_run = False
        mount_paths = OrderedDict()
        mount_paths[edenfs_path] = {
            "bind-mounts": {},
            "mount": edenfs_path,
            "scm_type": "git",
            "snapshot": "abcd" * 10,
            "client-dir": "/I_DO_NOT_EXIST",
        }
        mount_paths[edenfs_path_not_watched] = {
            "bind-mounts": {},
            "mount": edenfs_path_not_watched,
            "scm_type": "git",
            "snapshot": "abcd" * 10,
            "client-dir": "/I_DO_NOT_EXIST",
        }
        config = FakeConfig(mount_paths, is_healthy=True)
        config.get_thrift_client()._mounts = [
            eden_ttypes.MountInfo(mountPoint=edenfs_path),
            eden_ttypes.MountInfo(mountPoint=edenfs_path_not_watched),
        ]
        mount_table = FakeMountTable()
        mount_table.stats["/path/to/eden-mount"] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=10
        )
        mount_table.stats["/path/to/eden-mount-not-watched"] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11
        )
        exit_code = doctor.cure_what_ails_you(
            config, dry_run, out, mount_table=mount_table, printer=printer
        )

        self.assertEqual(
            "Performing 2 checks for /path/to/eden-mount.\n"
            "Performing 2 checks for /path/to/eden-mount-not-watched.\n"
            "<green>All is well.<reset>\n",
            out.getvalue(),
        )
        mock_watchman.assert_has_calls(calls)
        self.assertEqual(0, exit_code)

    @patch("eden.cli.doctor._call_watchman")
    @patch("eden.cli.doctor._get_roots_for_nuclide")
    def test_not_much_to_do_when_eden_is_not_running(
        self, mock_get_roots_for_nuclide, mock_watchman
    ):
        edenfs_path = "/path/to/eden-mount"
        side_effects: List[Dict[str, Any]] = []
        calls = []

        # Note that even though Nuclide has a root that points to an unmounted
        # Eden directory, `eden doctor` is not going to be able to report
        # anything because it cannot make the Thrift call to `eden list` to
        # discover that edenfs_path is normally an Eden mount.
        mock_get_roots_for_nuclide.return_value = {edenfs_path}

        calls.append(call(["watch-list"]))
        side_effects.append({"roots": [edenfs_path]})
        mock_watchman.side_effect = side_effects

        out = io.StringIO()
        dry_run = False
        mount_paths = {
            edenfs_path: {
                "bind-mounts": {},
                "mount": edenfs_path,
                "scm_type": "hg",
                "snapshot": "abcd" * 10,
                "client-dir": "/I_DO_NOT_EXIST",
            }
        }
        config = FakeConfig(mount_paths, is_healthy=False)
        exit_code = doctor.cure_what_ails_you(
            config, dry_run, out, FakeMountTable(), printer=printer
        )

        self.assertEqual(
            dedent(
                """\
Eden is not running: cannot perform all checks.
To start Eden, run:

    eden start

Cannot check if running latest edenfs because the daemon is not running.
<green>All is well.<reset>
"""
            ),
            out.getvalue(),
        )
        mock_watchman.assert_has_calls(calls)
        self.assertEqual(0, exit_code)

    @patch("eden.cli.doctor._call_watchman")
    def test_no_issue_when_watchman_using_eden_watcher(self, mock_watchman):
        self._test_watchman_watcher_check(
            mock_watchman,
            CheckResultType.NO_ISSUE,
            initial_watcher="eden",
            dry_run=False,
        )

    @patch("eden.cli.doctor._call_watchman")
    def test_fix_when_watchman_using_inotify_watcher(self, mock_watchman):
        self._test_watchman_watcher_check(
            mock_watchman,
            CheckResultType.FIXED,
            initial_watcher="inotify",
            new_watcher="eden",
            dry_run=False,
        )

    @patch("eden.cli.doctor._call_watchman")
    def test_dry_run_identifies_inotify_watcher_issue(self, mock_watchman):
        self._test_watchman_watcher_check(
            mock_watchman,
            CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN,
            initial_watcher="inotify",
            dry_run=True,
        )

    @patch("eden.cli.doctor._call_watchman")
    def test_doctor_reports_failure_if_cannot_replace_inotify_watcher(
        self, mock_watchman
    ):
        self._test_watchman_watcher_check(
            mock_watchman,
            CheckResultType.FAILED_TO_FIX,
            initial_watcher="inotify",
            new_watcher="inotify",
            dry_run=False,
        )

    def _test_watchman_watcher_check(
        self,
        mock_watchman,
        expected_check_result: Optional[CheckResultType],
        initial_watcher: str,
        new_watcher: Optional[str] = None,
        dry_run: bool = True,
    ):
        edenfs_path = "/path/to/eden-mount"
        side_effects: List[Dict[str, Any]] = []
        calls = []

        calls.append(call(["watch-project", edenfs_path]))
        side_effects.append({"watch": edenfs_path, "watcher": initial_watcher})

        if initial_watcher != "eden" and not dry_run:
            calls.append(call(["watch-del", edenfs_path]))
            side_effects.append({"watch-del": True, "root": edenfs_path})

            self.assertIsNotNone(
                new_watcher,
                msg='Must specify new_watcher when initial_watcher is "eden".',
            )
            calls.append(call(["watch-project", edenfs_path]))
            side_effects.append({"watch": edenfs_path, "watcher": new_watcher})
        mock_watchman.side_effect = side_effects

        watchman_roots = {edenfs_path}
        watcher_check = doctor.WatchmanUsingEdenSubscriptionCheck(
            edenfs_path, watchman_roots, True  # is_healthy
        )

        check_result = watcher_check.do_check(dry_run)
        self.assertEqual(expected_check_result, check_result.result_type)
        mock_watchman.assert_has_calls(calls)

    @patch("eden.cli.doctor._call_watchman")
    def test_no_issue_when_expected_nuclide_subscriptions_present(self, mock_watchman):
        self._test_nuclide_check(
            mock_watchman=mock_watchman,
            expected_check_result=CheckResultType.NO_ISSUE,
            include_filewatcher_subscription=True,
        )

    @patch("eden.cli.doctor._call_watchman")
    def test_no_issue_when_path_not_in_nuclide_roots(self, mock_watchman):
        self._test_nuclide_check(
            mock_watchman=mock_watchman,
            expected_check_result=CheckResultType.NO_ISSUE,
            include_path_in_nuclide_roots=False,
        )

    @patch("eden.cli.doctor._call_watchman")
    def test_watchman_subscriptions_are_missing(self, mock_watchman):
        check_result = self._test_nuclide_check(
            mock_watchman=mock_watchman,
            expected_check_result=CheckResultType.FAILED_TO_FIX,
            include_hg_subscriptions=False,
            dry_run=False,
        )
        self.assertEqual(
            f"""\
Nuclide appears to be used to edit the following directories
under /path/to/eden-mount:

  /path/to/eden-mount/subdirectory

but the following Watchman subscriptions appear to be missing:

  filewatcher-/path/to/eden-mount/subdirectory
  hg-repository-watchman-subscription-primary
  hg-repository-watchman-subscription-conflicts
  hg-repository-watchman-subscription-hgbookmark
  hg-repository-watchman-subscription-hgbookmarks
  hg-repository-watchman-subscription-dirstate
  hg-repository-watchman-subscription-progress
  hg-repository-watchman-subscription-lock-files

This can cause file changes to fail to show up in Nuclide.
Currently, the only workaround for this is to run
"Nuclide Remote Projects: Kill And Restart" from the
command palette in Atom.
""",
            check_result.message,
        )

    @patch("eden.cli.doctor._call_watchman")
    def test_filewatcher_subscription_is_missing_dry_run(self, mock_watchman):
        self._test_nuclide_check(
            mock_watchman=mock_watchman,
            expected_check_result=CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN,
        )

    def _test_nuclide_check(
        self,
        mock_watchman,
        expected_check_result: CheckResultType,
        dry_run: bool = True,
        include_filewatcher_subscription: bool = False,
        include_path_in_nuclide_roots: bool = True,
        include_hg_subscriptions: bool = True,
    ) -> CheckResult:
        edenfs_path = "/path/to/eden-mount"
        side_effects: List[Dict[str, Any]] = []
        watchman_calls = []

        if include_path_in_nuclide_roots:
            watchman_calls.append(call(["debug-get-subscriptions", edenfs_path]))

        nuclide_root = os.path.join(edenfs_path, "subdirectory")
        if include_filewatcher_subscription:
            # Note that a "filewatcher-" subscription in a subdirectory of the
            # Eden mount should signal that the proper Watchman subscription is
            # set up.
            filewatcher_sub: Optional[str] = f"filewatcher-{nuclide_root}"
        else:
            filewatcher_sub = None

        unrelated_path = "/path/to/non-eden-mount"
        if include_path_in_nuclide_roots:
            nuclide_roots = {nuclide_root, unrelated_path}
        else:
            nuclide_roots = {unrelated_path}

        side_effects.append(
            _create_watchman_subscription(
                filewatcher_subscription=filewatcher_sub,
                include_hg_subscriptions=include_hg_subscriptions,
            )
        )
        mock_watchman.side_effect = side_effects
        watchman_roots = {edenfs_path}
        nuclide_check = doctor.NuclideHasExpectedWatchmanSubscriptions(
            edenfs_path, watchman_roots, nuclide_roots
        )

        check_result = nuclide_check.do_check(dry_run)
        self.assertEqual(expected_check_result, check_result.result_type)
        mock_watchman.assert_has_calls(watchman_calls)
        return check_result

    def test_snapshot_and_dirstate_file_match(self):
        dirstate_hash = b"\x12\x34\x56\x78" * 5
        snapshot_hex = "12345678" * 5
        self._test_hash_check(dirstate_hash, snapshot_hex, CheckResultType.NO_ISSUE)

    def test_snapshot_and_dirstate_file_differ(self):
        dirstate_hash = b"\x12\x00\x00\x00" * 5
        snapshot_hex = "12345678" * 5
        self._test_hash_check(
            dirstate_hash, snapshot_hex, CheckResultType.FAILED_TO_FIX
        )

    def _test_hash_check(
        self,
        dirstate_hash: bytes,
        snapshot_hex: str,
        expected_check_result: CheckResultType,
    ):
        mount_path = tempfile.mkdtemp(prefix="eden_test.")
        try:
            hg_dir = os.path.join(mount_path, ".hg")
            os.mkdir(hg_dir)
            dirstate = os.path.join(hg_dir, "dirstate")
            parents = (dirstate_hash, b"\x00" * 20)
            with open(dirstate, "wb") as f:
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

    @patch("eden.cli.version.get_installed_eden_rpm_version")
    def test_edenfs_when_installed_and_running_match(self, mock_gierv):
        self._test_edenfs_version(
            mock_gierv, "20171213-165642", CheckResultType.NO_ISSUE, ""
        )

    @patch("eden.cli.version.get_installed_eden_rpm_version")
    def test_edenfs_when_installed_and_running_differ(self, mock_gierv):
        self._test_edenfs_version(
            mock_gierv,
            "20171120-246561",
            CheckResultType.FAILED_TO_FIX,
            dedent(
                """\
    The version of Eden that is installed on your machine is:
        fb-eden-20171120-246561.x86_64
    but the version of Eden that is currently running is:
        fb-eden-20171213-165642.x86_64

    Consider running `eden restart` to migrate to the newer version, which
    may have important bug fixes or performance improvements.
                """
            ),
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
                "build_package_version": "20171213",
                "build_package_release": "165642",
            },
        )
        version_check = doctor.EdenfsIsLatest(config)
        check_result = version_check.do_check(dry_run=False)
        self.assertEqual(expected_check_result, check_result.result_type)
        self.assertEqual(expected_message, check_result.message)

        mock_rpm_q.assert_has_calls(calls)

    @patch("eden.cli.doctor._get_roots_for_nuclide", return_value=set())
    def test_unconfigured_mounts_dont_crash(self, mock_get_roots_for_nuclide):
        # If Eden advertises that a mount is active, but it is not in the
        # configuration, then at least don't throw an exception.
        tmp_dir = tempfile.mkdtemp(prefix="eden_test.")
        try:
            edenfs_path1 = os.path.join(tmp_dir, "path1")
            edenfs_path2 = os.path.join(tmp_dir, "path2")

            mount_paths = OrderedDict()
            mount_paths[edenfs_path1] = {
                "bind-mounts": {},
                "mount": edenfs_path1,
                "scm_type": "hg",
                "snapshot": "abcd" * 10,
                "client-dir": "/I_DO_NOT_EXIST1",
            }
            # path2 is not configured in the config...
            config = FakeConfig(mount_paths, is_healthy=True)
            # ... but is advertised by the daemon...
            config.get_thrift_client()._mounts = [
                eden_ttypes.MountInfo(mountPoint=edenfs_path1),
                eden_ttypes.MountInfo(mountPoint=edenfs_path2),
            ]

            # ... and is in the system mount table.
            mount_table = FakeMountTable()
            mount_table.stats[edenfs_path1] = mtab.MTStat(st_uid=os.getuid(), st_dev=11)
            mount_table.stats[edenfs_path2] = mtab.MTStat(st_uid=os.getuid(), st_dev=12)

            os.mkdir(edenfs_path1)
            hg_dir = os.path.join(edenfs_path1, ".hg")
            os.mkdir(hg_dir)
            dirstate = os.path.join(hg_dir, "dirstate")
            dirstate_hash = b"\xab\xcd" * 10
            parents = (dirstate_hash, b"\x00" * 20)
            with open(dirstate, "wb") as f:
                eden.dirstate.write(f, parents, tuples_dict={}, copymap={})

            dry_run = False
            out = io.StringIO()
            exit_code = doctor.cure_what_ails_you(
                config, dry_run, out, mount_table, printer=printer
            )
        finally:
            shutil.rmtree(tmp_dir)

        self.assertEqual(
            f"""\
Performing 3 checks for {edenfs_path1}.
<green>All is well.<reset>
""",
            out.getvalue(),
        )
        self.assertEqual(0, exit_code)


class StaleMountsCheckTest(unittest.TestCase):
    maxDiff = None

    def setUp(self):
        self.active_mounts: List[bytes] = [b"/mnt/active1", b"/mnt/active2"]
        self.mount_table = FakeMountTable()
        self.mount_table.add_mount("/mnt/active1")
        self.mount_table.add_mount("/mnt/active2")
        self.check = doctor.StaleMountsCheck(
            active_mount_points=self.active_mounts, mount_table=self.mount_table
        )

    def test_does_not_unmount_active_mounts(self):
        result = self.check.do_check(dry_run=False)
        self.assertEqual("", result.message)
        self.assertEqual(doctor.CheckResultType.NO_ISSUE, result.result_type)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_working_nonactive_mount_is_not_unmounted(self):
        # Add a working edenfs mount that is not part of our active list
        self.mount_table.add_mount("/mnt/other1")

        result = self.check.do_check(dry_run=False)
        self.assertEqual("", result.message)
        self.assertEqual(doctor.CheckResultType.NO_ISSUE, result.result_type)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_force_unmounts_if_lazy_fails(self):
        self.mount_table.add_stale_mount("/mnt/stale1")
        self.mount_table.add_stale_mount("/mnt/stale2")
        self.mount_table.fail_unmount_lazy(b"/mnt/stale1")

        result = self.check.do_check(dry_run=False)
        self.assertEqual(
            dedent(
                """\
            Unmounted 2 stale edenfs mount points:
              /mnt/stale1
              /mnt/stale2
        """
            ),
            result.message,
        )
        self.assertEqual(doctor.CheckResultType.FIXED, result.result_type)
        self.assertEqual(
            [b"/mnt/stale1", b"/mnt/stale2"], self.mount_table.unmount_lazy_calls
        )
        self.assertEqual([b"/mnt/stale1"], self.mount_table.unmount_force_calls)

    def test_dry_run_prints_stale_mounts_and_does_not_unmount(self):
        self.mount_table.add_stale_mount("/mnt/stale1")
        self.mount_table.add_stale_mount("/mnt/stale2")

        result = self.check.do_check(dry_run=True)
        self.assertEqual(
            doctor.CheckResultType.NOT_FIXED_BECAUSE_DRY_RUN, result.result_type
        )
        self.assertEqual(
            dedent(
                """\
            Found 2 stale edenfs mount points:
              /mnt/stale1
              /mnt/stale2
            Not unmounting because dry run.
        """
            ),
            result.message,
        )
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_fails_if_unmount_fails(self):
        self.mount_table.add_stale_mount("/mnt/stale1")
        self.mount_table.add_stale_mount("/mnt/stale2")
        self.mount_table.fail_unmount_lazy(b"/mnt/stale1", b"/mnt/stale2")
        self.mount_table.fail_unmount_force(b"/mnt/stale1")

        result = self.check.do_check(dry_run=False)
        self.assertEqual(doctor.CheckResultType.FAILED_TO_FIX, result.result_type)
        self.assertEqual(
            dedent(
                """\
            Successfully unmounted 1 mount point:
              /mnt/stale2
            Failed to unmount 1 mount point:
              /mnt/stale1
        """
            ),
            result.message,
        )
        self.assertEqual(
            [b"/mnt/stale1", b"/mnt/stale2"], self.mount_table.unmount_lazy_calls
        )
        self.assertEqual(
            [b"/mnt/stale1", b"/mnt/stale2"], self.mount_table.unmount_force_calls
        )

    def test_ignores_noneden_mounts(self):
        self.mount_table.add_mount("/", device="/dev/sda1", vfstype="ext4")
        result = self.check.do_check(dry_run=False)
        self.assertEqual(doctor.CheckResultType.NO_ISSUE, result.result_type)
        self.assertEqual("", result.message)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_gives_up_if_cannot_stat_active_mount(self):
        self.mount_table.fail_access("/mnt/active1", errno.ENOENT)
        self.mount_table.fail_access("/mnt/active1/.eden", errno.ENOENT)

        result = self.check.do_check(dry_run=False)
        self.assertEqual(doctor.CheckResultType.FAILED_TO_FIX, result.result_type)
        self.assertEqual(
            "Failed to lstat active eden mount b'/mnt/active1'\n", result.message
        )

    @patch("eden.cli.doctor.log.warning")
    def test_does_not_unmount_if_cannot_stat_stale_mount(self, warning):
        self.mount_table.add_mount("/mnt/stale1")
        self.mount_table.fail_access("/mnt/stale1", errno.EACCES)
        self.mount_table.fail_access("/mnt/stale1/.eden", errno.EACCES)

        result = self.check.do_check(dry_run=False)
        self.assertEqual(doctor.CheckResultType.NO_ISSUE, result.result_type)
        self.assertEqual("", result.message)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)
        # Verify that the reason for skipping this mount is logged.
        warning.assert_called_once_with(
            "Unclear whether /mnt/stale1 is stale or not. "
            "lstat() failed: [Errno 13] Permission denied"
        )

    def test_does_unmount_if_stale_mount_is_unconnected(self):
        self.mount_table.add_stale_mount("/mnt/stale1")

        result = self.check.do_check(dry_run=False)
        self.assertEqual(doctor.CheckResultType.FIXED, result.result_type)
        self.assertEqual(
            "Unmounted 1 stale edenfs mount point:\n  /mnt/stale1\n", result.message
        )
        self.assertEqual([b"/mnt/stale1"], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_does_not_unmount_other_users_mounts(self):
        self.mount_table.add_mount("/mnt/stale1", uid=os.getuid() + 1)

        result = self.check.do_check(dry_run=False)
        self.assertEqual("", result.message)
        self.assertEqual(doctor.CheckResultType.NO_ISSUE, result.result_type)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_does_not_unmount_mounts_with_same_device_as_active_mount(self):
        active1_dev = self.mount_table.lstat("/mnt/active1").st_dev
        self.mount_table.add_mount("/mnt/stale1", dev=active1_dev)

        result = self.check.do_check(dry_run=False)
        self.assertEqual("", result.message)
        self.assertEqual(doctor.CheckResultType.NO_ISSUE, result.result_type)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)


def _create_watchman_subscription(
    filewatcher_subscription: Optional[str] = None,
    include_hg_subscriptions: bool = True,
) -> Dict:
    subscribers = []
    if filewatcher_subscription is not None:
        subscribers.append(
            {
                "info": {
                    "name": filewatcher_subscription,
                    "query": {
                        "empty_on_fresh_instance": True,
                        "defer_vcs": False,
                        "fields": ["name", "new", "exists", "mode"],
                        "relative_root": "fbcode",
                        "since": "c:1511985586:2749065:2774073346:354",
                    },
                }
            }
        )
    if include_hg_subscriptions:
        for name in doctor.NUCLIDE_HG_SUBSCRIPTIONS:
            subscribers.append(
                {
                    "info": {
                        "name": name,
                        "query": {
                            "empty_on_fresh_instance": True,
                            "fields": ["name", "new", "exists", "mode"],
                        },
                    }
                }
            )
    return {"subscribers": subscribers}


class FakeClient:
    def __init__(self):
        self._mounts = []

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_traceback):
        pass

    def listMounts(self):
        return self._mounts


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
        self._fake_client = FakeClient()

    def get_mount_paths(self) -> Iterable[str]:
        return self._mount_paths.keys()

    def check_health(self) -> config_mod.HealthStatus:
        status = fb_status.ALIVE if self._is_healthy else fb_status.STOPPED
        return config_mod.HealthStatus(status, pid=None, detail="")

    def get_client_info(self, mount_path: str) -> Dict[str, str]:
        return self._mount_paths[mount_path]

    def get_server_build_info(self) -> Dict[str, str]:
        return dict(self._build_info)

    def get_thrift_client(self) -> FakeClient:
        return self._fake_client


class FakeMountTable(mtab.MountTable):
    def __init__(self):
        self.mounts: List[mtab.MountInfo] = []
        self.unmount_lazy_calls: List[bytes] = []
        self.unmount_force_calls: List[bytes] = []
        self.unmount_lazy_fails: Set[bytes] = set()
        self.unmount_force_fails: Set[bytes] = set()
        self.stats: Dict[str, Union[mtab.MTStat, Exception]] = {}
        self._next_dev: int = 10

    def add_mount(
        self,
        path: str,
        uid: Optional[int] = None,
        dev: Optional[int] = None,
        device: str = "edenfs",
        vfstype: str = "fuse",
    ) -> None:
        if uid is None:
            uid = os.getuid()
        if dev is None:
            dev = self._next_dev
        self._next_dev += 1

        self._add_mount_info(path, device=device, vfstype=vfstype)
        self.stats[path] = mtab.MTStat(st_uid=uid, st_dev=dev)
        if device == "edenfs":
            self.stats[os.path.join(path, ".eden")] = mtab.MTStat(
                st_uid=uid, st_dev=dev
            )

    def add_stale_mount(
        self, path: str, uid: Optional[int] = None, dev: Optional[int] = None
    ) -> None:
        # Stale mounts are always edenfs FUSE mounts
        self.add_mount(path, uid=uid, dev=dev)
        # Stale mounts still successfully respond to stat() calls for the root
        # directory itself, but fail stat() calls to any other path with
        # ENOTCONN
        self.fail_access(os.path.join(path, ".eden"), errno.ENOTCONN)

    def fail_access(self, path: str, errnum: int) -> None:
        self.stats[path] = OSError(errnum, os.strerror(errnum))

    def _add_mount_info(self, path: str, device: str, vfstype: str):
        self.mounts.append(
            mtab.MountInfo(
                device=device.encode("utf-8"),
                mount_point=os.fsencode(path),
                vfstype=vfstype.encode("utf-8"),
            )
        )

    def fail_unmount_lazy(self, *mounts: bytes):
        self.unmount_lazy_fails |= set(mounts)

    def fail_unmount_force(self, *mounts: bytes):
        self.unmount_force_fails |= set(mounts)

    def read(self) -> List[mtab.MountInfo]:
        return self.mounts

    def unmount_lazy(self, mount_point: bytes) -> bool:
        self.unmount_lazy_calls.append(mount_point)

        if mount_point in self.unmount_lazy_fails:
            return False
        self._remove_mount(mount_point)
        return True

    def unmount_force(self, mount_point: bytes) -> bool:
        self.unmount_force_calls.append(mount_point)

        if mount_point in self.unmount_force_fails:
            return False
        self._remove_mount(mount_point)
        return True

    def lstat(self, path: Union[bytes, str]) -> mtab.MTStat:
        # If the input is bytes decode it to a string
        if isinstance(path, bytes):
            path = os.fsdecode(path)

        try:
            result = self.stats[path]
        except KeyError:
            raise OSError(errno.ENOENT, f"no path {path}")

        if isinstance(result, BaseException):
            raise result
        else:
            return result

    def _remove_mount(self, mount_point: bytes):
        self.mounts[:] = [
            mount_info
            for mount_info in self.mounts
            if mount_info.mount_point != mount_point
        ]
