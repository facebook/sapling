#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess
import unittest

from .lib import edenclient
from .lib.find_executables import FindExe


class UserInfoTest(unittest.TestCase):
    @unittest.skipIf(not edenclient.can_run_sudo(), "unable to run sudo")
    def test_drop_privs(self):
        expected_user = os.environ["USER"]

        cmd = ["/usr/bin/sudo", FindExe.DROP_PRIVS, "/bin/env"]
        out = subprocess.check_output(cmd)
        lines = out.splitlines()
        self.assertIn(f"USER={expected_user}".encode("utf-8"), lines)
        self.assertIn(f"USERNAME={expected_user}".encode("utf-8"), lines)
        self.assertIn(f"LOGNAME={expected_user}".encode("utf-8"), lines)
