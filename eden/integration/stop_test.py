#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import os
import signal
import sys
import time
from pathlib import Path
from typing import Callable, List, Optional

from eden.fs.cli import proc_utils as proc_utils_mod
from eden.fs.cli.daemon import wait_for_process_exit
from eden.fs.cli.util import poll_until

from .lib.find_executables import FindExe
from .lib.pexpect import (
    pexpect_spawn,
    PexpectAssertionMixin,
    PexpectSpawnType,
    wait_for_pexpect_process,
)
from .lib.service_test_case import service_test, ServiceTestCaseBase
from .lib.testcase import eden_test, EdenTestCase


SHUTDOWN_EXIT_CODE_NORMAL = 0
SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN = 0
SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR = 2
SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL = 3


class StopTestBase(ServiceTestCaseBase):
    eden_dir: Path

    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = self.tmp_dir / "eden"
        self.eden_dir.mkdir()

    def spawn_stop(self, extra_args: List[str]) -> PexpectSpawnType:
        edenfsctl, env = FindExe.get_edenfsctl_env()
        return pexpect_spawn(
            edenfsctl,
            ["--config-dir", str(self.eden_dir)]
            + self.get_required_eden_cli_args()
            + ["stop"]
            + extra_args,
            env=env,
            encoding="utf-8",
            logfile=sys.stderr,
        )


@service_test
class StopTest(StopTestBase, PexpectAssertionMixin):
    def test_stop_stops_running_daemon(self) -> None:
        proc_utils = proc_utils_mod.new()
        with self.spawn_fake_edenfs(self.eden_dir) as daemon_pid:
            stop_process = self.spawn_stop(["--timeout", "5"])
            stop_process.expect_exact("edenfs exited cleanly.")
            self.assert_process_exit_code(stop_process, SHUTDOWN_EXIT_CODE_NORMAL)
            self.assertFalse(
                proc_utils.is_process_alive(daemon_pid),
                f"Process {daemon_pid} should have died",
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

            self.assertTrue(wait_for_process_exit(daemon_pid, timeout=10))

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


@eden_test
class AutoStopTest(EdenTestCase):
    def update_validity_interval(self, interval: str) -> None:
        config_text = f"""
[config]
[core]
check-validity-interval = "{interval}"
"""
        self.eden.user_rc_path.write_text(config_text)
        with self.get_thrift_client_legacy() as client:
            client.reloadConfig()

    def _run_test(self, invalidate_fn: Callable[[], None], timeout: float = 15) -> None:
        self.update_validity_interval("20ms")

        # Run the function which will invalidate the state directory
        invalidate_fn()

        # EdenFS should exit on its own
        optional_edenfs = self.eden._process
        assert optional_edenfs is not None
        edenfs = optional_edenfs

        def edenfs_exited() -> Optional[bool]:
            returncode = edenfs.poll()
            if returncode is None:
                return None
            return True

        poll_until(edenfs_exited, timeout=timeout)

    def test_delete_lock_file(self) -> None:
        def delete_lock_file() -> None:
            (self.eden.eden_dir / "lock").unlink()

        self._run_test(delete_lock_file)

    def test_replace_lock_file(self) -> None:
        def replace_lock_file() -> None:
            lock_path = self.eden.eden_dir / "lock"
            new_lock_path = self.eden.eden_dir / "lock2"
            new_lock_path.touch()
            new_lock_path.rename(lock_path)

        self._run_test(replace_lock_file)

    def test_move_state_dir(self) -> None:
        def move_state_dir() -> None:
            new_path = Path(self.tmp_dir) / "new-eden-dir"
            self.eden.eden_dir.rename(new_path)

        self._run_test(move_state_dir)

    def test_runs_normally(self) -> None:
        """Make sure that EdenFS continues running normally if the lock file
        isn't replaced.
        """

        def noop() -> None:
            pass

        # Call _run_test().  Since we don't replace the lock file it should time
        # out waiting for edenfs to exit.
        with self.assertRaises(TimeoutError):
            self._run_test(noop, timeout=5)

        self.assertTrue(self.eden.is_healthy())
