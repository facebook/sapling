#!/usr/bin/env python3
#
# Copyright (c) 2017-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import errno
import io
import os
import shutil
import stat
import subprocess
import tempfile
import typing
import unittest
from collections import OrderedDict
from textwrap import dedent
from typing import Any, Dict, Iterable, List, NamedTuple, Optional, Set, Tuple, Union
from unittest.mock import call, patch

import eden.cli.doctor as doctor
import eden.cli.ui
import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
from eden.cli import filesystem, mtab
from eden.cli.config import EdenInstance, HealthStatus
from fb303.ttypes import fb_status


class TestOutput(eden.cli.ui.TerminalOutput):
    def __init__(self) -> None:
        Color = eden.cli.ui.Color
        Attribute = eden.cli.ui.Attribute
        term_settings = eden.cli.ui.TerminalSettings(
            foreground={
                Color.RED: b"<red>",
                Color.GREEN: b"<green>",
                Color.YELLOW: b"<yellow>",
            },
            background={
                Color.RED: b"<red_bg>",
                Color.GREEN: b"<green_bg>",
                Color.YELLOW: b"<yellow_bg>",
            },
            attributes={Attribute.BOLD: b"<bold>", Attribute.UNDERLINE: b"<underline>"},
            reset=b"<reset>",
        )
        self._out = io.BytesIO()
        super().__init__(self._out, term_settings)

    def getvalue(self) -> str:
        return self._out.getvalue().decode("utf-8", errors="surrogateescape")


