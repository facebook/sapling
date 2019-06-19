#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import subprocess
import sys
import typing

import eden.thrift
import eden.thrift.client
import pexpect
from eden.cli.config import EdenInstance
from eden.cli.util import HealthStatus

from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin
from .lib.service_test_case import (
    ServiceTestCaseBase,
    SystemdServiceTestCaseMarker,
    service_test,
)


class RestartTestBase(ServiceTestCaseBase):
    def setUp(self) -> None:
        self.eden_dir = self.tmp_dir / "eden"
        self.eden_dir.mkdir()

        def ensure_stopped() -> None:
            stop_cmd = (
                [
                    typing.cast(str, FindExe.EDEN_CLI),  # T38947910
                    "--config-dir",
                    str(self.eden_dir),
                ]
                + self.get_required_eden_cli_args()
                + ["stop"]
            )
            subprocess.call(stop_cmd)

        self.addCleanup(ensure_stopped)

    def _spawn_restart(self, *args: str) -> "pexpect.spawn[bytes]":
        restart_cmd = (
            [FindExe.EDEN_CLI, "--config-dir", str(self.eden_dir)]
            + self.get_required_eden_cli_args()
            + ["restart", "--daemon-binary", FindExe.FAKE_EDENFS]
        )
        restart_cmd.extend(args)

        print("Retarting eden: %r" % (restart_cmd,))
        return pexpect.spawn(
            restart_cmd[0], restart_cmd[1:], logfile=sys.stdout.buffer, timeout=5
        )

    def _start_fake_edenfs(self) -> int:
        daemon = self.spawn_fake_edenfs(eden_dir=self.eden_dir)
        return daemon.process_id


