#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Optional
from unittest.mock import MagicMock, patch

import eden.fs.cli.doctor as doctor
from eden.fs.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.fs.cli.doctor.test.lib.fake_fs_util import FakeFsUtil
from eden.fs.cli.doctor.test.lib.fake_kerberos_checker import FakeKerberosChecker
from eden.fs.cli.doctor.test.lib.fake_vscode_extensions_checker import (
    getFakeVSCodeExtensionsChecker,
)
from eden.fs.cli.doctor.test.lib.testcase import DoctorTestBase
from eden.fs.cli.test.lib.output import TestOutput


class DoctorUnixTest(DoctorTestBase):
    """Doctor tests relevant to only Unix-like platforms"""

    # The diffs for what is written to stdout can be large.
    maxDiff: Optional[int] = None

    @patch(
        "eden.fs.cli.doctor.test.lib.fake_eden_instance.FakeEdenInstance.check_privhelper_connection",
        return_value=False,
    )
    def test_privhelper_check_not_accessible(
        self, mock_check_privhelper_connection: MagicMock
    ) -> None:
        instance = FakeEdenInstance(self.make_temporary_directory())
        mount = instance.create_test_mount("path1").path
        dry_run = False
        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            # pyre-fixme[6]: For 1st param expected `EdenInstance` but got
            #  `FakeEdenInstance`.
            instance,
            dry_run,
            mount_table=instance.mount_table,
            fs_util=FakeFsUtil(),
            proc_utils=self.make_proc_utils(),
            kerberos_checker=FakeKerberosChecker(),
            vscode_extensions_checker=getFakeVSCodeExtensionsChecker(),
            out=out,
        )

        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
The PrivHelper process is not accessible.
To restore the connection to the PrivHelper, run `eden restart`

Checking {mount}
<yellow>1 issue requires manual attention.<reset>
Ask in the EdenFS Users group if you need help fixing issues with EdenFS:
https://fb.facebook.com/groups/eden.users/
""",
            out.getvalue(),
        )
        self.assertEqual(1, exit_code)

    # remove_checkout_configuration silently fails on Windows, leading to a test
    # failure.
    def test_unconfigured_mounts_dont_crash(self) -> None:
        # If EdenFS advertises that a mount is active, but it is not in the
        # configuration, then at least don't throw an exception.
        instance = FakeEdenInstance(self.make_temporary_directory())
        edenfs_path1 = instance.create_test_mount("path1").path
        edenfs_path2 = instance.create_test_mount("path2").path
        # Remove path2 from the list of mounts in the instance
        instance.remove_checkout_configuration(str(edenfs_path2))

        dry_run = False
        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            # pyre-fixme[6]: For 1st param expected `EdenInstance` but got
            #  `FakeEdenInstance`.
            instance,
            dry_run,
            mount_table=instance.mount_table,
            fs_util=FakeFsUtil(),
            proc_utils=self.make_proc_utils(),
            kerberos_checker=FakeKerberosChecker(),
            vscode_extensions_checker=getFakeVSCodeExtensionsChecker(),
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
Ask in the EdenFS Users group if you need help fixing issues with EdenFS:
https://fb.facebook.com/groups/eden.users/
""",
            out.getvalue(),
        )
        self.assertEqual(1, exit_code)
