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
import sys
import typing

import pexpect
from eden.cli.config import EdenInstance
from eden.cli.util import HealthStatus
from fb303.ttypes import fb_status

from .lib import testcase
from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin
from .lib.service_test_case import ServiceTestCaseBase, service_test
from .lib.temporary_directory import TemporaryDirectoryMixin


class StartTest(testcase.EdenTestCase):
    def test_start_if_necessary(self) -> None:
        # Confirm there are no checkouts configured, then stop edenfs
        checkouts = self.eden.list_cmd()
        self.assertEqual({}, checkouts)
        self.assertTrue(self.eden.is_healthy())
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())

        # `eden start --if-necessary` should not start eden
        output = self.eden.run_cmd("start", "--if-necessary")
        self.assertEqual("No Eden mount points configured.\n", output)
        self.assertFalse(self.eden.is_healthy())

        # Restart eden and create a checkout
        self.eden.start()
        self.assertTrue(self.eden.is_healthy())

        # Create a repository with one commit
        repo = self.create_hg_repo("testrepo")
        repo.write_file("README", "test\n")
        repo.commit("Initial commit.")
        # Create an Eden checkout of this repository
        checkout_dir = os.path.join(self.mounts_dir, "test_checkout")
        self.eden.clone(repo.path, checkout_dir)

        checkouts = self.eden.list_cmd()
        self.assertEqual({checkout_dir: self.eden.CLIENT_ACTIVE}, checkouts)

        # Stop edenfs
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())
        # `eden start --if-necessary` should start edenfs now
        if "SANDCASTLE" in os.environ:
            output = self.eden.run_cmd(
                "start", "--if-necessary", "--", "--allowRoot", capture_stderr=True
            )
        else:
            output = self.eden.run_cmd("start", "--if-necessary", capture_stderr=True)
        self.assertIn("Started edenfs", output)
        self.assertTrue(self.eden.is_healthy())

        # Stop edenfs.  We didn't start it through self.eden.start()
        # so the self.eden class doesn't really know it is running and that
        # it needs to be shut down.
        self.eden.run_cmd("stop")


@service_test
class StartFakeEdenFSTest(
    ServiceTestCaseBase, PexpectAssertionMixin, TemporaryDirectoryMixin
):
    def setUp(self):
        super().setUp()
        self.eden_dir = pathlib.Path(self.make_temporary_directory())

    def test_eden_start_launches_separate_processes_for_separate_eden_dirs(
        self
    ) -> None:
        eden_dir_1 = self.eden_dir
        eden_dir_2 = pathlib.Path(self.make_temporary_directory())

        start_1_process = self.spawn_start(eden_dir=eden_dir_1)
        self.assert_process_succeeds(start_1_process)
        start_2_process = self.spawn_start(eden_dir=eden_dir_2)
        self.assert_process_succeeds(start_2_process)

        instance_1_health: HealthStatus = EdenInstance(
            str(eden_dir_1), etc_eden_dir=None, home_dir=None
        ).check_health()
        self.assertEqual(
            instance_1_health.status,
            fb_status.ALIVE,
            f"First edenfs process should be healthy, but it isn't: "
            f"{instance_1_health}",
        )

        instance_2_health: HealthStatus = EdenInstance(
            str(eden_dir_2), etc_eden_dir=None, home_dir=None
        ).check_health()
        self.assertEqual(
            instance_2_health.status,
            fb_status.ALIVE,
            f"Second edenfs process should be healthy, but it isn't: "
            f"{instance_2_health}",
        )

        self.assertNotEqual(
            instance_1_health.pid,
            instance_2_health.pid,
            f"The edenfs process should have separate process IDs",
        )

    def test_eden_start_fails_if_edenfs_is_already_running(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir) as daemon_pid:
            start_process = self.spawn_start()
            start_process.expect_exact(f"edenfs is already running (pid {daemon_pid})")
            self.assert_process_fails(start_process, 1)

    def test_eden_start_fails_if_edenfs_fails_during_startup(self) -> None:
        start_process = self.spawn_start(daemon_args=["--failDuringStartup"])
        start_process.expect_exact(
            "Started successfully, but reporting failure because "
            "--failDuringStartup was specified"
        )
        self.assert_process_fails(start_process, 1)

    def spawn_start(
        self,
        daemon_args: typing.Sequence[str] = (),
        eden_dir: typing.Optional[pathlib.Path] = None,
    ) -> "pexpect.spawn[str]":
        if eden_dir is None:
            eden_dir = self.eden_dir
        args = [
            "--config-dir",
            str(eden_dir),
            "start",
            "--daemon-binary",
            FindExe.FAKE_EDENFS,
            "--",
        ]
        args.extend(daemon_args)
        return pexpect.spawn(
            FindExe.EDEN_CLI, args, encoding="utf-8", logfile=sys.stderr
        )
