#!/usr/bin/env python3
#
# Copyright (c) 2017-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import collections
import errno
import os
import re
import shutil
import stat
import subprocess
import typing
import unittest
from pathlib import Path
from types import SimpleNamespace
from typing import (
    Any,
    Callable,
    Dict,
    Iterable,
    List,
    NamedTuple,
    Optional,
    Set,
    Tuple,
    Union,
)
from unittest.mock import call, patch

import eden.cli.doctor as doctor
import eden.cli.process_finder
import eden.cli.ui
import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
from eden.cli import filesystem, mtab, process_finder, util
from eden.cli.config import CheckoutConfig, EdenCheckout, EdenInstance, HealthStatus
from eden.cli.doctor import (
    check_bind_mounts,
    check_hg,
    check_os,
    check_rogue_edenfs,
    check_stale_mounts,
    check_watchman,
)
from eden.test_support.temporary_directory import TemporaryDirectoryMixin
from fb303.ttypes import fb_status

from .lib.output import TestOutput


class DoctorTestBase(unittest.TestCase, TemporaryDirectoryMixin):
    def create_fixer(self, dry_run: bool) -> Tuple[doctor.ProblemFixer, TestOutput]:
        out = TestOutput()
        if not dry_run:
            fixer = doctor.ProblemFixer(out)
        else:
            fixer = doctor.DryRunFixer(out)
        return fixer, out

    def assert_results(
        self,
        fixer: doctor.ProblemFixer,
        num_problems: int = 0,
        num_fixed_problems: int = 0,
        num_failed_fixes: int = 0,
        num_manual_fixes: int = 0,
    ) -> None:
        self.assertEqual(num_problems, fixer.num_problems)
        self.assertEqual(num_fixed_problems, fixer.num_fixed_problems)
        self.assertEqual(num_failed_fixes, fixer.num_failed_fixes)
        self.assertEqual(num_manual_fixes, fixer.num_manual_fixes)


