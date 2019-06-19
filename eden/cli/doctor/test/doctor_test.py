#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import typing
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple
from unittest.mock import call, patch

import eden.cli.doctor as doctor
from eden.cli import filesystem
from eden.cli.config import EdenCheckout, EdenInstance
from eden.cli.doctor import check_hg, check_watchman
from eden.cli.doctor.test.lib.fake_client import FakeClient, ResetParentsCommitsArgs
from eden.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.cli.doctor.test.lib.fake_mount_table import FakeMountTable
from eden.cli.doctor.test.lib.testcase import DoctorTestBase
from eden.cli.test.lib.output import TestOutput
from fb303.ttypes import fb_status


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
            process_finder=self.make_process_finder(),
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
Missing hg directory: {edenfs_path3}/.hg
Repairing hg directory contents for {edenfs_path3}...<green>fixed<reset>

<yellow>Successfully fixed 3 problems.<reset>
<yellow>1 issue requires manual attention.<reset>
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
            process_finder=self.make_process_finder(),
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
            process_finder=self.make_process_finder(),
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
            process_finder=self.make_process_finder(),
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
            process_finder=self.make_process_finder(),
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
            process_finder=self.make_process_finder(),
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
            process_finder=self.make_process_finder(),
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

    def test_remount_checkouts_old_edenfs(self) -> None:
        exit_code, out, mounts = self._test_remount_checkouts(  # type: ignore
            dry_run=False, old_edenfs=True
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
            dry_run=True, old_edenfs=True
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
        self,
        mock_get_roots_for_nuclide,
        mock_watchman,
        dry_run: bool,
        old_edenfs: bool = False,
    ) -> Tuple[int, str, List[Path]]:
        """Test that `eden doctor` remounts configured mount points that are not
        currently mounted.
        """
        tmp_dir = self.make_temporary_directory()
        instance = FakeEdenInstance(tmp_dir)

        mounts = []
        mount1 = instance.create_test_mount("path1")
        mounts.append(mount1.path)
        mounts.append(instance.create_test_mount("path2", active=False).path)
        if old_edenfs:
            # Mimic older versions of edenfs, and do not return mount state data.
            instance.get_thrift_client().change_mount_state(mount1.path, None)

        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            typing.cast(EdenInstance, instance),
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=self.make_process_finder(),
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
            process_finder=self.make_process_finder(),
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

    def test_pwd_out_of_date(self) -> None:
        tmp_dir = self.make_temporary_directory()
        instance = FakeEdenInstance(tmp_dir)
        mount = instance.create_test_mount("path1").path

        exit_code, out = self._test_with_pwd(instance, pwd=tmp_dir)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Your current working directory is out-of-date.
This can happen if you have (re)started Eden but your shell is still pointing to
the old directory from before the Eden checkouts were mounted.

Run "cd / && cd -" to update your shell's working directory.

Checking {mount}
<yellow>1 issue requires manual attention.<reset>
Ask in the Eden Users group if you need help fixing issues with Eden:
https://fb.facebook.com/groups/eden.users/
""",
        )
        self.assertEqual(1, exit_code)

    def test_pwd_not_set(self) -> None:
        tmp_dir = self.make_temporary_directory()
        instance = FakeEdenInstance(tmp_dir)
        mount = instance.create_test_mount("path1").path

        exit_code, out = self._test_with_pwd(instance, pwd=None)
        self.assertEqual(
            out,
            f"""\
Checking {mount}
<green>No issues detected.<reset>
""",
        )
        self.assertEqual(0, exit_code)

    def _test_with_pwd(
        self, instance: "FakeEdenInstance", pwd: Optional[str]
    ) -> Tuple[int, str]:
        if pwd is None:
            old_pwd = os.environ.pop("PWD", None)
        else:
            old_pwd = os.environ.get("PWD")
            os.environ["PWD"] = pwd
        try:
            out = TestOutput()
            exit_code = doctor.cure_what_ails_you(
                typing.cast(EdenInstance, instance),
                dry_run=False,
                mount_table=instance.mount_table,
                fs_util=filesystem.LinuxFsUtil(),
                process_finder=self.make_process_finder(),
                out=out,
            )
            return exit_code, out.getvalue()
        finally:
            if old_pwd is not None:
                os.environ["PWD"] = old_pwd


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
