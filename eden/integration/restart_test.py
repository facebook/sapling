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
import subprocess
import sys
import unittest

import eden.thrift
import eden.thrift.client
import pexpect

from .lib.fake_edenfs import FakeEdenFS
from .lib.find_executables import FindExe
from .lib.temporary_directory import TemporaryDirectoryMixin


class RestartTest(unittest.TestCase, TemporaryDirectoryMixin):
    def setUp(self) -> None:
        self.tmp_dir = self.make_temporary_directory()

        def ensure_stopped() -> None:
            stop_cmd = [FindExe.EDEN_CLI, "--config-dir", self.tmp_dir, "stop"]
            subprocess.call(stop_cmd)

        self.addCleanup(ensure_stopped)

    def _spawn_restart(self, *args: str) -> "pexpect.spawn[bytes]":
        restart_cmd = [
            FindExe.EDEN_CLI,
            "--config-dir",
            self.tmp_dir,
            "restart",
            "--daemon-binary",
            FindExe.FAKE_EDENFS,
        ]
        restart_cmd.extend(args)

        print("Retarting eden: %r" % (restart_cmd,))
        return pexpect.spawn(
            restart_cmd[0], restart_cmd[1:], logfile=sys.stdout.buffer, timeout=5
        )

    def _start_fake_edenfs(self) -> int:
        daemon = FakeEdenFS.spawn_via_cli(eden_dir=pathlib.Path(self.tmp_dir))
        return daemon.process_id

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
        return eden.thrift.create_thrift_client(self.tmp_dir)

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
        os.rename(
            os.path.join(self.tmp_dir, eden.thrift.client.SOCKET_PATH),
            os.path.join(self.tmp_dir, "old.socket"),
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