class DoctorTest(DoctorTestBase):
    # The diffs for what is written to stdout can be large.
    maxDiff = None

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    @patch("eden.cli.doctor.check_watchman._get_roots_for_nuclide")
    def test_end_to_end_test_with_various_scenarios(
        self, mock_get_roots_for_nuclide, mock_watchman
    ):
        side_effects: List[Dict[str, Any]] = []
        calls = []
        instance = FakeEdenInstance(self.make_temporary_directory())

        # In edenfs_path1, we will break the snapshot check.
        edenfs_path1_snapshot = "abcd" * 10
        edenfs_path1_dirstate_parent = "12345678" * 5
        edenfs_path1 = str(
            instance.create_test_mount(
                "path1",
                snapshot=edenfs_path1_snapshot,
                dirstate_parent=edenfs_path1_dirstate_parent,
            ).path
        )

        # In edenfs_path2, we will break the inotify check and the Nuclide
        # subscriptions check.
        edenfs_path2 = str(
            instance.create_test_mount("path2", scm_type="git", setup_path=False).path
        )

        # In edenfs_path3, we do not create the .hg directory
        edenfs_path3 = str(instance.create_test_mount("path3", setup_path=False).path)
        os.makedirs(edenfs_path3)

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
                filewatcher_subscriptions=[f"filewatcher-{edenfs_path1}"]
            )
        )

        calls.append(call(["watch-project", edenfs_path2]))
        side_effects.append({"watcher": "inotify"})
        calls.append(call(["watch-del", edenfs_path2]))
        side_effects.append({"watch-del": True, "root": edenfs_path2})
        calls.append(call(["watch-project", edenfs_path2]))
        side_effects.append({"watcher": "eden"})

        calls.append(call(["debug-get-subscriptions", edenfs_path2]))
        side_effects.append(_create_watchman_subscription(filewatcher_subscriptions=[]))

        calls.append(call(["watch-project", edenfs_path3]))
        side_effects.append({"watcher": "eden"})
        calls.append(call(["debug-get-subscriptions", edenfs_path3]))
        side_effects.append(
            _create_watchman_subscription(
                filewatcher_subscriptions=[f"filewatcher-{edenfs_path3}"]
            )
        )

        mock_watchman.side_effect = side_effects

        out = TestOutput()
        dry_run = False

        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )

        self.assertEqual(
            f"""\
Checking {edenfs_path1}
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {edenfs_path1}/.hg:
  mercurial's parent commit is {edenfs_path1_dirstate_parent}, \
but Eden's internal parent commit is {edenfs_path1_snapshot}
Repairing hg directory contents for {edenfs_path1}...<green>fixed<reset>

Checking {edenfs_path2}
<yellow>- Found problem:<reset>
Watchman is watching {edenfs_path2} with the wrong watcher type: \
"inotify" instead of "eden"
Fixing watchman watch for {edenfs_path2}...<green>fixed<reset>

<yellow>- Found problem:<reset>
Nuclide appears to be used to edit the following directories
under {edenfs_path2}:

  {edenfs_path2}

but the following Watchman subscriptions appear to be missing:

  filewatcher-{edenfs_path2}

This can cause file changes to fail to show up in Nuclide.
Currently, the only workaround for this is to run
"Nuclide Remote Projects: Kill And Restart" from the
command palette in Atom.

Checking {edenfs_path3}
<yellow>- Found problem:<reset>
{edenfs_path3}/.hg/dirstate is missing
The most common cause of this is if you previously tried to manually remove this eden
mount with "rm -rf".  You should instead remove it using "eden rm {edenfs_path3}",
and can re-clone the checkout afterwards if desired.

<yellow>Successfully fixed 2 problems.<reset>
<yellow>2 issues require manual attention.<reset>
Ask in the Eden Users group if you need help fixing issues with Eden:
https://fb.facebook.com/groups/eden.users/
""",
            out.getvalue(),
        )
        mock_watchman.assert_has_calls(calls)
        self.assertEqual(1, exit_code)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    @patch("eden.cli.doctor.check_watchman._get_roots_for_nuclide", return_value=set())
    def test_not_all_mounts_have_watchman_watcher(
        self, mock_get_roots_for_nuclide, mock_watchman
    ):
        instance = FakeEdenInstance(self.make_temporary_directory())
        edenfs_path = str(instance.create_test_mount("eden-mount", scm_type="git").path)
        edenfs_path_not_watched = str(
            instance.create_test_mount("eden-mount-not-watched", scm_type="git").path
        )
        side_effects: List[Dict[str, Any]] = []
        calls = []

        calls.append(call(["watch-list"]))
        side_effects.append({"roots": [edenfs_path]})
        calls.append(call(["watch-project", edenfs_path]))
        side_effects.append({"watcher": "eden"})
        mock_watchman.side_effect = side_effects

        out = TestOutput()
        dry_run = False
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            mount_table=instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )

        self.assertEqual(
            f"Checking {edenfs_path}\n"
            f"Checking {edenfs_path_not_watched}\n"
            "<green>No issues detected.<reset>\n",
            out.getvalue(),
        )
        mock_watchman.assert_has_calls(calls)
        self.assertEqual(0, exit_code)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    @patch("eden.cli.doctor.check_watchman._get_roots_for_nuclide")
    def test_eden_not_in_use(self, mock_get_roots_for_nuclide, mock_watchman):
        instance = FakeEdenInstance(
            self.make_temporary_directory(), status=fb_status.DEAD
        )

        out = TestOutput()
        dry_run = False
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            FakeMountTable(),
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )

        self.assertEqual("Eden is not in use.\n", out.getvalue())
        self.assertEqual(0, exit_code)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    @patch("eden.cli.doctor.check_watchman._get_roots_for_nuclide")
    def test_edenfs_not_running(self, mock_get_roots_for_nuclide, mock_watchman):
        instance = FakeEdenInstance(
            self.make_temporary_directory(), status=fb_status.DEAD
        )
        instance.create_test_mount("eden-mount")

        out = TestOutput()
        dry_run = False
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            FakeMountTable(),
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )

        self.assertEqual(
            """\
<yellow>- Found problem:<reset>
Eden is not running.
To start Eden, run:

    eden start

<yellow>1 issue requires manual attention.<reset>
Ask in the Eden Users group if you need help fixing issues with Eden:
https://fb.facebook.com/groups/eden.users/
""",
            out.getvalue(),
        )
        self.assertEqual(1, exit_code)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    @patch("eden.cli.doctor.check_watchman._get_roots_for_nuclide")
    def test_edenfs_starting(self, mock_get_roots_for_nuclide, mock_watchman):
        instance = FakeEdenInstance(
            self.make_temporary_directory(), status=fb_status.STARTING
        )
        instance.create_test_mount("eden-mount")

        out = TestOutput()
        dry_run = False
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            FakeMountTable(),
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )

        self.assertEqual(
            """\
<yellow>- Found problem:<reset>
Eden is currently still starting.
Please wait for edenfs to finish starting.
If Eden seems to be taking too long to start you can try restarting it
with "eden restart"

<yellow>1 issue requires manual attention.<reset>
Ask in the Eden Users group if you need help fixing issues with Eden:
https://fb.facebook.com/groups/eden.users/
""",
            out.getvalue(),
        )
        self.assertEqual(1, exit_code)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    @patch("eden.cli.doctor.check_watchman._get_roots_for_nuclide")
    def test_edenfs_stopping(self, mock_get_roots_for_nuclide, mock_watchman):
        instance = FakeEdenInstance(
            self.make_temporary_directory(), status=fb_status.STOPPING
        )
        instance.create_test_mount("eden-mount")

        out = TestOutput()
        dry_run = False
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            FakeMountTable(),
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )

        self.assertEqual(
            """\
<yellow>- Found problem:<reset>
Eden is currently shutting down.
Either wait for edenfs to exit, or to forcibly kill Eden, run:

    eden stop --kill

<yellow>1 issue requires manual attention.<reset>
Ask in the Eden Users group if you need help fixing issues with Eden:
https://fb.facebook.com/groups/eden.users/
""",
            out.getvalue(),
        )
        self.assertEqual(1, exit_code)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_no_issue_when_watchman_using_eden_watcher(self, mock_watchman):
        fixer, out = self._test_watchman_watcher_check(
            mock_watchman, initial_watcher="eden"
        )
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_fix_when_watchman_using_inotify_watcher(self, mock_watchman):
        fixer, out = self._test_watchman_watcher_check(
            mock_watchman, initial_watcher="inotify", new_watcher="eden", dry_run=False
        )
        self.assertEqual(
            (
                "<yellow>- Found problem:<reset>\n"
                "Watchman is watching /path/to/eden-mount with the wrong watcher type: "
                '"inotify" instead of "eden"\n'
                "Fixing watchman watch for /path/to/eden-mount...<green>fixed<reset>\n"
                "\n"
            ),
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_dry_run_identifies_inotify_watcher_issue(self, mock_watchman):
        fixer, out = self._test_watchman_watcher_check(
            mock_watchman, initial_watcher="inotify", dry_run=True
        )
        self.assertEqual(
            (
                "<yellow>- Found problem:<reset>\n"
                "Watchman is watching /path/to/eden-mount with the wrong watcher type: "
                '"inotify" instead of "eden"\n'
                "Would fix watchman watch for /path/to/eden-mount\n"
                "\n"
            ),
            out,
        )
        self.assert_results(fixer, num_problems=1)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_doctor_reports_failure_if_cannot_replace_inotify_watcher(
        self, mock_watchman
    ):
        fixer, out = self._test_watchman_watcher_check(
            mock_watchman,
            initial_watcher="inotify",
            new_watcher="inotify",
            dry_run=False,
        )
        self.assertEqual(
            (
                "<yellow>- Found problem:<reset>\n"
                "Watchman is watching /path/to/eden-mount with the wrong watcher type: "
                '"inotify" instead of "eden"\n'
                "Fixing watchman watch for /path/to/eden-mount...<red>error<reset>\n"
                "Failed to fix problem: Failed to replace watchman watch for "
                '/path/to/eden-mount with an "eden" watcher\n'
                "\n"
            ),
            out,
        )
        self.assert_results(fixer, num_problems=1, num_failed_fixes=1)

    def _test_watchman_watcher_check(
        self,
        mock_watchman,
        initial_watcher: str,
        new_watcher: Optional[str] = None,
        dry_run: bool = True,
    ) -> Tuple[doctor.ProblemFixer, str]:
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

        fixer, out = self.create_fixer(dry_run)

        watchman_roots = {edenfs_path}
        watchman_info = check_watchman.WatchmanCheckInfo(watchman_roots, None)
        check_watchman.check_active_mount(fixer, edenfs_path, watchman_info)

        mock_watchman.assert_has_calls(calls)
        return fixer, out.getvalue()

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_no_issue_when_expected_nuclide_subscriptions_present(self, mock_watchman):
        fixer, out = self._test_nuclide_check(
            mock_watchman=mock_watchman, include_filewatcher_subscriptions=True
        )
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_no_issue_when_path_not_in_nuclide_roots(self, mock_watchman):
        fixer, out = self._test_nuclide_check(
            mock_watchman=mock_watchman, include_path_in_nuclide_roots=False
        )
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_watchman_subscriptions_are_missing(self, mock_watchman):
        fixer, out = self._test_nuclide_check(
            mock_watchman=mock_watchman, include_hg_subscriptions=False, dry_run=False
        )
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
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
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_filewatcher_watchman_subscription_has_duplicate(self, mock_watchman):
        fixer, out = self._test_nuclide_check(
            mock_watchman=mock_watchman,
            include_hg_subscriptions=False,
            dry_run=False,
            include_filewatcher_subscriptions=2,
        )
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Nuclide appears to be used to edit the following directories
under /path/to/eden-mount:

  /path/to/eden-mount/subdirectory

but the following Watchman subscriptions appear to be missing:

  hg-repository-watchman-subscription-primary
  hg-repository-watchman-subscription-conflicts
  hg-repository-watchman-subscription-hgbookmark
  hg-repository-watchman-subscription-hgbookmarks
  hg-repository-watchman-subscription-dirstate
  hg-repository-watchman-subscription-progress
  hg-repository-watchman-subscription-lock-files

and the following Watchman subscriptions have duplicates:

  filewatcher-/path/to/eden-mount/subdirectory

This can cause file changes to fail to show up in Nuclide.
Currently, the only workaround for this is to run
"Nuclide Remote Projects: Kill And Restart" from the
command palette in Atom.

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_filewatcher_subscription_is_missing_dry_run(self, mock_watchman):
        fixer, out = self._test_nuclide_check(mock_watchman=mock_watchman)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Nuclide appears to be used to edit the following directories
under /path/to/eden-mount:

  /path/to/eden-mount/subdirectory

but the following Watchman subscriptions appear to be missing:

  filewatcher-/path/to/eden-mount/subdirectory

This can cause file changes to fail to show up in Nuclide.
Currently, the only workaround for this is to run
"Nuclide Remote Projects: Kill And Restart" from the
command palette in Atom.

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def _test_nuclide_check(
        self,
        mock_watchman,
        dry_run: bool = True,
        include_filewatcher_subscriptions: int = 0,
        include_path_in_nuclide_roots: bool = True,
        include_hg_subscriptions: bool = True,
    ) -> Tuple[doctor.ProblemFixer, str]:
        edenfs_path = "/path/to/eden-mount"
        side_effects: List[Dict[str, Any]] = []
        watchman_calls = []

        if include_path_in_nuclide_roots:
            watchman_calls.append(call(["debug-get-subscriptions", edenfs_path]))

        nuclide_root = os.path.join(edenfs_path, "subdirectory")
        # Note that a "filewatcher-" subscription in a subdirectory of the
        # Eden mount should signal that the proper Watchman subscription is
        # set up.
        filewatcher_sub: List[str] = [
            f"filewatcher-{nuclide_root}"
        ] * include_filewatcher_subscriptions

        unrelated_path = "/path/to/non-eden-mount"
        if include_path_in_nuclide_roots:
            nuclide_roots = {nuclide_root, unrelated_path}
        else:
            nuclide_roots = {unrelated_path}

        side_effects.append(
            _create_watchman_subscription(
                filewatcher_subscriptions=filewatcher_sub,
                include_hg_subscriptions=include_hg_subscriptions,
            )
        )
        mock_watchman.side_effect = side_effects
        watchman_roots = {edenfs_path}

        fixer, out = self.create_fixer(dry_run)
        watchman_info = check_watchman.WatchmanCheckInfo(watchman_roots, nuclide_roots)
        check_watchman.check_nuclide_subscriptions(fixer, edenfs_path, watchman_info)

        mock_watchman.assert_has_calls(watchman_calls)
        return fixer, out.getvalue()

    def test_snapshot_and_dirstate_file_match(self):
        dirstate_hash_hex = "12345678" * 5
        snapshot_hex = "12345678" * 5
        _checkout, fixer, out = self._test_hash_check(dirstate_hash_hex, snapshot_hex)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_snapshot_and_dirstate_file_differ(self):
        dirstate_hash_hex = "12000000" * 5
        snapshot_hex = "12345678" * 5
        checkout, fixer, out = self._test_hash_check(dirstate_hash_hex, snapshot_hex)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {checkout.path}/.hg:
  mercurial's parent commit is 1200000012000000120000001200000012000000, \
but Eden's internal parent commit is \
1234567812345678123456781234567812345678
Repairing hg directory contents for {checkout.path}...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)
        # Make sure resetParentCommits() was called once with the expected arguments
        self.assertEqual(
            checkout.instance.get_thrift_client().set_parents_calls,
            [
                ResetParentsCommitsArgs(
                    mount=bytes(checkout.path),
                    parent1=b"\x12\x00\x00\x00" * 5,
                    parent2=None,
                )
            ],
        )

    def test_snapshot_and_dirstate_file_differ_and_snapshot_invalid(self):
        def check_commit_validity(path: bytes, commit: str) -> bool:
            if commit == "12345678" * 5:
                return False
            return True

        dirstate_hash_hex = "12000000" * 5
        snapshot_hex = "12345678" * 5
        checkout, fixer, out = self._test_hash_check(
            dirstate_hash_hex, snapshot_hex, commit_checker=check_commit_validity
        )
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {checkout.path}/.hg:
  Eden's snapshot file points to a bad commit: {snapshot_hex}
