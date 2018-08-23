#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import re
import subprocess

from eden.integration.lib.find_executables import FindExe

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test("TreeOnly")
class EmptyFileTest(EdenHgTestCase):
    def populate_backing_repo(self, repo):
        self.main_contents = "echo hello world\n"
        repo.write_file("README", "docs")
        repo.write_file("src/main.sh", self.main_contents)
        repo.write_file("test/test.txt", "test\n")
        repo.commit("Initial commit.")

    def select_storage_engine(self):
        return "rocksdb"

    def test(self):
        filename = os.path.join(self.mount, "src/main.sh")
        with open(filename, "r") as f:
            self.assertEqual(f.read(), self.main_contents)

        # Get the blob ID of this file
        out = self.eden.run_cmd("debug", "inode", os.path.join(self.mount, "src"))
        r = re.compile(" ([a-fA-F0-9]{40}) main.sh$")
        for line in out.splitlines():
            m = r.search(line)
            if m:
                blob_id = m.group(1)
                break
        else:
            raise Exception(f"unable to find blob ID for src/main.sh:\n{out}")

        # Stop eden and then replace the blob in the store with empty contents
        self.eden.shutdown()
        cmd = [FindExe.ZERO_BLOB, "--edenDir", self.eden._eden_dir, "--blobID", blob_id]
        subprocess.check_call(cmd)

        # Confirm that the contents are empty if we start eden with
        # --reverify-empty-files=no
        self.eden._extra_args = ["--reverify-empty-files=no"]
        self.eden.start()
        filename = os.path.join(self.mount, "src/main.sh")
        with open(filename, "r") as f:
            self.assertEqual(f.read(), "")

        # Confirm that the file contents are correct if we restart eden
        # without explicitly disabling the re-verification logic.
        self.eden._extra_args = []
        self.eden.restart()
        filename = os.path.join(self.mount, "src/main.sh")
        with open(filename, "r") as f:
            self.assertEqual(f.read(), self.main_contents)
