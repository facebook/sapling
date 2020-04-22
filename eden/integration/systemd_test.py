#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import pathlib
import shutil
import signal
import subprocess
import sys
import typing
import unittest

import pexpect
import toml
from eden.test_support.testcase import EdenTestCaseBase

from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin
from .lib.service_test_case import SystemdServiceTest


class SystemdTest(SystemdServiceTest, PexpectAssertionMixin):
    """Test Eden's systemd service for Linux."""

    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = self.make_test_dir("eden")

    # TODO(T33122320): Delete this test when systemd is properly integrated.
    def test_eden_start_with_systemd_disabled_does_not_say_systemd_mode_is_enabled(
        self
    ) -> None:
        self.unsetenv("EDEN_EXPERIMENTAL_SYSTEMD")

        def test(start_args: typing.List[str]) -> None:
            eden_cli: str = FindExe.EDEN_CLI  # pyre-ignore[9]: T38947910
            with self.subTest(start_args=start_args):
                start_process: "pexpect.spawn[str]" = pexpect.spawn(
                    eden_cli,
                    self.get_required_eden_cli_args()
                    + ["start", "--foreground"]
                    + start_args,
                    encoding="utf-8",
                    logfile=sys.stderr,
                )
                start_process.expect_exact("Started edenfs")
                self.assertNotIn(
                    "Running in experimental systemd mode", start_process.before
                )
                subprocess.check_call(
                    [eden_cli]
                    + self.get_required_eden_cli_args()
                    + ["stop", "--timeout", "0"]
                )
                start_process.wait()

        test(start_args=["--", "--allowRoot"])
        # pyre-ignore[6]: T38947910
        test(start_args=["--daemon-binary", FindExe.FAKE_EDENFS])

    def test_eden_start_starts_systemd_service(self) -> None:
        subprocess.check_call(
            self.get_edenfsctl_cmd()
            # pyre-ignore[6]: T38947910
            + ["start", "--daemon-binary", FindExe.FAKE_EDENFS]
        )
        self.assert_systemd_service_is_active(eden_dir=self.eden_dir)

    def test_systemd_service_is_failed_if_edenfs_crashes_on_start(self) -> None:
        self.assert_systemd_service_is_stopped(eden_dir=self.eden_dir)
        subprocess.call(
            self.get_edenfsctl_cmd()
            + [  # pyre-ignore[6]: T38947910
                "start",
                "--daemon-binary",
                FindExe.FAKE_EDENFS,
                "--",
                "--failDuringStartup",
            ]
        )
        self.assert_systemd_service_is_failed(eden_dir=self.eden_dir)

    def test_eden_start_reports_service_failure_if_edenfs_fails_during_startup(
        self
    ) -> None:
        start_process = self.spawn_start_with_fake_edenfs(
            extra_args=["--", "--failDuringStartup"]
        )
        start_process.expect(
            r"error: Starting the fb-edenfs@.+?\.service systemd service "
            r"failed \(reason: exit-code\)"
        )
        self.assertNotIn(
            "journalctl",
            start_process.before,
            "journalctl doesn't work and should not be mentioned",
        )
        remaining_output = start_process.read()
        self.assertNotIn(
            "journalctl",
            remaining_output,
            "journalctl doesn't work and should not be mentioned",
        )

    def test_eden_start_reports_error_if_systemd_is_dead(self) -> None:
        systemd = self.systemd
        assert systemd is not None
        systemd.exit()
        self.assertTrue(
            (systemd.xdg_runtime_dir / "systemd" / "private").exists(),
            "systemd's socket file should still exist",
        )

        self.spoof_user_name("testuser")
        start_process = self.spawn_start_with_fake_edenfs()
        start_process.expect_exact(
            "error: The systemd user manager is not running. Run the following "
            "command to\r\nstart it, then try again:"
        )
        start_process.expect_exact("sudo systemctl start user@testuser.service")

    def test_eden_start_reports_error_if_systemd_is_dead_and_cleaned_up(self) -> None:
        systemd = self.systemd
        assert systemd is not None
        systemd.exit()
        shutil.rmtree(systemd.xdg_runtime_dir)

        self.spoof_user_name("testuser")
        start_process = self.spawn_start_with_fake_edenfs()
        start_process.expect_exact(
            "error: The systemd user manager is not running. Run the following "
            "command to\r\nstart it, then try again:"
        )
        start_process.expect_exact("sudo systemctl start user@testuser.service")

    def test_eden_start_uses_fallback_if_systemd_environment_is_missing(self) -> None:
        systemd = self.systemd
        assert systemd is not None

        fallback_xdg_runtime_dir = str(systemd.xdg_runtime_dir)
        self.set_eden_config(
            {"service": {"fallback_systemd_xdg_runtime_dir": fallback_xdg_runtime_dir}}
        )
        self.unsetenv("XDG_RUNTIME_DIR")

        start_process = self.spawn_start_with_fake_edenfs()
        start_process.expect_exact(
            f"warning: The XDG_RUNTIME_DIR environment variable is not set; "
            f"using fallback: '{fallback_xdg_runtime_dir}'"
        )
        start_process.expect_exact("Started edenfs")
        self.assert_process_succeeds(start_process)
        self.assert_systemd_service_is_active(eden_dir=self.eden_dir)

        edenfs_log = self.eden_dir / "logs" / "edenfs.log"
        log_contents = edenfs_log.read_text(encoding="utf-8", errors="replace")
        self.assertIn("Running in experimental systemd mode", log_contents)

    def spawn_start_with_fake_edenfs(
        self, extra_args: typing.Sequence[str] = ()
    ) -> "pexpect.spawn[str]":
        return pexpect.spawn(
            # pyre-ignore[6]: T38947910
            FindExe.EDEN_CLI,
            self.get_required_eden_cli_args()
            # pyre-ignore[6]: T38947910
            + ["start", "--daemon-binary", FindExe.FAKE_EDENFS] + list(extra_args),
            encoding="utf-8",
            logfile=sys.stderr,
        )

    def get_edenfsctl_cmd(self) -> typing.List[str]:
        # pyre-ignore[6,7]: T38947910
        return [FindExe.EDEN_CLI] + self.get_required_eden_cli_args()

    def get_required_eden_cli_args(self) -> typing.List[str]:
        return [
            "--config-dir",
            str(self.eden_dir),
            "--etc-eden-dir",
            str(self.etc_eden_dir),
            "--home-dir",
            str(self.home_dir),
        ]

    def set_eden_config(self, config) -> None:
        config_d = self.etc_eden_dir / "config.d"
        config_d.mkdir()
        with open(config_d / "systemd.toml", "w") as config_file:
            # pyre-ignore[6]: T39129461
            toml.dump(config, config_file)

    def spoof_user_name(self, user_name: str) -> None:
        self.setenv("LOGNAME", user_name)
        self.setenv("USER", user_name)
