#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import shutil
import subprocess
import tempfile
import unittest

from .lib.find_executables import FindExe


SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR = 2
SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL = 3


class StopTest(unittest.TestCase):
    def setUp(self):
        def cleanup_tmp_dir() -> None:
            shutil.rmtree(self.tmp_dir, ignore_errors=True)

        self.tmp_dir = tempfile.mkdtemp(prefix="eden_test.")
        self.addCleanup(cleanup_tmp_dir)

    def test_stop_sigkill(self):
        # Start eden, using the FAKE_EDENFS binary instead of the real edenfs.
        # This binary behaves enough like edenfs to pass health checks, but it refuses
        # to ever shut down gracefully.
        start_cmd = [
            FindExe.EDEN_CLI,
            "--config-dir",
            self.tmp_dir,
            "start",
            "--daemon-binary",
            FindExe.FAKE_EDENFS,
            "--",
            "--ignoreStop",
        ]
        print("Starting eden: %r" % (start_cmd,))
        subprocess.check_call(start_cmd)

        # Ask the CLI to stop edenfs, with a 1 second timeout.
        # It should have to kill the process with SIGKILL
        stop_cmd = [
            FindExe.EDEN_CLI,
            "--config-dir",
            self.tmp_dir,
            "stop",
            "--timeout",
            "1",
        ]
        print("Stopping eden: %r" % (stop_cmd,))
        stop_result = subprocess.run(
            stop_cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
        self.assertIn(b"Terminated edenfs with SIGKILL", stop_result.stderr)
        self.assertEqual(
            SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL, stop_result.returncode
        )

    def test_stop_not_running(self):
        stop_cmd = [
            FindExe.EDEN_CLI,
            "--config-dir",
            self.tmp_dir,
            "stop",
            "--timeout",
            "1",
        ]
        stop_result = subprocess.run(
            stop_cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
        self.assertIn(b"edenfs is not running", stop_result.stderr)
        self.assertEqual(SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR, stop_result.returncode)