Repairing hg directory contents for {checkout.path}...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)
        # Make sure resetParentCommits() was called once with the expected arguments
        self.assertEqual(
            checkout.instance.get_thrift_client().set_parents_calls,
            [
                ResetParentsCommitsArgs(
                    mount=bytes(checkout.path),
                    parent1=b"\x12\x00\x00\x00" * 5,
                    parent2=None,
                )
            ],
        )

    @patch(
        "eden.cli.doctor.check_hg.get_tip_commit_hash",
        return_value=b"\x87\x65\x43\x21" * 5,
    )
    def test_snapshot_and_dirstate_file_differ_and_all_commit_hash_invalid(
        self, mock_get_tip_commit_hash
    ):
        def check_commit_validity(path: bytes, commit: str) -> bool:
            return False

        dirstate_hash_hex = "12000000" * 5
        snapshot_hex = "12345678" * 5
        valid_commit_hash = "87654321" * 5
        checkout, fixer, out = self._test_hash_check(
            dirstate_hash_hex, snapshot_hex, commit_checker=check_commit_validity
        )

        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {checkout.path}/.hg:
  mercurial's p0 commit points to a bad commit: {dirstate_hash_hex}
  Eden's snapshot file points to a bad commit: {snapshot_hex}
Repairing hg directory contents for {checkout.path}...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)
        # Make sure resetParentCommits() was called once with the expected arguments
        self.assertEqual(
            checkout.instance.get_thrift_client().set_parents_calls,
            [
                ResetParentsCommitsArgs(
                    mount=bytes(checkout.path),
                    parent1=b"\x87\x65\x43\x21" * 5,
                    parent2=None,
                )
            ],
        )
        self.assert_dirstate_p0(checkout, valid_commit_hash)

    @patch(
        "eden.cli.doctor.check_hg.get_tip_commit_hash",
        return_value=b"\x87\x65\x43\x21" * 5,
    )
    def test_snapshot_and_dirstate_file_differ_and_all_parents_invalid(
        self, mock_get_tip_commit_hash
    ):
        def check_commit_validity(path: bytes, commit: str) -> bool:
            return False

        dirstate_hash_hex = "12000000" * 5
        dirstate_parent2_hash_hex = "12340000" * 5
        snapshot_hex = "12345678" * 5
        valid_commit_hash = "87654321" * 5
        checkout, fixer, out = self._test_hash_check(
            dirstate_hash_hex,
            snapshot_hex,
            dirstate_parent2_hash_hex,
            commit_checker=check_commit_validity,
        )

        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {checkout.path}/.hg:
  mercurial's p0 commit points to a bad commit: {dirstate_hash_hex}
  mercurial's p1 commit points to a bad commit: {dirstate_parent2_hash_hex}
  Eden's snapshot file points to a bad commit: {snapshot_hex}
Repairing hg directory contents for {checkout.path}...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)
        # Make sure resetParentCommits() was called once with the expected arguments
        self.assertEqual(
            checkout.instance.get_thrift_client().set_parents_calls,
            [
                ResetParentsCommitsArgs(
                    mount=bytes(checkout.path),
                    parent1=b"\x87\x65\x43\x21" * 5,
                    parent2=None,
                )
            ],
        )
        self.assert_dirstate_p0(checkout, valid_commit_hash)

    def test_snapshot_and_dirstate_file_differ_and_dirstate_commit_hash_invalid(self):
        def check_commit_validity(path: bytes, commit: str) -> bool:
            if commit == "12000000" * 5:
                return False
            return True

        dirstate_hash_hex = "12000000" * 5
        snapshot_hex = "12345678" * 5
        checkout, fixer, out = self._test_hash_check(
            dirstate_hash_hex, snapshot_hex, commit_checker=check_commit_validity
        )

        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {checkout.path}/.hg:
  mercurial's p0 commit points to a bad commit: {dirstate_hash_hex}
