#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess
from typing import Dict

from .lib import testcase


@testcase.eden_repo_test
class PatchTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden.strace": "DBG7", "eden.fs.fuse": "DBG7"}

    def test_patch(self) -> None:
        proc = subprocess.Popen(["patch"], cwd=self.mount, stdin=subprocess.PIPE)
        stdout, stderr = proc.communicate(
            b"""
--- hello
+++ hello
@@ -1 +1 @@
-hola
+bye
"""
        )

        print(stdout, stderr)

        with open(os.path.join(self.mount, "hello"), "r") as f:
            self.assertEqual("bye\n", f.read())