class DoctorTestBase(unittest.TestCase):
    def _create_tmp_dir(self) -> str:
        tmp_dir = tempfile.mkdtemp(prefix="eden_test.")
        self.addCleanup(shutil.rmtree, tmp_dir)
        return tmp_dir

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

    @patch("eden.cli.doctor._call_watchman")
    @patch("eden.cli.doctor._get_roots_for_nuclide")
    def test_end_to_end_test_with_various_scenarios(
        self, mock_get_roots_for_nuclide, mock_watchman
    ):
        side_effects: List[Dict[str, Any]] = []
        calls = []
        instance = FakeEdenInstance(self._create_tmp_dir())

        # In edenfs_path1, we will break the snapshot check.
        edenfs_path1_snapshot = "abcd" * 10
        edenfs_path1_dirstate_parent = "12345678" * 5
        edenfs_path1 = instance.create_test_mount(
            "path1",
            snapshot=edenfs_path1_snapshot,
            dirstate_parent=edenfs_path1_dirstate_parent,
        )

        # In edenfs_path2, we will break the inotify check and the Nuclide
        # subscriptions check.
        edenfs_path2 = instance.create_test_mount(
            "path2", scm_type="git", setup_path=False
        )

        # In edenfs_path3, we do not create the .hg directory
        edenfs_path3 = instance.create_test_mount("path3", setup_path=False)
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
            out=out,
        )

        self.assertEqual(
            f"""\
Checking {edenfs_path1}
<yellow>- Found problem:<reset>
mercurial's parent commit for {edenfs_path1} is {edenfs_path1_dirstate_parent},
but Eden's internal hash in its SNAPSHOT file is {edenfs_path1_snapshot}.

Fixing Eden to point to parent commit {edenfs_path1_dirstate_parent}...\
<green>fixed<reset>

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

    @patch("eden.cli.doctor._call_watchman")
    @patch("eden.cli.doctor._get_roots_for_nuclide", return_value=set())
    def test_not_all_mounts_have_watchman_watcher(
        self, mock_get_roots_for_nuclide, mock_watchman
    ):
        instance = FakeEdenInstance(self._create_tmp_dir())
        edenfs_path = instance.create_test_mount("eden-mount", scm_type="git")
        edenfs_path_not_watched = instance.create_test_mount(
            "eden-mount-not-watched", scm_type="git"
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

    @patch("eden.cli.doctor._call_watchman")
    @patch("eden.cli.doctor._get_roots_for_nuclide")
    def test_eden_not_in_use(self, mock_get_roots_for_nuclide, mock_watchman):
        instance = FakeEdenInstance(self._create_tmp_dir(), is_healthy=False)

        out = TestOutput()
        dry_run = False
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            FakeMountTable(),
            fs_util=filesystem.LinuxFsUtil(),
            out=out,
        )

        self.assertEqual("Eden is not in use.\n", out.getvalue())
        self.assertEqual(0, exit_code)

    @patch("eden.cli.doctor._call_watchman")
    @patch("eden.cli.doctor._get_roots_for_nuclide")
    def test_edenfs_not_running(self, mock_get_roots_for_nuclide, mock_watchman):
        instance = FakeEdenInstance(self._create_tmp_dir(), is_healthy=False)
        instance.create_test_mount("eden-mount")

        out = TestOutput()
        dry_run = False
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            FakeMountTable(),
            fs_util=filesystem.LinuxFsUtil(),
            out=out,
        )

        self.assertEqual(
            dedent(
                """\
<yellow>- Found problem:<reset>
Eden is not running.
To start Eden, run:

    eden start

<yellow>1 issue requires manual attention.<reset>
Ask in the Eden Users group if you need help fixing issues with Eden:
https://fb.facebook.com/groups/eden.users/
"""
            ),
            out.getvalue(),
        )
        self.assertEqual(1, exit_code)

    @patch("eden.cli.doctor._call_watchman")
    def test_no_issue_when_watchman_using_eden_watcher(self, mock_watchman):
        fixer, out = self._test_watchman_watcher_check(
            mock_watchman, initial_watcher="eden"
        )
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    @patch("eden.cli.doctor._call_watchman")
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

    @patch("eden.cli.doctor._call_watchman")
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

    @patch("eden.cli.doctor._call_watchman")
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
        doctor.check_watchman_subscriptions(fixer, edenfs_path, watchman_roots)

        mock_watchman.assert_has_calls(calls)
        return fixer, out.getvalue()

    @patch("eden.cli.doctor._call_watchman")
    def test_no_issue_when_expected_nuclide_subscriptions_present(self, mock_watchman):
        fixer, out = self._test_nuclide_check(
            mock_watchman=mock_watchman, include_filewatcher_subscriptions=True
        )
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    @patch("eden.cli.doctor._call_watchman")
    def test_no_issue_when_path_not_in_nuclide_roots(self, mock_watchman):
        fixer, out = self._test_nuclide_check(
            mock_watchman=mock_watchman, include_path_in_nuclide_roots=False
        )
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    @patch("eden.cli.doctor._call_watchman")
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

    @patch("eden.cli.doctor._call_watchman")
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

    @patch("eden.cli.doctor._call_watchman")
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
        doctor.check_nuclide_watchman_subscriptions(
            fixer, edenfs_path, watchman_roots, nuclide_roots
        )

        mock_watchman.assert_has_calls(watchman_calls)
        return fixer, out.getvalue()

    def test_snapshot_and_dirstate_file_match(self):
        dirstate_hash_hex = "12345678" * 5
        snapshot_hex = "12345678" * 5
        _instance, _mount_path, fixer, out = self._test_hash_check(
            dirstate_hash_hex, snapshot_hex
        )
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_snapshot_and_dirstate_file_differ(self):
        dirstate_hash_hex = "12000000" * 5
        snapshot_hex = "12345678" * 5
        instance, mount_path, fixer, out = self._test_hash_check(
            dirstate_hash_hex, snapshot_hex
        )
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
mercurial's parent commit for {mount_path} is 1200000012000000120000001200000012000000,
but Eden's internal hash in its SNAPSHOT file is \
1234567812345678123456781234567812345678.

Fixing Eden to point to parent commit 1200000012000000120000001200000012000000...\
<green>fixed<reset>

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_fixed_problems=1)
        # Make sure resetParentCommits() was called once with the expected arguments
        self.assertEqual(
            instance.get_thrift_client().set_parents_calls,
            [
                ResetParentsCommitsArgs(
                    mount=mount_path.encode("utf-8"),
                    parent1=b"\x12\x00\x00\x00" * 5,
                    parent2=None,
                )
            ],
        )

    def _test_hash_check(
        self, dirstate_hash_hex: str, snapshot_hex: str
    ) -> Tuple["FakeEdenInstance", str, doctor.ProblemFixer, str]:
        instance = FakeEdenInstance(self._create_tmp_dir())
        mount_path = instance.create_test_mount(
            "path1", snapshot=snapshot_hex, dirstate_parent=dirstate_hash_hex
        )

        fixer, out = self.create_fixer(dry_run=False)
        doctor.check_snapshot_dirstate_consistency(
            fixer, typing.cast(EdenInstance, instance), mount_path, snapshot_hex
        )
        return instance, mount_path, fixer, out.getvalue()

    @patch("eden.cli.version.get_installed_eden_rpm_version")
    def test_edenfs_when_installed_and_running_match(self, mock_gierv):
        fixer, out = self._test_edenfs_version(mock_gierv, "20171213-165642")
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    @patch("eden.cli.version.get_installed_eden_rpm_version")
    def test_edenfs_when_installed_and_running_differ(self, mock_gierv):
        fixer, out = self._test_edenfs_version(mock_gierv, "20171120-246561")
        self.assertEqual(
            dedent(
                """\
    <yellow>- Found problem:<reset>
    The version of Eden that is installed on your machine is:
        fb-eden-20171120-246561.x86_64
    but the version of Eden that is currently running is:
        fb-eden-20171213-165642.x86_64

    Consider running `eden restart` to migrate to the newer version, which
    may have important bug fixes or performance improvements.

                """
            ),
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
            self._create_tmp_dir(),
            build_info={
                "build_package_version": "20171213",
                "build_package_release": "165642",
            },
        )
        fixer, out = self.create_fixer(dry_run=False)
        doctor.check_edenfs_version(fixer, typing.cast(EdenInstance, instance))
        mock_rpm_q.assert_has_calls(calls)
        return fixer, out.getvalue()

    @patch("eden.cli.doctor._get_roots_for_nuclide", return_value=set())
    def test_unconfigured_mounts_dont_crash(self, mock_get_roots_for_nuclide):
        # If Eden advertises that a mount is active, but it is not in the
        # configuration, then at least don't throw an exception.
        instance = FakeEdenInstance(self._create_tmp_dir())
        edenfs_path1 = instance.create_test_mount("path1")
        edenfs_path2 = instance.create_test_mount("path2")
        # Remove path2 from the list of mounts in the instance
        del instance._mount_paths[edenfs_path2]

        dry_run = False
        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            out=out,
        )

        self.assertEqual(
            f"""\
Checking {edenfs_path1}
<green>No issues detected.<reset>
""",
            out.getvalue(),
        )
        self.assertEqual(0, exit_code)

    def test_remount_checkouts(self) -> None:
        exit_code, out, mounts = self._test_remount_checkouts(dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
{mounts[1]} is not currently mounted
Remounting {mounts[1]}...<green>fixed<reset>

Checking {mounts[0]}
<yellow>Successfully fixed 1 problem.<reset>
""",
            out,
        )
        self.assertEqual(exit_code, 0)

    def test_remount_checkouts_dry_run(self) -> None:
        exit_code, out, mounts = self._test_remount_checkouts(dry_run=True)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
{mounts[1]} is not currently mounted
Would remount {mounts[1]}