Repairing hg directory contents for {checkout.path}...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)
        # The dirstate file should have been updated to use the snapshot hash
        self.assertEqual(checkout.instance.get_thrift_client().set_parents_calls, [])
        self.assert_dirstate_p0(checkout, snapshot_hex)

    def assert_dirstate_p0(self, checkout: EdenCheckout, commit: str) -> None:
        dirstate_path = checkout.path / ".hg" / "dirstate"
        with dirstate_path.open("rb") as f:
            parents, self._tuples_dict, self._copymap = eden.dirstate.read(
                f, str(dirstate_path)
            )
        self.assertEqual(binascii.hexlify(parents[0]).decode("utf-8"), commit)

    def _test_hash_check(
        self,
        dirstate_hash_hex: str,
        snapshot_hex: str,
        dirstate_parent2_hash_hex=None,
        commit_checker: Optional[Callable[[bytes, str], bool]] = None,
    ) -> Tuple[EdenCheckout, doctor.ProblemFixer, str]:
        instance = FakeEdenInstance(self.make_temporary_directory())
        if dirstate_parent2_hash_hex is None:
            checkout = instance.create_test_mount(
                "path1", snapshot=snapshot_hex, dirstate_parent=dirstate_hash_hex
            )
        else:
            checkout = instance.create_test_mount(
                "path1",
                snapshot=snapshot_hex,
                dirstate_parent=(dirstate_hash_hex, dirstate_parent2_hash_hex),
            )

        if commit_checker:
            client = typing.cast(FakeClient, checkout.instance.get_thrift_client())
            client.commit_checker = commit_checker

        fixer, out = self.create_fixer(dry_run=False)
        check_hg.check_hg(fixer, checkout)
        return checkout, fixer, out.getvalue()

    @patch("eden.cli.version.get_installed_eden_rpm_version")
    def test_edenfs_when_installed_and_running_match(self, mock_gierv):
        fixer, out = self._test_edenfs_version(mock_gierv, "20171213-165642")
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    @patch("eden.cli.version.get_installed_eden_rpm_version")
    def test_edenfs_when_installed_and_running_differ(self, mock_gierv):
        fixer, out = self._test_edenfs_version(mock_gierv, "20171120-246561")
        self.assertEqual(
            """\
<yellow>- Found problem:<reset>
The version of Eden that is installed on your machine is:
    fb-eden-20171120-246561.x86_64
but the version of Eden that is currently running is:
    fb-eden-20171213-165642.x86_64

Consider running `eden restart` to migrate to the newer version, which
may have important bug fixes or performance improvements.

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def _test_edenfs_version(
        self, mock_rpm_q, rpm_value: str
    ) -> Tuple[doctor.ProblemFixer, str]:
        side_effects: List[str] = []
        calls = []
        calls.append(call())
        side_effects.append(rpm_value)
        mock_rpm_q.side_effect = side_effects

        instance = FakeEdenInstance(
            self.make_temporary_directory(),
            build_info={
                "build_package_version": "20171213",
                "build_package_release": "165642",
            },
        )
        fixer, out = self.create_fixer(dry_run=False)
        doctor.check_edenfs_version(fixer, typing.cast(EdenInstance, instance))
        mock_rpm_q.assert_has_calls(calls)
        return fixer, out.getvalue()

    @patch("eden.cli.doctor.check_watchman._get_roots_for_nuclide", return_value=set())
    def test_unconfigured_mounts_dont_crash(self, mock_get_roots_for_nuclide):
        # If Eden advertises that a mount is active, but it is not in the
        # configuration, then at least don't throw an exception.
        instance = FakeEdenInstance(self.make_temporary_directory())
        edenfs_path1 = instance.create_test_mount("path1").path
        edenfs_path2 = instance.create_test_mount("path2").path
        # Remove path2 from the list of mounts in the instance
        instance.remove_checkout_configuration(str(edenfs_path2))

        dry_run = False
        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )

        self.assertEqual(
            f"""\
Checking {edenfs_path1}
Checking {edenfs_path2}
<yellow>- Found problem:<reset>
Checkout {edenfs_path2} is running but not listed in Eden's configuration file.
Running "eden unmount {edenfs_path2}" will unmount this checkout.

<yellow>1 issue requires manual attention.<reset>
Ask in the Eden Users group if you need help fixing issues with Eden:
https://fb.facebook.com/groups/eden.users/
""",
            out.getvalue(),
        )
        self.assertEqual(1, exit_code)

    def test_remount_checkouts(self) -> None:
        exit_code, out, mounts = self._test_remount_checkouts(  # type: ignore
            dry_run=False
        )
        self.assertEqual(
            f"""\
Checking {mounts[0]}
Checking {mounts[1]}
<yellow>- Found problem:<reset>
{mounts[1]} is not currently mounted
Remounting {mounts[1]}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
            out,
        )
        self.assertEqual(exit_code, 0)

    def test_remount_checkouts_dry_run(self) -> None:
        exit_code, out, mounts = self._test_remount_checkouts(  # type: ignore
            dry_run=True
        )
        self.assertEqual(
            f"""\
Checking {mounts[0]}
Checking {mounts[1]}
<yellow>- Found problem:<reset>
{mounts[1]} is not currently mounted
Would remount {mounts[1]}

<yellow>Discovered 1 problem during --dry-run<reset>
""",
            out,
        )
        self.assertEqual(exit_code, 1)

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    @patch("eden.cli.doctor.check_watchman._get_roots_for_nuclide", return_value=set())
    def _test_remount_checkouts(
        self, mock_get_roots_for_nuclide, mock_watchman, dry_run: bool
    ) -> Tuple[int, str, List[Path]]:
        """Test that `eden doctor` remounts configured mount points that are not
        currently mounted.
        """
        tmp_dir = self.make_temporary_directory()
        instance = FakeEdenInstance(tmp_dir)

        mounts = []
        mounts.append(instance.create_test_mount("path1").path)
        mounts.append(instance.create_test_mount("path2", active=False).path)

        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            typing.cast(EdenInstance, instance),
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )
        return exit_code, out.getvalue(), mounts

    @patch("eden.cli.doctor.check_watchman._call_watchman")
    def test_watchman_fails(self, mock_watchman):
        tmp_dir = self.make_temporary_directory()
        instance = FakeEdenInstance(tmp_dir)

        mount = instance.create_test_mount("path1", active=False).path

        # Make calls to watchman fail rather than returning expected output
        side_effects = [{"error": "watchman failed"}]
        mock_watchman.side_effect = side_effects

        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            typing.cast(EdenInstance, instance),
            dry_run=False,
            mount_table=instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )

        # "watchman watch-list" should have been called by the doctor code
        calls = [call(["watch-list"])]
        mock_watchman.assert_has_calls(calls)

        self.assertEqual(
            out.getvalue(),
            f"""\
Checking {mount}
<yellow>- Found problem:<reset>
{mount} is not currently mounted
Remounting {mount}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
        )
        self.assertEqual(exit_code, 0)


class MultipleEdenfsRunningTest(DoctorTestBase):
    maxDiff = None

    def run_check(
        self, process_finder: process_finder.ProcessFinder, dry_run: bool
    ) -> Tuple[doctor.ProblemFixer, str]:
        fixer, out = self.create_fixer(dry_run)
        check_rogue_edenfs.check_many_edenfs_are_running(fixer, process_finder)
        return fixer, out.getvalue()

    def test_when_there_are_rogue_pids(self):
        process_finder = FakeProcessFinder()
        process_finder.add_edenfs(123, "/home/someuser/.eden", set_lockfile=False)
        process_finder.add_edenfs(124, "/home/someuser/.eden", set_lockfile=False)
        process_finder.add_edenfs(125, "/home/someuser/.eden", set_lockfile=True)
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 123 124

""",
        )

    def test_when_no_rogue_edenfs_process_running(self):
        process_finder = FakeProcessFinder()
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def exception_throwing_subprocess(self):
        raise subprocess.CalledProcessError(
            2, cmd="pgrep -aU " + util.get_username() + " edenfs"
        )

    @patch("subprocess.check_output", side_effect=exception_throwing_subprocess)
    def test_when_pgrep_fails(self, subprocess_function):
        linux_process_finder = process_finder.LinuxProcessFinder()
        with self.assertLogs() as logs_assertion:
            fixer, out = self.run_check(linux_process_finder, dry_run=False)
            self.assertEqual("", out)
            self.assert_results(fixer, num_problems=0)
        self.assertIn(
            "Error running command: ['pgrep', '-aU", "\n".join(logs_assertion.output)
        )

    def test_when_os_found_no_pids_at_all(self):
        process_finder = FakeProcessFinder()
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_os_found_pids_but_edenDir_not_in_cmdline(self):
        process_finder = FakeProcessFinder()
        process_finder.add_process(1_614_248, b"edenfs")
        process_finder.add_process(1_639_164, b"edenfs")
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_many_edenfs_procs_run_for_same_config(self):
        process_finder = FakeProcessFinder()
        process_finder.add_edenfs(
            475_203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=True
        )
        process_finder.add_edenfs(
            475_204, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        process_finder.add_edenfs(
            475_205, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 475204 475205

""",
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_when_other_processes_with_similar_names_running(self):
        process_finder = FakeProcessFinder()
        process_finder.add_edenfs(475_203, "/home/user/.eden")
        process_finder.add_process(
            475_204, b"/foobar/fooedenfs --edenDir /home/user/.eden --edenfs"
        )
        process_finder.add_process(
            475_205, b"/foobar/edenfsbar --edenDir /home/user/.eden --edenfs"
        )
        process_finder.add_process(
            475_206, b"/foobar/edenfs --edenDir /home/user/.eden --edenfs"
        )

        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 475206

""",
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_when_only_valid_edenfs_process_running(self):
        process_finder = FakeProcessFinder()
        process_finder.add_edenfs(475_203, "/home/someuser/.eden")
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_os_found_pids_but_edenDir_value_not_in_cmdline(self):
        process_finder = FakeProcessFinder()
        process_finder.add_process(1_614_248, b"edenfs --edenDir")
        process_finder.add_process(1_639_164, b"edenfs")
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_differently_configured_edenfs_processes_running_with_rogue_pids(self):
        process_finder = FakeProcessFinder()
        process_finder.add_edenfs(475_203, "/tmp/config1/.eden")
        process_finder.add_edenfs(475_204, "/tmp/config1/.eden", set_lockfile=False)
        process_finder.add_edenfs(475_205, "/tmp/config1/.eden", set_lockfile=False)
        process_finder.add_edenfs(575_203, "/tmp/config2/.eden")
        process_finder.add_edenfs(575_204, "/tmp/config2/.eden", set_lockfile=False)
        process_finder.add_edenfs(575_205, "/tmp/config2/.eden", set_lockfile=False)
        fixer, out = self.run_check(process_finder, dry_run=False)

        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 475204 475205 575204 575205

""",
        )

    def test_single_edenfs_process_per_dir_okay(self):
        # The rogue process finder should not complain about edenfs processes
        # when there is just a single edenfs process running per directory, even if the
        # pid file does not appear to currently contain that pid.
        #
        # The pid file check is inherently racy.  `eden doctor` may not read the correct
        # pid if edenfs was in the middle of (re)starting.  Therefore we intentionally
        # only report rogue processes when we can actually confirm there is more than
        # one edenfs process running for a given directory.
        process_finder = FakeProcessFinder()
        # In config1/ replace the lock file contents with a different process ID
        process_finder.add_edenfs(123_203, "/tmp/config1/.eden", set_lockfile=False)
        process_finder.set_file_contents("/tmp/config1/.eden/lock", "9765\n")
        # In config2/ do not write a lock file at all
        process_finder.add_edenfs(123_456, "/tmp/config2/.eden", set_lockfile=False)
        # In config3/ report two separate edenfs processes, with one legitimate rogue
        # process
        process_finder.add_edenfs(123_900, "/tmp/config3/.eden")
        process_finder.add_edenfs(123_901, "/tmp/config3/.eden", set_lockfile=False)

        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 123901

""",
        )

    def test_when_lock_file_op_has_io_exception(self):
        process_finder = FakeProcessFinder()
        process_finder.add_edenfs(
            475_203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        process_finder.add_edenfs(
            475_204, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        with self.assertLogs() as logs_assertion:
            fixer, out = self.run_check(process_finder, dry_run=False)
            self.assertEqual("", out)
            self.assert_results(fixer, num_problems=0)
            logs = "\n".join(logs_assertion.output)
            self.assertIn(
                "WARNING:eden.cli.process_finder:Lock file cannot be read for",
                logs,
                "when lock file can't be opened",
            )

    def test_when_lock_file_data_is_garbage(self):
        process_finder = FakeProcessFinder()
        process_finder.add_edenfs(
            475_203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        process_finder.add_edenfs(
            475_204, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        process_finder.set_file_contents("/tmp/eden_test.68yxptnx/.eden/lock", "asdf")
        with self.assertLogs() as logs_assertion:
            fixer, out = self.run_check(process_finder, dry_run=False)
            self.assertEqual("", out)
            self.assert_results(fixer, num_problems=0)
        self.assertIn(
            "lock file contains data that cannot be parsed",
            "\n".join(logs_assertion.output),
        )


class BindMountsCheckTest(DoctorTestBase):
    maxDiff = None

    def setUp(self) -> None:
        tmp_dir = self.make_temporary_directory()
        self.instance = FakeEdenInstance(tmp_dir)

        self.fbsource_client = os.path.join(self.instance.clients_path, "fbsource")
        self.fbsource_bind_mounts = os.path.join(self.fbsource_client, "bind-mounts")
        self.edenfs_path1 = str(
            self.instance.create_test_mount(
                "path1",
                bind_mounts={
                    "fbcode-buck-out": "fbcode/buck-out",
                    "fbandroid-buck-out": "fbandroid/buck-out",
                    "buck-out": "buck-out",
                },
                client_name="fbsource",
                setup_path=False,
            ).path
        )

        # Entries for later inclusion in client bind mount table
        self.client_bm1 = os.path.join(self.fbsource_bind_mounts, "fbcode-buck-out")
        self.client_bm2 = os.path.join(self.fbsource_bind_mounts, "fbandroid-buck-out")
        self.client_bm3 = os.path.join(self.fbsource_bind_mounts, "buck-out")

        # Entries for later inclusion in bind mount table
        self.bm1 = os.path.join(self.edenfs_path1, "fbcode/buck-out")
        self.bm2 = os.path.join(self.edenfs_path1, "fbandroid/buck-out")
        self.bm3 = os.path.join(self.edenfs_path1, "buck-out")

    def run_check(
        self,
        mount_table: mtab.MountTable,
        dry_run: bool,
        fs_util: Optional["FakeFsUtil"] = None,
    ) -> Tuple[doctor.ProblemFixer, str]:
        fixer, out = self.create_fixer(dry_run)
        if fs_util is None:
            fs_util = FakeFsUtil()
        checkout = EdenCheckout(
            typing.cast(EdenInstance, self.instance),
            Path(self.edenfs_path1),
            Path(self.fbsource_client),
        )
        check_bind_mounts.check_bind_mounts(
            fixer, checkout, mount_table=mount_table, fs_util=fs_util
        )
        return fixer, out.getvalue()

    def test_bind_mounts_okay(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_bind_mounts_missing_dry_run(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=True)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbcode/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/fbcode/buck-out

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbandroid/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/fbandroid/buck-out

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/buck-out

""",
            out,
        )
        self.assert_results(fixer, num_problems=3)

    def test_bind_mounts_missing(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        mount_table.bind_mount_success_paths[self.client_bm1] = self.bm1
        mount_table.bind_mount_success_paths[self.client_bm2] = self.bm2
        mount_table.bind_mount_success_paths[self.client_bm3] = self.bm3

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbcode/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/fbcode/buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbandroid/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/fbandroid/buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/buck-out...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=3, num_fixed_problems=3)

    def test_bind_mounts_missing_fail(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # These bound mind operations will succeed.
        mount_table.bind_mount_success_paths[self.client_bm1] = self.bm1

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbcode/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/fbcode/buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbandroid/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/fbandroid/buck-out...<red>error<reset>
Failed to fix problem: Command 'sudo mount -o bind \
{self.fbsource_bind_mounts}/fbandroid-buck-out \
{self.edenfs_path1}/fbandroid/buck-out' returned non-zero exit status 1.

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/buck-out...<red>error<reset>
Failed to fix problem: Command \
'sudo mount -o bind {self.fbsource_bind_mounts}/buck-out \
{self.edenfs_path1}/buck-out' returned non-zero exit status 1.

""",
            out,
        )
        self.assert_results(
            fixer, num_problems=3, num_fixed_problems=1, num_failed_fixes=2
        )

    def test_bind_mounts_and_dir_missing_dry_run(self):
        mount_table = FakeMountTable()

        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        fixer, out = self.run_check(mount_table, dry_run=True)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbcode-buck-out
Would create directory {self.fbsource_bind_mounts}/fbcode-buck-out

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbcode/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/fbcode/buck-out

<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbandroid-buck-out
Would create directory {self.fbsource_bind_mounts}/fbandroid-buck-out

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbandroid/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/fbandroid/buck-out

<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/buck-out
Would create directory {self.fbsource_bind_mounts}/buck-out

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/buck-out

""",
            out,
        )
        self.assert_results(fixer, num_problems=6)

    def test_bind_mount_wrong_device_dry_run(self):
        # bm1, bm2 should not have same device as edenfs
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=True)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbcode/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/fbcode/buck-out

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbandroid/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/fbandroid/buck-out

""",
            out,
        )
        self.assert_results(fixer, num_problems=2)

    def test_bind_mount_wrong_device(self):
        # bm1, bm2 should not have same device as edenfs
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # These bound mind operations will succeed.
        mount_table.bind_mount_success_paths[self.client_bm1] = self.bm1
        mount_table.bind_mount_success_paths[self.client_bm2] = self.bm2

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbcode/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/fbcode/buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbandroid/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/fbandroid/buck-out...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=2, num_fixed_problems=2)

    def test_client_mount_path_not_dir(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        # Note: client_bm3 is not a directory
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=33188
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Expected {self.fbsource_bind_mounts}/buck-out to be a directory
Please remove the file at {self.fbsource_bind_mounts}/buck-out

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_mount_path_not_dir(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        # Note: bm3 is not a directory
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=33188
        )

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Expected {self.edenfs_path1}/buck-out to be a directory
Please remove the file at {self.edenfs_path1}/buck-out

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_client_bind_mounts_missing_dry_run(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=True)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbcode-buck-out
Would create directory {self.fbsource_bind_mounts}/fbcode-buck-out

<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/buck-out
Would create directory {self.fbsource_bind_mounts}/buck-out

""",
            out,
        )
        self.assert_results(fixer, num_problems=2)

    def test_client_bind_mounts_missing(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbcode-buck-out
Creating directory {self.fbsource_bind_mounts}/fbcode-buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/buck-out
Creating directory {self.fbsource_bind_mounts}/buck-out...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=2, num_fixed_problems=2)

    def test_client_bind_mounts_missing_fail(self):
        mount_table = FakeMountTable()
        fs_util = FakeFsUtil()

        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fs_util.path_error[self.client_bm3] = "Failed to create directory"

        fixer, out = self.run_check(mount_table, dry_run=False, fs_util=fs_util)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbcode-buck-out
Creating directory {self.fbsource_bind_mounts}/fbcode-buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/buck-out
Creating directory {self.fbsource_bind_mounts}/buck-out...<red>error<reset>
Failed to fix problem: Failed to create directory

""",
            out,
        )
        self.assert_results(
            fixer, num_problems=2, num_fixed_problems=1, num_failed_fixes=1
        )

    def test_bind_mounts_and_client_dir_missing_dry_run(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=True)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbandroid-buck-out
Would create directory {self.fbsource_bind_mounts}/fbandroid-buck-out

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbandroid/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/fbandroid/buck-out

<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/buck-out
Would create directory {self.fbsource_bind_mounts}/buck-out

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/buck-out is not mounted
Would remount bind mount at {self.edenfs_path1}/buck-out

""",
            out,
        )
        self.assert_results(fixer, num_problems=4)

    def test_bind_mounts_and_client_dir_missing(self):
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # These bound mind operations will succeed.
        mount_table.bind_mount_success_paths[self.client_bm2] = self.bm2
        mount_table.bind_mount_success_paths[self.client_bm3] = self.bm3

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbandroid-buck-out
Creating directory {self.fbsource_bind_mounts}/fbandroid-buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/fbandroid/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/fbandroid/buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/buck-out
Creating directory {self.fbsource_bind_mounts}/buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Bind mount at {self.edenfs_path1}/buck-out is not mounted
Remounting bind mount at {self.edenfs_path1}/buck-out...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=4, num_fixed_problems=4)

    def test_client_bind_mount_multiple_issues_dry_run(self):
        # Bind mount 1 does not exist
        # Bind mount 2 has wrong device type
        # Bind mount 3 is a file instead of a directory
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=33188
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=True)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbcode-buck-out
Would create directory {self.fbsource_bind_mounts}/fbcode-buck-out

<yellow>- Found problem:<reset>
Expected {self.fbsource_bind_mounts}/buck-out to be a directory
Please remove the file at {self.fbsource_bind_mounts}/buck-out

""",
            out,
        )
        self.assert_results(fixer, num_problems=2, num_manual_fixes=1)

    def test_client_bind_mount_multiple_issues(self):
        # Bind mount 1 does not exist
        # Bind mount 2 has wrong device type
        # Bind mount 3 is a file instead of a directory
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        # Client bind mount paths (under .eden)
        mount_table.stats[self.edenfs_path1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=12, st_mode=16877
        )
        mount_table.stats[self.client_bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=33188
        )

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm2] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )
        mount_table.stats[self.bm3] = mtab.MTStat(
            st_uid=os.getuid(), st_dev=11, st_mode=16877
        )

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Missing client directory for bind mount {self.fbsource_bind_mounts}/fbcode-buck-out
Creating directory {self.fbsource_bind_mounts}/fbcode-buck-out...<green>fixed<reset>

