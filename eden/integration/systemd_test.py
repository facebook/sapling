#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import shutil
import subprocess
import sys
import typing
from typing import Optional

import pexpect
import toml

from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin, PexpectSpawnType, pexpect_spawn
from .lib.service_test_case import SystemdServiceTest, systemd_test


@systemd_test
class SystemdTest(SystemdServiceTest, PexpectAssertionMixin):
    """Test Eden's systemd service for Linux."""

    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = self.make_test_dir("eden")

    # TODO(T33122320): Delete this test when systemd is properly integrated.
    def test_eden_start_with_systemd_disabled_does_not_say_systemd_mode_is_enabled(
        self,
    ) -> None:
        self.unsetenv("EDEN_EXPERIMENTAL_SYSTEMD")

        def test(start_args: typing.List[str]) -> None:
            edenfsctl, env = FindExe.get_edenfsctl_env()
            with self.subTest(start_args=start_args):
                start_process: "pexpect.spawn[str]" = pexpect.spawn(
                    edenfsctl,
                    self.get_required_eden_cli_args()
                    + ["start", "--foreground"]
                    + start_args,
                    env=env,
                    encoding="utf-8",
                    logfile=sys.stderr,
                )
                start_process.expect_exact("Started EdenFS")
                self.assertNotIn(
                    "Running in experimental systemd mode", start_process.before
                )
                subprocess.check_call(
                    [edenfsctl]
                    + self.get_required_eden_cli_args()
                    + ["stop", "--timeout", "0"],
                    env=env,
                )
                start_process.wait()

        real_daemon_args = ["--daemon-binary", FindExe.EDEN_DAEMON, "--", "--allowRoot"]
        privhelper = FindExe.EDEN_PRIVHELPER
        if privhelper is not None:
            real_daemon_args.extend(["--privhelper_path", privhelper])

        test(start_args=real_daemon_args)
        test(start_args=["--daemon-binary", FindExe.FAKE_EDENFS])

    def test_eden_start_starts_systemd_service(self) -> None:
        edenfsctl, env = self.get_edenfsctl_cmd()
        subprocess.check_call(
            edenfsctl + ["start", "--daemon-binary", FindExe.FAKE_EDENFS], env=env
        )
        self.assert_systemd_service_is_active(eden_dir=self.eden_dir)

    def test_systemd_service_is_failed_if_edenfs_crashes_on_start(self) -> None:
        self.assert_systemd_service_is_stopped(eden_dir=self.eden_dir)
        edenfsctl, env = self.get_edenfsctl_cmd()
        subprocess.call(
            edenfsctl
            + [
                "start",
                "--daemon-binary",
                FindExe.FAKE_EDENFS,
                "--",
                "--failDuringStartup",
            ],
            env=env,
        )
        self.assert_systemd_service_is_failed(eden_dir=self.eden_dir)

    def test_eden_start_reports_service_failure_if_edenfs_fails_during_startup(
        self,
    ) -> None:
        start_process = self.spawn_start_with_fake_edenfs(
            extra_args=["--", "--failDuringStartup"]
        )
        start_process.expect(
            r"error: Starting the fb-edenfs@.+?\.service systemd service "
            r"failed \(reason: exit-code\)"
        )
        before = typing.cast(Optional[str], start_process.before)
        assert before is not None
        self.assertNotIn(
            "journalctl", before, "journalctl doesn't work and should not be mentioned"
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
        start_process.expect_exact("Started EdenFS")
        self.assert_process_succeeds(start_process)
        self.assert_systemd_service_is_active(eden_dir=self.eden_dir)

        edenfs_log = self.eden_dir / "logs" / "edenfs.log"
        log_contents = edenfs_log.read_text(encoding="utf-8", errors="replace")
        self.assertIn("Running in experimental systemd mode", log_contents)

    def spawn_start_with_fake_edenfs(
        self, extra_args: typing.Sequence[str] = ()
    ) -> PexpectSpawnType:
        edenfsctl, env = FindExe.get_edenfsctl_env()
        return pexpect_spawn(
            edenfsctl,
            self.get_required_eden_cli_args()
            + ["start", "--daemon-binary", FindExe.FAKE_EDENFS]
            + list(extra_args),
            env=env,
            encoding="utf-8",
            logfile=sys.stderr,
        )

    def get_edenfsctl_cmd(
        self,
    ) -> typing.Tuple[typing.List[str], typing.Dict[str, str]]:
        edenfsctl, env = FindExe.get_edenfsctl_env()
        return [edenfsctl] + self.get_required_eden_cli_args(), env

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
            toml.dump(config, config_file)

    def spoof_user_name(self, user_name: str) -> None:
        self.setenv("LOGNAME", user_name)
        self.setenv("USER", user_name)
