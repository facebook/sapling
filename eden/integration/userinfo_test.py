#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess
import unittest

from .lib import edenclient, testcase
from .lib.find_executables import FindExe


@testcase.eden_test
class UserInfoTest(testcase.IntegrationTestCase):
    @unittest.skipIf(not edenclient.can_run_sudo(), "unable to run sudo")
    def test_drop_privs(self) -> None:
        try:
            expected_user = os.environ["USER"]
        except KeyError:
            self.skipTest("No USER environment variable available")
            return
        if expected_user == "root":
            self.skipTest("Is root user")
            return

        cmd = ["/usr/bin/sudo", FindExe.DROP_PRIVS, "/usr/bin/env"]
        out = subprocess.check_output(cmd)
        lines = out.splitlines()
        self.assertIn(f"USER={expected_user}".encode("utf-8"), lines)
        self.assertIn(f"USERNAME={expected_user}".encode("utf-8"), lines)
        self.assertIn(f"LOGNAME={expected_user}".encode("utf-8"), lines)