Checking {mounts[0]}
<yellow>Discovered 1 problem during --dry-run<reset>
""",
            out,
        )
        self.assertEqual(exit_code, 1)

    @patch("eden.cli.doctor._call_watchman")
    @patch("eden.cli.doctor._get_roots_for_nuclide", return_value=set())
    def _test_remount_checkouts(
        self, mock_get_roots_for_nuclide, mock_watchman, dry_run: bool
    ) -> Tuple[int, str, List[str]]:
        """Test that `eden doctor` remounts configured mount points that are not
        currently mounted.
        """
        self._tmp_dir = self._create_tmp_dir()
        instance = FakeEdenInstance(self._tmp_dir)

        mounts = []
        mounts.append(instance.create_test_mount("path1"))
        mounts.append(instance.create_test_mount("path2", active=False))

        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            typing.cast(EdenInstance, instance),
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            out=out,
        )
        return exit_code, out.getvalue(), mounts

    @patch("eden.cli.doctor._call_watchman")
    def test_watchman_fails(self, mock_watchman):
        self._tmp_dir = self._create_tmp_dir()
        instance = FakeEdenInstance(self._tmp_dir)

        mount = instance.create_test_mount("path1", active=False)

        # Make calls to watchman fail rather than returning expected output
        side_effects = [{"error": "watchman failed"}]
        mock_watchman.side_effect = side_effects

        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            typing.cast(EdenInstance, instance),
            dry_run=False,
            mount_table=instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            out=out,
        )

        # "watchman watch-list" should have been called by the doctor code
        calls = [call(["watch-list"])]
        mock_watchman.assert_has_calls(calls)

        self.assertEqual(
            out.getvalue(),
            f"""\
<yellow>- Found problem:<reset>
{mount} is not currently mounted
Remounting {mount}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
        )
        self.assertEqual(exit_code, 0)


