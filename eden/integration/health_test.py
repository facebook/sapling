#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import pathlib
import shlex
import signal
import sys
import tempfile
import typing
import unittest

import pexpect
from eden.cli.daemon import wait_for_shutdown

from .lib import edenclient, testcase
from .lib.fake_edenfs import fake_eden_daemon
from .lib.find_executables import FindExe


class HealthTest(testcase.EdenTestCase):
    def test_is_healthy(self) -> None:
        self.assertTrue(self.eden.is_healthy())
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())

    def test_disconnected_daemon_is_not_healthy(self) -> None:
        # Create a new edenfs instance that is never started, and make sure
        # it is not healthy.
        with edenclient.EdenFS() as client:
            self.assertFalse(client.is_healthy())


class HealthOfFakeEdenFSTest(unittest.TestCase):
    def setUp(self):
        super().setUp()

        temp_dir = tempfile.TemporaryDirectory(prefix="eden_test")  # noqa: P201
        self.temp_dir = pathlib.Path(temp_dir.__enter__())
        self.addCleanup(lambda: temp_dir.__exit__(None, None, None))

    def test_healthy_daemon_is_healthy(self):
        with fake_eden_daemon(self.temp_dir):
            status_process = pexpect.spawn(
                FindExe.EDEN_CLI,
                ["--config-dir", str(self.temp_dir), "status"],
                encoding="utf-8",
                logfile=sys.stderr,
            )
            status_process.expect_exact("eden running normally")
            self.assert_process_succeeds(status_process)

    def test_killed_daemon_is_not_running(self):
        with fake_eden_daemon(self.temp_dir) as daemon_pid:
            os.kill(daemon_pid, signal.SIGKILL)
            wait_for_shutdown(pid=daemon_pid, timeout=5)

            status_process = pexpect.spawn(
                FindExe.EDEN_CLI,
                ["--config-dir", str(self.temp_dir), "status"],
                encoding="utf-8",
                logfile=sys.stderr,
            )
            status_process.expect_exact("edenfs not running")
            self.assert_process_fails(status_process, exit_code=1)

    def test_hanging_thrift_call_reports_daemon_is_unresponsive(self):
        with fake_eden_daemon(self.temp_dir, ["--sleepBeforeGetPid=5"]):
            status_process = pexpect.spawn(
                FindExe.EDEN_CLI,
                ["--config-dir", str(self.temp_dir), "status", "--timeout", "1"],
                encoding="utf-8",
                logfile=sys.stderr,
            )
            status_process.expect_exact(
                "Eden's Thrift server does not appear to be running, but the "
                "process is still alive"
            )
            self.assert_process_fails(status_process, exit_code=1)

    def test_slow_thrift_call_reports_daemon_is_healthy(self):
        with fake_eden_daemon(self.temp_dir, ["--sleepBeforeGetPid=2"]):
            status_process = pexpect.spawn(
                FindExe.EDEN_CLI,
                ["--config-dir", str(self.temp_dir), "status", "--timeout", "10"],
                encoding="utf-8",
                logfile=sys.stderr,
            )
            status_process.logfile = sys.stderr
            status_process.expect_exact("eden running normally")
            self.assert_process_succeeds(status_process)

    def assert_process_succeeds(self, process: pexpect.spawn):
        actual_exit_code = wait_for_pexpect_process(process)
        self.assertEqual(
            actual_exit_code,
            0,
            f"Command should return success: {pexpect_process_shell_command(process)}",
        )

    def assert_process_fails(self, process: pexpect.spawn, exit_code: int):
        assert exit_code != 0
        actual_exit_code = wait_for_pexpect_process(process)
        self.assertEqual(
            actual_exit_code,
            exit_code,
            f"Command should return an error code: "
            f"{pexpect_process_shell_command(process)}",
        )


def pexpect_process_shell_command(process: pexpect.spawn) -> str:
    def str_from_strlike(s: typing.Union[bytes, str]) -> str:
        if isinstance(s, str):
            return s
        else:
            return s.decode("utf-8")

    command_parts = [process.command] + [str_from_strlike(arg) for arg in process.args]
    return " ".join(map(shlex.quote, command_parts))


def wait_for_pexpect_process(process: pexpect.spawn) -> int:
    process.expect_exact(pexpect.EOF)
    return process.wait()
