# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


from typing import List, Tuple

from eden.fs.cli import doctor

from eden.fs.cli.doctor import check_stale_mounts
from eden.fs.cli.doctor.test.lib.fake_hang_mount_table import FakeHangMountTable
from eden.fs.cli.doctor.test.lib.testcase import DoctorTestBase


class HangMountTest(DoctorTestBase):
    def setUp(self) -> None:
        self.active_mounts: List[bytes] = [b"/mnt/active1", b"/mnt/active2"]
        self.mount_table = FakeHangMountTable()
        self.mount_table.add_mount("/mnt/active1")
        self.mount_table.add_mount("/mnt/active2")

    def run_check(self, dry_run: bool) -> Tuple[doctor.ProblemFixer, str]:
        fixer, out = self.create_fixer(dry_run)
        check_stale_mounts.check_for_stale_mounts(fixer, mount_table=self.mount_table)
        return fixer, out.getvalue()

    def test_doctor_not_hang_when_mount_checking_hangs(self) -> None:
        fixer, _ = self.run_check(dry_run=False)
        self.assertEqual(fixer.num_problems, 1)
