#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import os
from typing import List, Tuple

import eden.cli.doctor as doctor
from eden.cli.doctor import check_stale_mounts
from eden.cli.doctor.test.lib.fake_mount_table import FakeMountTable
from eden.cli.doctor.test.lib.testcase import DoctorTestBase


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