<yellow>- Found problem:<reset>
Expected {self.fbsource_bind_mounts}/buck-out to be a directory
Please remove the file at {self.fbsource_bind_mounts}/buck-out

""",
            out,
        )
        self.assert_results(
            fixer, num_problems=2, num_fixed_problems=1, num_manual_fixes=1
        )


class StaleMountsCheckTest(DoctorTestBase):
    maxDiff = None

    def setUp(self) -> None:
        self.active_mounts: List[bytes] = [b"/mnt/active1", b"/mnt/active2"]
        self.mount_table = FakeMountTable()
        self.mount_table.add_mount("/mnt/active1")
        self.mount_table.add_mount("/mnt/active2")

    def run_check(self, dry_run: bool) -> Tuple[doctor.ProblemFixer, str]:
        fixer, out = self.create_fixer(dry_run)
        check_stale_mounts.check_for_stale_mounts(fixer, mount_table=self.mount_table)
        return fixer, out.getvalue()

    def test_does_not_unmount_active_mounts(self):
        fixer, out = self.run_check(dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_working_nonactive_mount_is_not_unmounted(self):
        # Add a working edenfs mount that is not part of our active list
        self.mount_table.add_mount("/mnt/other1")

        fixer, out = self.run_check(dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_force_unmounts_if_lazy_fails(self):
        self.mount_table.add_stale_mount("/mnt/stale1")
        self.mount_table.add_stale_mount("/mnt/stale2")
        self.mount_table.fail_unmount_lazy(b"/mnt/stale1")

        fixer, out = self.run_check(dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found 2 stale edenfs mounts:
  /mnt/stale1
  /mnt/stale2
Unmounting 2 stale edenfs mounts...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)
        self.assertEqual(
            [b"/mnt/stale1", b"/mnt/stale2"], self.mount_table.unmount_lazy_calls
        )
        self.assertEqual([b"/mnt/stale1"], self.mount_table.unmount_force_calls)

    def test_dry_run_prints_stale_mounts_and_does_not_unmount(self):
        self.mount_table.add_stale_mount("/mnt/stale1")
        self.mount_table.add_stale_mount("/mnt/stale2")

        fixer, out = self.run_check(dry_run=True)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found 2 stale edenfs mounts:
  /mnt/stale1
  /mnt/stale2
Would unmount 2 stale edenfs mounts

""",
            out,
        )
        self.assert_results(fixer, num_problems=1)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_fails_if_unmount_fails(self):
        self.mount_table.add_stale_mount("/mnt/stale1")
        self.mount_table.add_stale_mount("/mnt/stale2")
        self.mount_table.fail_unmount_lazy(b"/mnt/stale1", b"/mnt/stale2")
        self.mount_table.fail_unmount_force(b"/mnt/stale1")

        fixer, out = self.run_check(dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found 2 stale edenfs mounts:
  /mnt/stale1
  /mnt/stale2
Unmounting 2 stale edenfs mounts...<red>error<reset>
Failed to fix problem: Failed to unmount 1 mount point:
  /mnt/stale1

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_failed_fixes=1)
        self.assertEqual(
            [b"/mnt/stale1", b"/mnt/stale2"], self.mount_table.unmount_lazy_calls
        )
        self.assertEqual(
            [b"/mnt/stale1", b"/mnt/stale2"], self.mount_table.unmount_force_calls
        )

    def test_ignores_noneden_mounts(self):
        self.mount_table.add_mount("/", device="/dev/sda1", vfstype="ext4")
        fixer, out = self.run_check(dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_ignores_errors_other_than_enotconn(self):
        self.mount_table.fail_access("/mnt/active1", errno.EPERM)
        self.mount_table.fail_access("/mnt/active1/.eden", errno.EPERM)

        fixer, out = self.run_check(dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_does_not_unmount_if_cannot_stat_stale_mount(self):
        self.mount_table.add_mount("/mnt/stale1")
        self.mount_table.fail_access("/mnt/stale1", errno.EACCES)
        self.mount_table.fail_access("/mnt/stale1/.eden", errno.EACCES)

        with self.assertLogs() as logs_assertion:
            fixer, out = self.run_check(dry_run=False)
            self.assertEqual("", out)
            self.assertEqual(0, fixer.num_problems)
            self.assertEqual([], self.mount_table.unmount_lazy_calls)
            self.assertEqual([], self.mount_table.unmount_force_calls)
        # Verify that the reason for skipping this mount is logged.
        self.assertIn(
            "Unclear whether /mnt/stale1 is stale or not. "
            "lstat() failed: [Errno 13] Permission denied",
            "\n".join(logs_assertion.output),
        )

    def test_does_unmount_if_stale_mount_is_unconnected(self):
        self.mount_table.add_stale_mount("/mnt/stale1")

        fixer, out = self.run_check(dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Found 1 stale edenfs mount:
  /mnt/stale1
Unmounting 1 stale edenfs mount...<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)
        self.assertEqual([b"/mnt/stale1"], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_does_not_unmount_other_users_mounts(self):
        self.mount_table.add_mount("/mnt/stale1", uid=os.getuid() + 1)

        fixer, out = self.run_check(dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)

    def test_does_not_unmount_mounts_with_same_device_as_active_mount(self):
        active1_dev = self.mount_table.lstat("/mnt/active1").st_dev
        self.mount_table.add_mount("/mnt/stale1", dev=active1_dev)

        fixer, out = self.run_check(dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)


def _create_watchman_subscription(
    filewatcher_subscriptions: Optional[List[str]] = None,
    include_hg_subscriptions: bool = True,
) -> Dict:
    if filewatcher_subscriptions is None:
        filewatcher_subscriptions = []
    subscribers = []
    for filewatcher_subscription in filewatcher_subscriptions:
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
        for name in check_watchman.NUCLIDE_HG_SUBSCRIPTIONS:
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


class ResetParentsCommitsArgs(NamedTuple):
    mount: bytes
    parent1: bytes
    parent2: Optional[bytes]


class FakeClient:
    commit_checker: Optional[Callable[[bytes, str], bool]] = None

    def __init__(self):
        self._mounts = []
        self.set_parents_calls: List[ResetParentsCommitsArgs] = []

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_traceback):
        pass

    def listMounts(self):
        return self._mounts

    def resetParentCommits(
        self, mountPoint: bytes, parents: eden_ttypes.WorkingDirectoryParents
    ):
        self.set_parents_calls.append(
            ResetParentsCommitsArgs(
                mount=mountPoint, parent1=parents.parent1, parent2=parents.parent2
            )
        )

    def getScmStatus(
        self,
        mountPoint: Optional[bytes] = None,
        listIgnored: Optional[bool] = None,
        commit: Optional[bytes] = None,
    ) -> Optional[eden_ttypes.ScmStatus]:
        assert mountPoint is not None
        self._check_commit_valid(mountPoint, commit)
        return None

    def getScmStatusBetweenRevisions(
        self,
        mountPoint: Optional[bytes] = None,
        oldHash: Optional[bytes] = None,
        newHash: Optional[bytes] = None,
    ) -> Optional[eden_ttypes.ScmStatus]:
        assert mountPoint is not None
        self._check_commit_valid(mountPoint, oldHash)
        self._check_commit_valid(mountPoint, newHash)
        return None

    def _check_commit_valid(self, path: bytes, commit: Union[None, bytes, str]):
        if self.commit_checker is None:
            return

        if commit is None:
            return
        if isinstance(commit, str):
            commit_hex = commit
        else:
            commit_hex = binascii.hexlify(commit).decode("utf-8")

        if not self.commit_checker(path, commit_hex):
            raise eden_ttypes.EdenError(
                message=f"RepoLookupError: unknown revision {commit_hex}"
            )


class FakeCheckout(NamedTuple):
    state_dir: Path
    config: CheckoutConfig
    snapshot: str


class FakeEdenInstance:
    def __init__(
        self,
        tmp_dir: str,
        status: fb_status = fb_status.ALIVE,
        build_info: Optional[Dict[str, str]] = None,
        config: Optional[Dict[str, str]] = None,
    ) -> None:
        self._tmp_dir = tmp_dir
        self._status = status
        self._build_info = build_info if build_info else {}
        self._config = config if config else {}
        self._fake_client = FakeClient()

        self._eden_dir = Path(self._tmp_dir) / "eden"
        self._eden_dir.mkdir()
        self.clients_path = self._eden_dir / "clients"
        self.clients_path.mkdir()

        # A map from mount path --> FakeCheckout
        self._checkouts_by_path: Dict[str, FakeCheckout] = {}

        self.mount_table = FakeMountTable()
        self._next_dev_id = 10

    @property
    def state_dir(self) -> Path:
        return self._eden_dir

    def create_test_mount(
        self,
        path: str,
        snapshot: Optional[str] = None,
        bind_mounts: Optional[Dict[str, str]] = None,
        client_name: Optional[str] = None,
        scm_type: str = "hg",
        active: bool = True,
        setup_path: bool = True,
        dirstate_parent: Union[str, Tuple[str, str], None] = None,
        backing_repo: Optional[Path] = None,
    ) -> EdenCheckout:
        """
        Define a configured mount.

        If active is True and status was set to ALIVE when creating the FakeClient
        then the mount will appear as a normal active mount.  It will be reported in the
        thrift results and the mount table, and the mount directory will be populated
        with a .hg/ or .git/ subdirectory.

        The setup_path argument can be set to False to prevent creating the fake mount
        directory on disk.

        Returns the absolute path to the mount directory.
        """
        full_path = os.path.join(self._tmp_dir, path)
        if full_path in self._checkouts_by_path:
            raise Exception(f"duplicate mount definition: {full_path}")

        if snapshot is None:
            snapshot = "1" * 40
        if bind_mounts is None:
            bind_mounts = {}
        if client_name is None:
            client_name = path.replace("/", "_")
        backing_repo_path = (
            backing_repo
            if backing_repo is not None
            else (Path(self._tmp_dir) / "eden-repos" / client_name)
        )

        state_dir = self.clients_path / client_name
        assert full_path not in self._checkouts_by_path
        config = CheckoutConfig(
            backing_repo=backing_repo_path,
            scm_type=scm_type,
            hooks_path="",
            bind_mounts=bind_mounts,
            default_revision=snapshot,
        )
        checkout = FakeCheckout(state_dir=state_dir, config=config, snapshot=snapshot)
        self._checkouts_by_path[full_path] = checkout

        # Write out the config file and snapshot file
        state_dir.mkdir()
        eden_checkout = EdenCheckout(
            typing.cast(EdenInstance, self), Path(full_path), state_dir
        )
        eden_checkout.save_config(config)
        eden_checkout.save_snapshot(snapshot)

        if active and self._status == fb_status.ALIVE:
            # Report the mount in /proc/mounts
            dev_id = self._next_dev_id
            self._next_dev_id += 1
            self.mount_table.stats[full_path] = mtab.MTStat(
                st_uid=os.getuid(), st_dev=dev_id, st_mode=(stat.S_IFDIR | 0o755)
            )

            # Tell the thrift client to report the mount as active
            self._fake_client._mounts.append(
                eden_ttypes.MountInfo(
                    mountPoint=os.fsencode(full_path),
                    edenClientPath=os.fsencode(state_dir),
                    state=eden_ttypes.MountState.RUNNING,
                )
            )

            # Set up directories on disk that look like the mounted checkout
            if setup_path:
                os.makedirs(full_path)
                if scm_type == "hg":
                    self._setup_hg_path(full_path, dirstate_parent, snapshot)
                elif scm_type == "git":
                    os.mkdir(os.path.join(full_path, ".git"))

        return EdenCheckout(
            typing.cast(EdenInstance, self), Path(full_path), Path(state_dir)
        )

    def remove_checkout_configuration(self, mount_path: str) -> None:
        """Update the state to make it look like the specified mount path is still
        actively mounted but not configured on disk."""
        checkout = self._checkouts_by_path.pop(mount_path)
        shutil.rmtree(checkout.state_dir)

    def _setup_hg_path(
        self,
        full_path: str,
        dirstate_parent: Union[str, Tuple[str, str], None],
        snapshot: str,
    ):
        hg_dir = os.path.join(full_path, ".hg")
        os.mkdir(hg_dir)
        dirstate_path = os.path.join(hg_dir, "dirstate")

        if dirstate_parent is None:
            # The dirstate parent should normally match the snapshot hash
            parents = (binascii.unhexlify(snapshot), b"\x00" * 20)
        elif isinstance(dirstate_parent, str):
            # Assume we were given a single parent hash as a hex string
            parents = (binascii.unhexlify(dirstate_parent), b"\x00" * 20)
        else:
            # Assume we were given a both parent hashes as hex strings
            parents = (
                binascii.unhexlify(dirstate_parent[0]),
                binascii.unhexlify(dirstate_parent[1]),
            )

        with open(dirstate_path, "wb") as f:
            eden.dirstate.write(f, parents, tuples_dict={}, copymap={})

    def get_mount_paths(self) -> Iterable[str]:
        return self._checkouts_by_path.keys()

    def mount(self, path: str) -> int:
        assert self._status in (fb_status.ALIVE, fb_status.STARTING, fb_status.STOPPING)
        assert path in self._checkouts_by_path
        return 0

    def check_health(self) -> HealthStatus:
        return HealthStatus(self._status, pid=None, detail="")

    def get_client_info(self, mount_path: str) -> collections.OrderedDict:
        checkout = self._checkouts_by_path[mount_path]
        return collections.OrderedDict(
            [
                ("bind-mounts", checkout.config.bind_mounts),
                ("mount", mount_path),
                ("scm_type", checkout.config.scm_type),
                ("snapshot", checkout.snapshot),
                ("client-dir", checkout.state_dir),
            ]
        )

    def get_server_build_info(self) -> Dict[str, str]:
        return dict(self._build_info)

    def get_thrift_client(self) -> FakeClient:
        return self._fake_client

    def get_checkouts(self) -> List[EdenCheckout]:
        results: List[EdenCheckout] = []
        for mount_path, checkout in self._checkouts_by_path.items():
            results.append(
                EdenCheckout(
                    typing.cast(EdenInstance, self),
                    Path(mount_path),
                    Path(checkout.state_dir),
                )
            )
        return results

    def get_config_value(self, key: str, default: str) -> str:
        return self._config.get(key, default)


class FakeProcessFinder(process_finder.LinuxProcessFinder):
    def __init__(self) -> None:
        self._pgrep_output = b""
        self._file_contents: Dict[Path, Union[bytes, Exception]] = {}

    def add_process(self, pid: int, cmdline: bytes) -> None:
        line = f"{pid} ".encode("utf-8") + cmdline + b"\n"
        self._pgrep_output += line

    def add_edenfs(self, pid: int, eden_dir: str, set_lockfile: bool = True) -> None:
        if set_lockfile:
            self.set_file_contents(Path(eden_dir) / "lock", f"{pid}\n".encode("utf-8"))

        cmdline = (
            f"/usr/bin/edenfs --edenfs --edenDir {eden_dir} "
            f"--etcEdenDir /etc/eden --configPath /home/user/.edenrc"
        ).encode("utf-8")
        self.add_process(pid, cmdline)

    def set_file_contents(self, path: Union[Path, str], contents: bytes) -> None:
        self._file_contents[Path(path)] = contents

    def set_file_exception(self, path: Union[Path, str], exception: Exception) -> None:
        self._file_contents[Path(path)] = exception

    def get_pgrep_output(self) -> bytes:
        return self._pgrep_output

    def read_lock_file(self, path: Path) -> bytes:
        contents = self._file_contents.get(path, None)
        if contents is None:
            raise FileNotFoundError(errno.ENOENT, str(path))
        if isinstance(contents, Exception):
            raise contents
        return contents


class FakeMountTable(mtab.MountTable):
    def __init__(self) -> None:
        self.mounts: List[mtab.MountInfo] = []
        self.unmount_lazy_calls: List[bytes] = []
        self.unmount_force_calls: List[bytes] = []
        self.unmount_lazy_fails: Set[bytes] = set()
        self.unmount_force_fails: Set[bytes] = set()
        self.stats: Dict[str, Union[mtab.MTStat, Exception]] = {}
        self._next_dev: int = 10
        self.bind_mount_success_paths: Dict[str, str] = {}

    def add_mount(
        self,
        path: str,
        uid: Optional[int] = None,
        dev: Optional[int] = None,
        mode: Optional[int] = None,
        device: str = "edenfs",
        vfstype: str = "fuse",
    ) -> None:
        if uid is None:
            uid = os.getuid()
        if dev is None:
            dev = self._next_dev
        self._next_dev += 1
        if mode is None:
            mode = 16877

        self._add_mount_info(path, device=device, vfstype=vfstype)
        self.stats[path] = mtab.MTStat(st_uid=uid, st_dev=dev, st_mode=mode)
        if device == "edenfs":
            self.stats[os.path.join(path, ".eden")] = mtab.MTStat(
                st_uid=uid, st_dev=dev, st_mode=mode
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

    def _add_mount_info(self, path: str, device: str, vfstype: str) -> None:
        self.mounts.append(
            mtab.MountInfo(
                device=device.encode("utf-8"),
                mount_point=os.fsencode(path),
                vfstype=vfstype.encode("utf-8"),
            )
        )

    def fail_unmount_lazy(self, *mounts: bytes) -> None:
        self.unmount_lazy_fails |= set(mounts)

    def fail_unmount_force(self, *mounts: bytes) -> None:
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
            return typing.cast(mtab.MTStat, result)

    def _remove_mount(self, mount_point: bytes) -> None:
        self.mounts[:] = [
            mount_info
            for mount_info in self.mounts
            if mount_info.mount_point != mount_point
        ]

    def create_bind_mount(self, source_path, dest_path) -> bool:
        if (
            source_path in self.bind_mount_success_paths
            and dest_path == self.bind_mount_success_paths[source_path]
        ):
            return True

        cmd = " ".join(["sudo", "mount", "-o", "bind", source_path, dest_path])
        output = "Command returned non-zero error code"
        raise subprocess.CalledProcessError(returncode=1, cmd=cmd, output=output)


class FakeFsUtil(filesystem.FsUtil):
    def __init__(self) -> None:
        self.path_error: Dict[str, str] = {}

    def mkdir_p(self, path: str) -> str:
        if path not in self.path_error:
            return path
        error = self.path_error[path] or "[Errno 2] no such file or directory (faked)"
        raise OSError(error)


class OperatingSystemsCheckTest(DoctorTestBase):
    def setUp(self) -> None:
        test_config = {
            "doctor.minimum-kernel-version": "4.11.3-67",
            "doctor.known-bad-kernel-versions": "^4.*_fbk13,TODO,TEST",
        }
        tmp_dir = self.make_temporary_directory()
        self.instance = FakeEdenInstance(tmp_dir, config=test_config)

    def test_kernel_version_split(self) -> None:
        test_versions = (
            ("1", (1, 0, 0, 0)),
            ("1.2", (1, 2, 0, 0)),
            ("1.2.3", (1, 2, 3, 0)),
            ("1.2.3.4", (1, 2, 3, 4)),
            ("1.2.3-4", (1, 2, 3, 4)),
            ("1.2.3.4-abc", (1, 2, 3, 4)),
            ("1.2.3-4.abc", (1, 2, 3, 4)),
            ("1.2.3.4-abc.def", (1, 2, 3, 4)),
            ("1.2.3-4.abc-def", (1, 2, 3, 4)),
        )
        for test_version, expected in test_versions:
            with self.subTest(test_version=test_version):
                result = check_os._parse_os_kernel_version(test_version)
                self.assertEquals(result, expected)

    def test_kernel_version_min(self) -> None:
        # Each of these are ((test_value, expected_result), ...)
        min_kernel_versions_tests = (
            ("4.6.7-73_fbk21_3608_gb5941a6", True),
            ("4.6", True),
            ("4.11", True),
            ("4.11.3", True),
            ("4.11.3.66", True),
            ("4.11.3-77_fbk20_4162_g6e876878d18e", False),
            ("4.11.3-77", False),
        )
        for fake_release, expected in min_kernel_versions_tests:
            with self.subTest(fake_release=fake_release):
                result = check_os._os_is_kernel_version_too_old(
                    typing.cast(EdenInstance, self.instance), fake_release
                )
                self.assertIs(result, expected)

    def test_bad_kernel_versions(self) -> None:
        bad_kernel_versions_tests = ("4.11.3-52_fbk13", "999.2.3-4_TEST", "777.1_TODO")
        for bad_release in bad_kernel_versions_tests:
            with self.subTest(bad_release=bad_release):
                result = check_os._os_is_bad_release(
                    typing.cast(EdenInstance, self.instance), bad_release
                )
                self.assertTrue(result)

    def test_custom_kernel_names(self) -> None:
        custom_name = "4.16.18-custom_byme_3744_g7833bc918498"
        instance = typing.cast(EdenInstance, self.instance)
        self.assertFalse(check_os._os_is_kernel_version_too_old(instance, custom_name))
        self.assertFalse(check_os._os_is_bad_release(instance, custom_name))


class CorruptHgTest(DoctorTestBase):
    maxDiff = None

    def setUp(self) -> None:
        self.instance = FakeEdenInstance(self.make_temporary_directory())
        self.checkout = self.instance.create_test_mount("test_mount", scm_type="hg")

    def test_unreadable_hg_shared_path_is_a_problem(self) -> None:
        sharedpath_path = self.checkout.path / ".hg" / "sharedpath"
        sharedpath_path.symlink_to(sharedpath_path.name)

        out = TestOutput()
        self.cure_what_ails_you(out=out)
        self.assertIn(
            "Failed to read .hg/sharedpath: [Errno 40] Too many levels of symbolic links",
            out.getvalue(),
        )

    def test_truncated_hg_dirstate_is_a_problem(self) -> None:
        dirstate_path = self.checkout.path / ".hg" / "dirstate"
        os.truncate(dirstate_path, dirstate_path.stat().st_size - 1)

        out = TestOutput()
        self.cure_what_ails_you(out=out)
        self.assertEqual(
            f"""\
Checking {self.checkout.path}
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {self.checkout.path}/.hg:
  error parsing .hg/dirstate: Reached EOF while reading checksum \
hash in {self.checkout.path}/.hg/dirstate.

Would repair hg directory contents for {self.checkout.path}

<yellow>Discovered 1 problem during --dry-run<reset>
""",
            out.getvalue(),
        )

    def cure_what_ails_you(self, out: TestOutput) -> int:
        dry_run = True
        return doctor.cure_what_ails_you(
            typing.cast(EdenInstance, self.instance),
            dry_run,
            self.instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )


class DiskUsageTest(DoctorTestBase):
    def _mock_disk_usage(self, blocks, avail, frsize=1024) -> None:
        """Mock test for disk usage."""
        mock_statvfs_patcher = patch("eden.cli.doctor.os.statvfs")
        mock_statvfs = mock_statvfs_patcher.start()
        self.addCleanup(lambda: mock_statvfs.stop())
        statvfs_tuple = collections.namedtuple("statvfs", "f_blocks f_bavail f_frsize")
        mock_statvfs.return_value = statvfs_tuple(blocks, avail, frsize)

        mock_getmountpt_and_deviceid_patcher = patch(
            "eden.cli.doctor.check_filesystems.get_mountpt"
        )
        mock_getmountpt_and_deviceid = mock_getmountpt_and_deviceid_patcher.start()
        self.addCleanup(lambda: mock_getmountpt_and_deviceid.stop())
        mock_getmountpt_and_deviceid.return_value = "/"

    @patch("eden.cli.doctor.ProblemFixer")
    def _check_disk_usage(self, mock_problem_fixer) -> List[doctor.Problem]:
        instance = FakeEdenInstance(self.make_temporary_directory())

        doctor.check_filesystems.check_disk_usage(
            tracker=mock_problem_fixer,
            mount_paths=["/"],
            instance=typing.cast(EdenInstance, instance),
        )
        if mock_problem_fixer.add_problem.call_args:
            problem = mock_problem_fixer.add_problem.call_args[0][0]
            return [problem]
        return []

    def test_low_free_absolute_disk_is_major(self):
        self._mock_disk_usage(blocks=100_000_000, avail=500_000)
        problems = self._check_disk_usage()

        self.assertEqual(
            problems[0].description(),
            "/ has only 512000000 bytes available. "
            "Eden lazily loads your files and needs enough disk "
            "space to store these files when loaded.",
        )
        self.assertEqual(problems[0].severity(), doctor.ProblemSeverity.ERROR)

    def test_low_percentage_free_but_high_absolute_free_disk_is_minor(self):
        self._mock_disk_usage(blocks=100_000_000, avail=2_000_000)
        problems = self._check_disk_usage()

        self.assertEqual(
            problems[0].description(),
            "/ is 98.00% full. "
            "Eden lazily loads your files and needs enough disk "
            "space to store these files when loaded.",
        )
        self.assertEqual(problems[0].severity(), doctor.ProblemSeverity.ADVICE)

    def test_high_percentage_free_but_small_disk_is_major(self):
        self._mock_disk_usage(blocks=800_000, avail=500_000)
        problems = self._check_disk_usage()

        self.assertEqual(
            problems[0].description(),
            "/ has only 512000000 bytes available. "
            "Eden lazily loads your files and needs enough disk "
            "space to store these files when loaded.",
        )
        self.assertEqual(problems[0].severity(), doctor.ProblemSeverity.ERROR)

    def test_disk_usage_normal(self):
        self._mock_disk_usage(blocks=100_000_000, avail=50_000_000)
        problems = self._check_disk_usage()
        self.assertEqual(len(problems), 0)


class NfsTest(DoctorTestBase):
    maxDiff: Optional[int] = None

    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_mounted(self, mock_is_nfs_mounted):
        mock_is_nfs_mounted.return_value = True
        instance = FakeEdenInstance(self.make_temporary_directory())
        checkout = instance.create_test_mount("mount_dir")

        dry_run = True
        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )
        expected = f"""\
<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.\
  Make sure that ~/local is a symlink pointing to local disk and then restart Eden.

Checking {checkout.path}
<yellow>Discovered 1 problem during --dry-run<reset>
"""
        self.assertEqual(expected, out.getvalue())
        self.assertEqual(1, exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_no_nfs(self, mock_is_nfs_mounted, mock_path_read_text):
        mock_is_nfs_mounted.side_effect = [False, False]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""\
Checking {v.client_path}
<green>No issues detected.<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(0, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_client_path(self, mock_is_nfs_mounted, mock_path_read_text):
        mock_is_nfs_mounted.side_effect = [True, False]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""\
<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {v.instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.\
  Make sure that ~/local is a symlink pointing to local disk and then restart Eden.

Checking {v.client_path}
<yellow>Discovered 1 problem during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_shared_path(self, mock_is_nfs_mounted, mock_path_read_text):
        mock_is_nfs_mounted.side_effect = [False, True]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""\
Checking {v.client_path}
<yellow>- Found problem:<reset>
The Mercurial data directory for {v.client_path}/.hg/sharedpath \
is at {v.shared_path} which is on a NFS filesystem. \
Accessing files and directories in this repository will be slow.
<yellow>Discovered 1 problem during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_client_path_and_shared_path(
        self, mock_is_nfs_mounted, mock_path_read_text
    ):
        mock_is_nfs_mounted.side_effect = [True, True]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""\
<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {v.instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.\
  Make sure that ~/local is a symlink pointing to local disk and then restart Eden.

Checking {v.client_path}
<yellow>- Found problem:<reset>
The Mercurial data directory for {v.client_path}/.hg/sharedpath is at\
 {v.shared_path} which is on a NFS filesystem. Accessing files and directories\
 in this repository will be slow.
<yellow>Discovered 2 problems during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    def run_varying_nfs(self, mock_path_read_text):
        instance = FakeEdenInstance(self.make_temporary_directory())
        v = SimpleNamespace(
            mount_dir="mount_dir", shared_path="shared_path", instance=instance
        )
        mock_path_read_text.return_value = v.shared_path
        v.client_path = str(instance.create_test_mount(v.mount_dir).path)

        dry_run = True
        out = TestOutput()
        v.exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=FakeProcessFinder(),
            out=out,
        )
        v.stdout = out.getvalue()
        return v
