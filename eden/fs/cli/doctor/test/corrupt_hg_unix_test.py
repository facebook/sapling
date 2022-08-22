#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import errno
import typing

import eden.fs.cli.doctor as doctor
from eden.fs.cli.config import EdenInstance
from eden.fs.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.fs.cli.doctor.test.lib.fake_fs_util import FakeFsUtil
from eden.fs.cli.doctor.test.lib.fake_kerberos_checker import FakeKerberosChecker
from eden.fs.cli.doctor.test.lib.testcase import DoctorTestBase
from eden.fs.cli.test.lib.output import TestOutput


class CorruptHgUnixTest(DoctorTestBase):
    """Corrupt hg tests relevant to only Unix-like platforms"""

    def setUp(self) -> None:
        self.instance = FakeEdenInstance(self.make_temporary_directory())
        self.checkout = self.instance.create_test_mount("test_mount", scm_type="hg")

    def test_unreadable_hg_shared_path_is_a_problem(self) -> None:
        sharedpath_path = self.checkout.path / ".hg" / "sharedpath"
        sharedpath_path.unlink()
        sharedpath_path.symlink_to(sharedpath_path.name)

        out = self.cure_what_ails_you(dry_run=True)
        self.assertIn(
            "Failed to read .hg/sharedpath: "
            f"[Errno {errno.ELOOP}] Too many levels of symbolic links",
            out.getvalue(),
        )

    def cure_what_ails_you(self, dry_run: bool) -> TestOutput:
        out = TestOutput()
        doctor.cure_what_ails_you(
            typing.cast(EdenInstance, self.instance),
            dry_run,
            mount_table=self.instance.mount_table,
            fs_util=FakeFsUtil(),
            proc_utils=self.make_proc_utils(),
            kerberos_checker=FakeKerberosChecker(),
            out=out,
        )
        return out