class BindMountsCheckTest(DoctorTestBase):
    maxDiff = None

    def setUp(self):
        tmp_dir = tempfile.mkdtemp(prefix="eden_test.")
        self.addCleanup(shutil.rmtree, tmp_dir)
        self.instance = FakeEdenInstance(tmp_dir, is_healthy=True)

        self.dot_eden_path = os.path.join(tmp_dir, ".eden")
        self.clients_path = os.path.join(self.dot_eden_path, "clients")

        self.fbsource_client = os.path.join(self.clients_path, "fbsource")
        self.fbsource_bind_mounts = os.path.join(self.fbsource_client, "bind-mounts")
        self.edenfs_path1 = self.instance.create_test_mount(
            "path1",
            bind_mounts={
                "fbcode-buck-out": "fbcode/buck-out",
                "fbandroid-buck-out": "fbandroid/buck-out",
                "buck-out": "buck-out",
            },
            client_dir=self.fbsource_client,
            setup_path=False,
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
        doctor.check_bind_mounts(
            fixer,
            self.edenfs_path1,
            self.instance,
            self.instance.get_client_info(self.edenfs_path1),
            mount_table=mount_table,
            fs_util=fs_util,
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

    def setUp(self):
        self.active_mounts: List[bytes] = [b"/mnt/active1", b"/mnt/active2"]
        self.mount_table = FakeMountTable()
        self.mount_table.add_mount("/mnt/active1")
        self.mount_table.add_mount("/mnt/active2")

    def run_check(self, dry_run: bool) -> Tuple[doctor.ProblemFixer, str]:
        fixer, out = self.create_fixer(dry_run)
        doctor.check_for_stale_mounts(fixer, mount_table=self.mount_table)
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

    @patch("eden.cli.doctor.log.warning")
    def test_does_not_unmount_if_cannot_stat_stale_mount(self, warning):
        self.mount_table.add_mount("/mnt/stale1")
        self.mount_table.fail_access("/mnt/stale1", errno.EACCES)
        self.mount_table.fail_access("/mnt/stale1/.eden", errno.EACCES)

        fixer, out = self.run_check(dry_run=False)
        self.assertEqual("", out)
        self.assertEqual(0, fixer.num_problems)
        self.assertEqual([], self.mount_table.unmount_lazy_calls)
        self.assertEqual([], self.mount_table.unmount_force_calls)
        # Verify that the reason for skipping this mount is logged.
        warning.assert_called_once_with(
            "Unclear whether /mnt/stale1 is stale or not. "
            "lstat() failed: [Errno 13] Permission denied"
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


class ResetParentsCommitsArgs(NamedTuple):
    mount: bytes
    parent1: bytes
    parent2: Optional[bytes]


class FakeClient:
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


class FakeEdenInstance:
    def __init__(
        self,
        tmp_dir: str,
        is_healthy: bool = True,
        build_info: Optional[Dict[str, str]] = None,
    ) -> None:
        self._tmp_dir = tmp_dir
        self._mount_paths: Dict[str, Dict[str, Any]] = {}
        self._is_healthy = is_healthy
        self._build_info = build_info if build_info else {}
        self._fake_client = FakeClient()

        self.mount_table = FakeMountTable()
        self._next_dev_id = 10

    def create_test_mount(
        self,
        path: str,
        snapshot: Optional[str] = None,
        bind_mounts: Optional[Dict[str, str]] = None,
        client_dir: Optional[str] = None,
        scm_type: str = "hg",
        active: bool = True,
        setup_path: bool = True,
        dirstate_parent: Union[str, Tuple[str, str], None] = None,
    ) -> str:
        """
        Define a configured mount.

        If active is True and is_healthy was set to True when creating the FakeClient
        then the mount will appear as a normal active mount.  It will be reported in the
        thrift results and the mount table, and the mount directory will be populated
        with a .hg/ or .git/ subdirectory.

        The setup_path argument can be set to False to prevent creating the fake mount
        directory on disk.

        Returns the absolute path to the mount directory.
        """
        full_path = os.path.join(self._tmp_dir, path)
        if full_path in self._mount_paths:
            raise Exception(f"duplicate mount definition: {full_path}")

        if snapshot is None:
            snapshot = "1" * 40
        if bind_mounts is None:
            bind_mounts = {}
        if client_dir is None:
            client_dir = "/" + path.replace("/", "_")

        self._mount_paths[full_path] = {
            "bind-mounts": bind_mounts,
            "mount": full_path,
            "scm_type": scm_type,
            "snapshot": snapshot,
            "client-dir": client_dir,
        }

        if self._is_healthy and active:
            # Report the mount in /proc/mounts
            dev_id = self._next_dev_id
            self._next_dev_id += 1
            self.mount_table.stats[full_path] = mtab.MTStat(
                st_uid=os.getuid(), st_dev=dev_id, st_mode=(stat.S_IFDIR | 0o755)
            )

            # Tell the thrift client to report the mount as active
            self._fake_client._mounts.append(
                eden_ttypes.MountInfo(mountPoint=os.fsencode(full_path))
            )

            # Set up directories on disk that look like the mounted checkout
            if setup_path:
                os.makedirs(full_path)
                if scm_type == "hg":
                    self._setup_hg_path(full_path, dirstate_parent, snapshot)
                elif scm_type == "git":
                    os.mkdir(os.path.join(full_path, ".git"))

        return full_path

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
        return self._mount_paths.keys()

    def mount(self, path: str) -> int:
        assert self._is_healthy
        assert path in self._mount_paths
        return 0

    def check_health(self) -> HealthStatus:
        status = fb_status.ALIVE if self._is_healthy else fb_status.STOPPED
        return HealthStatus(status, pid=None, detail="")

    def get_client_info(self, mount_path: str) -> Dict[str, str]:
        return self._mount_paths[mount_path]

    def get_server_build_info(self) -> Dict[str, str]:
        return dict(self._build_info)

    def get_thrift_client(self) -> FakeClient:
        return self._fake_client


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