@service_test
class RestartTest(RestartTestBase, PexpectAssertionMixin):
    def _check_edenfs_health(self) -> HealthStatus:
        instance = EdenInstance(str(self.eden_dir), etc_eden_dir=None, home_dir=None)
        return instance.check_health()

    def test_restart_starts_edenfs_if_not_running(self) -> None:
        """
        Run "eden restart".  It should start it without prompting since edenfs
        is not already running.
        """
        p = self._spawn_restart()
        p.expect_exact("Eden is not currently running.  Starting it...")
        p.expect_exact("Starting fake edenfs daemon")
        p.expect(r"Started edenfs \(pid ([0-9]+)\)")
        int(p.match.group(1))
        p.wait()
        self.assertEqual(p.exitstatus, 0)

    def _get_thrift_client(self) -> eden.thrift.EdenClient:
        return eden.thrift.create_thrift_client(str(self.eden_dir))

    def test_restart(self) -> None:
        self._start_fake_edenfs()

        # Run "eden restart"
        # It should prompt since we are about to do a non-graceful restart.
        p = self._spawn_restart()
        p.expect_exact("About to perform a full restart of Eden")
        p.expect_exact(
            "Note: this will temporarily disrupt access to your Eden-managed "
            "repositories"
        )
        p.expect_exact("Proceed? [y/N] ")
        p.sendline("y")
        p.expect_exact("Starting fake edenfs daemon")
        p.expect(r"Started edenfs \(pid [0-9]+\)")
        p.expect_exact("Successfully restarted edenfs.")
        p.expect_exact(
            "Note: any programs running inside of an Eden-managed "
            "directory will need to cd"
        )
        p.wait()
        self.assertEqual(p.exitstatus, 0)

    def test_eden_restart_creates_new_edenfs_process(self) -> None:
        old_pid = self._start_fake_edenfs()

        p = self._spawn_restart("--force")
        p.expect(r"Started edenfs \(pid (?P<pid>\d+)\)")
        new_pid_from_restart: int = int(p.match.group("pid"))
        new_pid_from_health_check: typing.Optional[
            int
        ] = self._check_edenfs_health().pid

        self.assertIsNotNone(new_pid_from_health_check, "EdenFS should be alive")
        self.assertNotEqual(
            old_pid, new_pid_from_health_check, "EdenFS process ID should have changed"
        )
        self.assertEqual(
            new_pid_from_restart,
            new_pid_from_health_check,
            "'eden restart' should have shown the process ID for the new "
            "EdenFS process",
        )

    def test_restart_sigkill(self) -> None:
        self._start_fake_edenfs()

        # Tell the fake edenfs binary to ignore attempts to stop it
        with self._get_thrift_client() as client:
            client.setOption("honor_stop", "false")

        # Run "eden restart".  It should have to kill eden with SIGKILL during the
        # restart operation.
        # Explicitly pass in a shorter than normal shutdown timeout just to reduce the
        # amount of time required for the test.
        p = self._spawn_restart("--shutdown-timeout=1")
        p.expect_exact("About to perform a full restart of Eden")
        p.expect_exact(
            "Note: this will temporarily disrupt access to your Eden-managed "
            "repositories"
        )
        p.expect_exact("Proceed? [y/N] ")
        p.sendline("y")
        p.expect(
            r"sent shutdown request, but edenfs did not exit within "
            r"[.0-9]+ seconds. Attempting SIGKILL."
        )
        p.expect_exact("Starting fake edenfs daemon")
        p.expect(r"Started edenfs \(pid [0-9]+\)")
        p.expect_exact("Successfully restarted edenfs.")
        p.expect_exact(
            "Note: any programs running inside of an Eden-managed "
            "directory will need to cd"
        )
        p.wait()
        self.assertEqual(p.exitstatus, 0)

    def test_restart_force(self) -> None:
        self._start_fake_edenfs()

        # "eden restart --force" should not prompt if the user wants to proceed
        p = self._spawn_restart("--force")
        p.expect_exact("About to perform a full restart of Eden")
        p.expect_exact(
            "Note: this will temporarily disrupt access to your Eden-managed "
            "repositories"
        )
        p.expect_exact("Starting fake edenfs daemon")
        p.expect(r"Started edenfs \(pid [0-9]+\)")
        p.expect_exact("Successfully restarted edenfs.")
        p.expect_exact(
            "Note: any programs running inside of an Eden-managed "
            "directory will need to cd"
        )
        p.wait()
        self.assertEqual(p.exitstatus, 0)

    def test_restart_while_starting(self) -> None:
        orig_pid = self._start_fake_edenfs()

        # Tell the fake edenfs daemon to report its status as "starting"
        with self._get_thrift_client() as client:
            client.setOption("status", "starting")

        # "eden restart" should not restart if edenfs is still starting
        p = self._spawn_restart()
        p.expect_exact(f"The current edenfs daemon (pid {orig_pid}) is still starting")
        p.expect_exact("Use --force if you want to forcibly restart the current daemon")
        p.wait()
        self.assertEqual(p.exitstatus, 1)

        # "eden restart --force" should force the restart anyway
        p = self._spawn_restart("--force")
        p.expect_exact(f"The current edenfs daemon (pid {orig_pid}) is still starting")
        p.expect_exact("Forcing a full restart...")
        p.expect_exact("Starting fake edenfs daemon")
        p.expect(r"Started edenfs \(pid [0-9]+\)")
        p.expect_exact("Successfully restarted edenfs.")
        p.expect_exact(
            "Note: any programs running inside of an Eden-managed "
            "directory will need to cd"
        )
        p.wait()
        self.assertEqual(p.exitstatus, 0)

    def test_restart_unresponsive_thrift(self) -> None:
        orig_pid = self._start_fake_edenfs()

        # Rename the thrift socket so that "eden restart" will not be able to
        # communicate with the existing daemon.
        (self.eden_dir / eden.thrift.client.SOCKET_PATH).rename(
            self.eden_dir / "old.socket"
        )

        # "eden restart" should not restart if it cannot confirm the current health of
        # edenfs.
        p = self._spawn_restart()
        p.expect_exact(
            f"Found an existing edenfs daemon (pid {orig_pid} that does not "
            "seem to be responding to thrift calls."
        )
        p.expect_exact("Use --force if you want to forcibly restart the current daemon")
        p.wait()
        self.assertEqual(p.exitstatus, 1)

        # "eden restart --force" should force the restart anyway
        p = self._spawn_restart("--force")
        p.expect_exact(
            f"Found an existing edenfs daemon (pid {orig_pid} that does not "
            "seem to be responding to thrift calls."
        )
        p.expect_exact("Forcing a full restart...")
        p.expect_exact("Starting fake edenfs daemon")
        p.expect(r"Started edenfs \(pid [0-9]+\)")
        p.expect_exact("Successfully restarted edenfs.")
        p.expect_exact(
            "Note: any programs running inside of an Eden-managed "
            "directory will need to cd"
        )
        p.wait()
        self.assertEqual(p.exitstatus, 0)

    def test_eden_restart_fails_if_edenfs_crashes_on_start(self) -> None:
        self._start_fake_edenfs()
        restart_process = self._spawn_restart(
            "--force", "--daemon-binary", "/bin/false"
        )
        restart_process.expect_exact("Failed to start edenfs")
        self.assert_process_fails(restart_process, 1)


@service_test
class RestartWithSystemdTest(
    RestartTestBase, SystemdServiceTestCaseMarker, PexpectAssertionMixin
):
    def test_eden_restart_starts_service_if_not_running(self) -> None:
        restart_process = self._spawn_restart()
        restart_process.expect_exact("Eden is not currently running.  Starting it...")
        restart_process.expect_exact("Started edenfs")
        self.assert_process_succeeds(restart_process)
        self.assert_systemd_service_is_active(eden_dir=self.eden_dir)

    def test_service_is_active_after_full_eden_restart(self) -> None:
        self._start_fake_edenfs()
        restart_process = self._spawn_restart("--force")
        self.assert_process_succeeds(restart_process)
        self.assert_systemd_service_is_active(eden_dir=self.eden_dir)

    def test_graceful_restart_is_not_supported_yet(self) -> None:
        self._start_fake_edenfs()
        restart_process = self._spawn_restart("--graceful")
        restart_process.expect_exact("NotImplementedError")
        restart_process.expect_exact("eden restart --graceful")
        self.assert_process_fails(restart_process, 1)
        self.assert_systemd_service_is_active(eden_dir=self.eden_dir)
