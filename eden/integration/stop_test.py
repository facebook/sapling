#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import os
import pathlib
import signal
import sys
import time
import typing

import pexpect
from eden.cli.daemon import did_process_exit
from eden.cli.util import poll_until

from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin, wait_for_pexpect_process
from .lib.service_test_case import (
    ServiceTestCaseBase,
    SystemdServiceTestCaseMarker,
    service_test,
)


SHUTDOWN_EXIT_CODE_NORMAL = 0
SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN = 0
SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR = 2
SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL = 3


# pyre-fixme[38]: `StopTestBase` does not implement all inherited abstract methods.
class StopTestBase(ServiceTestCaseBase):
    eden_dir: pathlib.Path

    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = self.tmp_dir / "eden"
        self.eden_dir.mkdir()

    def spawn_stop(self, extra_args: typing.List[str]) -> "pexpect.spawn[str]":
        return pexpect.spawn(
            FindExe.EDEN_CLI,
            ["--config-dir", str(self.eden_dir)]
            + self.get_required_eden_cli_args()
            + ["stop"]
            + extra_args,
            encoding="utf-8",
            logfile=sys.stderr,
        )


@service_test
# pyre-fixme[38]: `StopTest` does not implement all inherited abstract methods.
class StopTest(StopTestBase, PexpectAssertionMixin):
    def test_stop_stops_running_daemon(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir) as daemon_pid:
            stop_process = self.spawn_stop(["--timeout", "5"])
            stop_process.expect_exact("edenfs exited cleanly.")
            self.assert_process_exit_code(stop_process, SHUTDOWN_EXIT_CODE_NORMAL)
            self.assertTrue(
                did_process_exit(daemon_pid), f"Process {daemon_pid} should have died"
            )

    def test_eden_stop_shuts_down_edenfs_cleanly(self) -> None:
        clean_shutdown_file = self.eden_dir / "clean_shutdown"
        assert not clean_shutdown_file.exists()

        with self.spawn_fake_edenfs(
            self.eden_dir, ["--cleanShutdownFile", str(clean_shutdown_file)]
        ):
            self.assertFalse(
                clean_shutdown_file.exists(),
                f"{clean_shutdown_file} should not exist after starting EdenFS",
            )

            stop_process = self.spawn_stop([])
            wait_for_pexpect_process(stop_process)
            self.assertTrue(
                clean_shutdown_file.exists(),
                f"{clean_shutdown_file} should exist after EdenFS cleanly shuts down",
            )

    def test_stop_sigkill(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir, ["--ignoreStop"]):
            # Ask the CLI to stop edenfs, with a 1 second timeout.
            # It should have to kill the process with SIGKILL
            stop_process = self.spawn_stop(["--timeout", "1"])
            stop_process.expect_exact("Terminated edenfs with SIGKILL")
            self.assert_process_exit_code(
                stop_process, SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
            )

    def test_stop_kill(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir, ["--ignoreStop"]):
            # Run "eden stop --kill"
            # This should attempt to kill edenfs immediately with SIGKILL.
            stop_process = self.spawn_stop(["--kill", "--timeout", "1"])
            stop_process.expect_exact("Terminated edenfs with SIGKILL")
            self.assert_process_exit_code(stop_process, SHUTDOWN_EXIT_CODE_NORMAL)

    def test_async_stop_stops_daemon_eventually(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir) as daemon_pid:
            stop_process = self.spawn_stop(["--timeout", "0"])
            stop_process.expect_exact("Sent async shutdown request to edenfs.")
            self.assert_process_exit_code(
                stop_process, SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN
            )

            def daemon_exited() -> typing.Optional[bool]:
                if did_process_exit(daemon_pid):
                    return True
                else:
                    return None

            poll_until(daemon_exited, timeout=10)

    def test_stop_not_running(self) -> None:
        stop_process = self.spawn_stop(["--timeout", "1"])
        stop_process.expect_exact("edenfs is not running")
        self.assert_process_exit_code(
            stop_process, SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR
        )

    def test_stopping_killed_daemon_reports_not_running(self) -> None:
        daemon = self.spawn_fake_edenfs(self.eden_dir)
        os.kill(daemon.process_id, signal.SIGKILL)

        stop_process = self.spawn_stop(["--timeout", "1"])
        stop_process.expect_exact("edenfs is not running")
        self.assert_process_exit_code(
            stop_process, SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR
        )

    def test_killing_hung_daemon_during_stop_makes_stop_finish(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir) as daemon_pid:
            os.kill(daemon_pid, signal.SIGSTOP)
            try:
                stop_process = self.spawn_stop(["--timeout", "5"])

                time.sleep(2)
                self.assertTrue(
                    stop_process.isalive(),
                    "'eden stop' should wait while daemon is hung",
                )

                os.kill(daemon_pid, signal.SIGKILL)

                stop_process.expect_exact("error: edenfs is not running")
                self.assert_process_exit_code(
                    stop_process, SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR
                )
            finally:
                with contextlib.suppress(ProcessLookupError):
                    os.kill(daemon_pid, signal.SIGCONT)

    def test_stopping_daemon_stopped_by_sigstop_kills_daemon(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir) as daemon_pid:
            os.kill(daemon_pid, signal.SIGSTOP)
            try:
                stop_process = self.spawn_stop(["--timeout", "1"])
                stop_process.expect_exact("warning: edenfs is not responding")
                self.assert_process_exit_code(
                    stop_process, SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
                )
            finally:
                with contextlib.suppress(ProcessLookupError):
                    os.kill(daemon_pid, signal.SIGCONT)

    def test_hanging_thrift_call_kills_daemon_with_sigkill(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir, ["--sleepBeforeStop=5"]):
            stop_process = self.spawn_stop(["--timeout", "1"])
            self.assert_process_exit_code(
                stop_process, SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
            )

    def test_stop_succeeds_if_thrift_call_abruptly_kills_daemon(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir, ["--exitWithoutCleanupOnStop"]):
            stop_process = self.spawn_stop(["--timeout", "10"])
            self.assert_process_exit_code(stop_process, SHUTDOWN_EXIT_CODE_NORMAL)


@service_test
# pyre-fixme[38]: `StopWithSystemdTest` does not implement all inherited abstract
#  methods.
class StopWithSystemdTest(
    SystemdServiceTestCaseMarker, StopTestBase, PexpectAssertionMixin
):
    def test_stop_stops_systemd_service(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir):
            stop_process = self.spawn_stop(["--timeout", "5"])
            stop_process.expect_exact("edenfs exited cleanly.")
            self.assert_process_exit_code(stop_process, SHUTDOWN_EXIT_CODE_NORMAL)
            self.assert_systemd_service_is_stopped(eden_dir=self.eden_dir)
