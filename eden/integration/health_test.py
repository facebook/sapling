#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import signal
import sys
import typing

from eden.fs.cli.daemon import wait_for_shutdown

from .lib import edenclient, testcase
from .lib.find_executables import FindExe
from .lib.pexpect import pexpect_spawn, PexpectAssertionMixin, PexpectSpawnType
from .lib.service_test_case import service_test, ServiceTestCaseBase


@testcase.eden_test
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


@service_test()
class HealthOfFakeEdenFSTest(ServiceTestCaseBase, PexpectAssertionMixin):
    def setUp(self) -> None:
        super().setUp()
        self.temp_dir = self.make_temp_dir()

    def test_healthy_daemon_is_healthy(self) -> None:
        with self.spawn_fake_edenfs(self.temp_dir):
            status_process = self.spawn_status([])
            status_process.expect_exact("EdenFS is running normally")
            self.assert_process_succeeds(status_process)

    def test_killed_daemon_is_not_running(self) -> None:
        with self.spawn_fake_edenfs(self.temp_dir) as daemon_pid:
            os.kill(daemon_pid, signal.SIGKILL)
            wait_for_shutdown(pid=daemon_pid, timeout=5)

            status_process = self.spawn_status([])
            status_process.expect_exact("EdenFS is not running")
            self.assert_process_fails(status_process, exit_code=1)

    def test_hanging_thrift_call_reports_daemon_is_unresponsive(self) -> None:
        with self.spawn_fake_edenfs(self.temp_dir, ["--sleepBeforeGetPid=5"]):
            status_process = self.spawn_status(["--timeout", "1"])
            status_process.expect_exact(
                "EdenFS's Thrift server does not appear to be running, but the "
                "process is still alive"
            )
            self.assert_process_fails(status_process, exit_code=1)

    def test_slow_thrift_call_reports_daemon_is_healthy(self) -> None:
        with self.spawn_fake_edenfs(self.temp_dir, ["--sleepBeforeGetPid=2"]):
            status_process = self.spawn_status(["--timeout", "10"])
            status_process.expect_exact("EdenFS is running normally")
            self.assert_process_succeeds(status_process)

    def spawn_status(self, extra_args: typing.List[str]) -> PexpectSpawnType:
        edenfsctl, env = FindExe.get_edenfsctl_env()
        return pexpect_spawn(
            edenfsctl,
            ["--config-dir", str(self.temp_dir)]
            + self.get_required_eden_cli_args()
            + ["status"]
            + extra_args,
            env=env,
            encoding="utf-8",
            logfile=sys.stderr,
        )
