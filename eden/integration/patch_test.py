#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

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
