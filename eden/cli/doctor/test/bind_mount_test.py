#!/usr/bin/env python3
#
# Copyright (c) 2019-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import typing
from pathlib import Path
from typing import Dict, Optional, Tuple

import eden.cli.doctor as doctor
from eden.cli import filesystem, mtab
from eden.cli.config import EdenCheckout, EdenInstance
from eden.cli.doctor import check_bind_mounts
from eden.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.cli.doctor.test.lib.fake_mount_table import FakeMountTable
from eden.cli.doctor.test.lib.testcase import DoctorTestBase


BACKING_DIR_STAT = mtab.MTStat(st_uid=os.getuid(), st_dev=11, st_mode=16877)
BACKING_FILE_STAT = mtab.MTStat(st_uid=os.getuid(), st_dev=11, st_mode=33188)
FUSE_DIR_STAT = mtab.MTStat(st_uid=os.getuid(), st_dev=12, st_mode=16877)
FUSE_FILE_STAT = mtab.MTStat(st_uid=os.getuid(), st_dev=12, st_mode=33188)


class FakeFsUtil(filesystem.FsUtil):
    def __init__(self) -> None:
        self.path_error: Dict[str, str] = {}

    def mkdir_p(self, path: str) -> str:
        if path not in self.path_error:
            return path
        error = self.path_error[path] or "[Errno 2] no such file or directory (faked)"
        raise OSError(error)


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
        fs_util: Optional[FakeFsUtil] = None,
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

    def _make_ideal_mount_table(self) -> FakeMountTable:
        mount_table = FakeMountTable()
        mount_table.stats[self.fbsource_bind_mounts] = BACKING_DIR_STAT
        mount_table.stats[self.edenfs_path1] = FUSE_DIR_STAT

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = BACKING_DIR_STAT
        mount_table.stats[self.client_bm2] = BACKING_DIR_STAT
        mount_table.stats[self.client_bm3] = BACKING_DIR_STAT

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = BACKING_DIR_STAT
        mount_table.stats[self.bm2] = BACKING_DIR_STAT
        mount_table.stats[self.bm3] = BACKING_DIR_STAT

        return mount_table

    def test_bind_mounts_okay(self):
        mount_table = self._make_ideal_mount_table()

        fixer, out = self.run_check(mount_table, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_bind_mounts_missing_dry_run(self):
        mount_table = self._make_ideal_mount_table()

        # Bind mount paths (under eden path); these should be
        # visible as BACKING_DIR_STAT but by reporting as
        # FUSE_DIR_STAT we believe that they are not mounted.
        mount_table.stats[self.bm1] = FUSE_DIR_STAT
        mount_table.stats[self.bm2] = FUSE_DIR_STAT
        mount_table.stats[self.bm3] = FUSE_DIR_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Bind mount paths (under eden path); these should be
        # visible as BACKING_DIR_STAT but by reporting as
        # FUSE_DIR_STAT we believe that they are not mounted.
        mount_table.stats[self.bm1] = FUSE_DIR_STAT
        mount_table.stats[self.bm2] = FUSE_DIR_STAT
        mount_table.stats[self.bm3] = FUSE_DIR_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Bind mount paths (under eden path); these should be
        # visible as BACKING_DIR_STAT but by reporting as
        # FUSE_DIR_STAT we believe that they are not mounted.
        mount_table.stats[self.bm1] = FUSE_DIR_STAT
        mount_table.stats[self.bm2] = FUSE_DIR_STAT
        mount_table.stats[self.bm3] = FUSE_DIR_STAT

        # These bind mount operations will succeed.
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

        mount_table.stats[self.fbsource_bind_mounts] = BACKING_DIR_STAT
        mount_table.stats[self.edenfs_path1] = FUSE_DIR_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Bind mount paths (under eden path)
        # bm1, bm2 should not have same device as edenfs
        mount_table.stats[self.bm1] = FUSE_DIR_STAT
        mount_table.stats[self.bm2] = FUSE_DIR_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Bind mount paths (under eden path)
        # bm1, bm2 should not have same device as edenfs
        mount_table.stats[self.bm1] = FUSE_DIR_STAT
        mount_table.stats[self.bm2] = FUSE_DIR_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Client bind mount paths (under .eden)
        # Note: client_bm3 is not a directory
        mount_table.stats[self.client_bm3] = BACKING_FILE_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Bind mount paths (under eden path)
        # Note: bm3 is not a directory
        mount_table.stats[self.bm3] = FUSE_FILE_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Client bind mount paths (under .eden)
        del mount_table.stats[self.client_bm1]
        del mount_table.stats[self.client_bm3]

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
        mount_table = self._make_ideal_mount_table()

        # Client bind mount paths (under .eden)
        del mount_table.stats[self.client_bm1]
        del mount_table.stats[self.client_bm3]

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
        mount_table = self._make_ideal_mount_table()

        # Client bind mount paths (under .eden)
        del mount_table.stats[self.client_bm1]
        del mount_table.stats[self.client_bm3]

        fs_util = FakeFsUtil()
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
        mount_table.stats[self.fbsource_bind_mounts] = BACKING_DIR_STAT
        mount_table.stats[self.edenfs_path1] = FUSE_DIR_STAT

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = BACKING_DIR_STAT

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = BACKING_DIR_STAT

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
        mount_table.stats[self.fbsource_bind_mounts] = BACKING_DIR_STAT
        mount_table.stats[self.edenfs_path1] = FUSE_DIR_STAT

        # Client bind mount paths (under .eden)
        mount_table.stats[self.client_bm1] = BACKING_DIR_STAT

        # Bind mount paths (under eden path)
        mount_table.stats[self.bm1] = BACKING_DIR_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Client bind mount paths (under .eden)
        del mount_table.stats[self.client_bm1]
        mount_table.stats[self.client_bm2] = FUSE_DIR_STAT
        mount_table.stats[self.client_bm3] = BACKING_FILE_STAT

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
        mount_table = self._make_ideal_mount_table()

        # Client bind mount paths (under .eden)
        del mount_table.stats[self.client_bm1]
        mount_table.stats[self.client_bm2] = FUSE_DIR_STAT
        mount_table.stats[self.client_bm3] = BACKING_FILE_STAT

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
