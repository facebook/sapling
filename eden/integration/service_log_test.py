#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import pathlib
import signal
import subprocess
from typing import Dict, List, Tuple

from eden.integration.lib.find_executables import FindExe

from .lib.service_test_case import fake_service_test, service_test, ServiceTestCaseBase
from .lib.start import run_eden_start_with_real_daemon


class ServiceLogTestBase(ServiceTestCaseBase):
    """Test how the EdenFS service stores its logs."""

    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = self.make_temp_dir("eden")

    @property
    def log_file_path(self) -> pathlib.Path:
        return self.eden_dir / "logs" / "edenfs.log"


@service_test
class ServiceLogFakeEdenFSTest(ServiceLogTestBase):
    def test_fake_edenfs_writes_logs_to_file_in_eden_dir(self) -> None:
        self.assertFalse(
            self.log_file_path.exists(),
            f"{self.log_file_path} should not exist before starting fake_edenfs",
        )
        with self.spawn_fake_edenfs(self.eden_dir):
            self.assertTrue(
                self.log_file_path.exists(),
                f"fake_edenfs should create {self.log_file_path}",
            )
            self.assertIn("Starting fake edenfs daemon", self.log_file_path.read_text())

    def test_fake_edenfs_appends_to_existing_log_file(self) -> None:
        self.log_file_path.parent.mkdir(exist_ok=True, parents=True)
        self.log_file_path.write_text("test log messages\n")
        with self.spawn_fake_edenfs(self.eden_dir):
            pass
        self.assertIn("test log messages", self.log_file_path.read_text())


@fake_service_test
class ServiceLogRealEdenFSTest(ServiceLogTestBase):
    def get_cli_args_and_env(self) -> Tuple[List[str], Dict[str, str]]:
        edenfsctl, edenfsctl_env = FindExe.get_edenfsctl_env()
        env = dict(os.environ)
        env.update(edenfsctl_env)
        eden_cli_args: List[str] = [
            edenfsctl,
            "--config-dir",
            str(self.eden_dir),
            "--etc-eden-dir",
            str(self.etc_eden_dir),
            "--home-dir",
            str(self.home_dir),
        ]

        return (eden_cli_args, env)

    def get_running_daemon_pid(self) -> str:
        eden_cli_args, env = self.get_cli_args_and_env()

        return subprocess.check_output(
            eden_cli_args + ["pid"], env=env, encoding="utf-8"
        )

    def set_running_daemon_log_level(self, log_level: str) -> str:
        eden_cli_args, env = self.get_cli_args_and_env()

        return subprocess.check_output(
            eden_cli_args + ["debug", "logging", f".={log_level}"],
            env=env,
            encoding="utf-8",
        )

    def test_real_edenfs_writes_logs_to_file_in_eden_dir(self) -> None:
        self.assertFalse(
            self.log_file_path.exists(),
            f"{self.log_file_path} should not exist before starting edenfs",
        )
        self.exit_stack.enter_context(
            run_eden_start_with_real_daemon(
                eden_dir=self.eden_dir,
                etc_eden_dir=self.etc_eden_dir,
                home_dir=self.home_dir,
            )
        )
        self.assertTrue(
            self.log_file_path.exists(), f"edenfs should create {self.log_file_path}"
        )
        self.assertIn("Starting edenfs", self.log_file_path.read_text())

    def test_eden_reopens_log_file_on_sighup(self) -> None:
        self.assertFalse(
            self.log_file_path.exists(),
            f"{self.log_file_path} should not exist before starting edenfs",
        )

        self.exit_stack.enter_context(
            run_eden_start_with_real_daemon(
                eden_dir=self.eden_dir,
                etc_eden_dir=self.etc_eden_dir,
                home_dir=self.home_dir,
            )
        )

        self.assertTrue(
            self.log_file_path.exists(),
            f"{self.log_file_path} should exist after starting edenfs",
        )

        # remove and recreate the log file so that the daemon continues to
        # write to the unlinked log file
        self.log_file_path.unlink()
        self.log_file_path.touch(mode=0o644, exist_ok=True)
        new_log_size = os.stat(self.log_file_path).st_size
        self.assertEqual(
            new_log_size, 0, f"{self.log_file_path} should be 0 size after creation"
        )

        # crank log level to ensure verbose logging occurs
        self.set_running_daemon_log_level("DBG4")

        # trigger an EdenFS thrift call to force daemon to "log" something
        pid = int(self.get_running_daemon_pid().strip())
        self.assertEqual(
            0,
            os.stat(self.log_file_path).st_size,
            f"{self.log_file_path} should be unwriteable after removal",
        )

        # Send a signal to trigger reopening Eden logs
        os.kill(pid, signal.SIGHUP)

        # retrigger an EdenFS thrift call and ensure the daemon logged to the reopened logs
        self.get_running_daemon_pid()
        self.assertLess(
            0,
            os.stat(self.log_file_path).st_size,
            f"{self.log_file_path} should be written to after SIGHUP",
        )

        # This is the error for failing to redirect stderr or stdout during
        # signal handling. Ensure it's not present in the logs.
        self.assertNotIn("Failed to redirect", self.log_file_path.read_text())
