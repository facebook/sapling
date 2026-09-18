#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

from eden.fs.service.eden.thrift_types import MountInfo, MountState

from .lib import edenclient, testcase


class FuseTransportVerificationTest(unittest.TestCase):
    base_dir: pathlib.Path
    eden: edenclient.EdenFS

    def setUp(self) -> None:
        super().setUp()
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        self.base_dir = pathlib.Path(temp_dir.name)
        self.eden = edenclient.EdenFS(
            base_dir=self.base_dir, expected_fuse_transport="io_uring"
        )

    def test_no_expectation_needs_no_daemon(self) -> None:
        with mock.patch.object(
            edenclient.client, "create_thrift_client"
        ) as create_client:
            edenclient.EdenFS(base_dir=self.base_dir).assert_running_fuse_transports()
            create_client.assert_not_called()

    def test_checks_every_running_fuse_mount(self) -> None:
        for first, actual, error in (
            ("io_uring", "devfuse", unittest.SkipTest),
            ("io_uring", None, AssertionError),
            ("devfuse", None, AssertionError),
            ("devfuse", "unknown", AssertionError),
        ):
            with self.subTest(first=first, actual=actual):
                with mock.patch.object(
                    edenclient.client, "create_thrift_client"
                ) as create_client:
                    thrift_client = create_client.return_value.__enter__.return_value
                    thrift_client.listMounts.return_value = [
                        MountInfo(
                            mountPoint=b"/ring",
                            edenClientPath=b"/client-ring",
                            state=MountState.RUNNING,
                            fsChannelType="fuse",
                            fuseTransport=first,
                        ),
                        MountInfo(
                            mountPoint=b"/fallback",
                            edenClientPath=b"/client-fallback",
                            state=MountState.RUNNING,
                            fsChannelType="fuse",
                            fuseTransport=actual,
                        ),
                    ]
                    with self.assertRaisesRegex(
                        error, "/fallback: expected 'io_uring', got"
                    ):
                        self.eden.assert_running_fuse_transports()

    def test_ignores_nonrunning_and_nonfuse_mounts(self) -> None:
        with mock.patch.object(
            edenclient.client, "create_thrift_client"
        ) as create_client:
            thrift_client = create_client.return_value.__enter__.return_value
            thrift_client.listMounts.return_value = [
                MountInfo(
                    mountPoint=b"/ring",
                    edenClientPath=b"/client-ring",
                    state=MountState.RUNNING,
                    fsChannelType="fuse",
                    fuseTransport="io_uring",
                ),
                MountInfo(
                    mountPoint=b"/initializing",
                    edenClientPath=b"/client-initializing",
                    state=MountState.INITIALIZING,
                    fsChannelType="fuse",
                ),
                MountInfo(
                    mountPoint=b"/failed",
                    edenClientPath=b"/client-failed",
                    state=MountState.INIT_ERROR,
                    fsChannelType="fuse",
                ),
                MountInfo(
                    mountPoint=b"/nfs",
                    edenClientPath=b"/client-nfs",
                    state=MountState.RUNNING,
                    fsChannelType="nfs3",
                ),
            ]
            self.eden.assert_running_fuse_transports()
            thrift_client.listMounts.assert_called_once_with()

    def test_allows_no_mounted_checkouts(self) -> None:
        with mock.patch.object(
            edenclient.client, "create_thrift_client"
        ) as create_client:
            thrift_client = create_client.return_value.__enter__.return_value
            thrift_client.listMounts.return_value = []
            self.eden.assert_running_fuse_transports()
            thrift_client.listMounts.assert_called_once_with()

    def test_successful_mount_commands_propagate_skip(self) -> None:
        for command in ("clone", "mount", "restart"):
            with (
                self.subTest(command=command),
                mock.patch.object(
                    self.eden, "get_edenfsctl_cmd_env", return_value=([command], {})
                ),
                mock.patch.object(edenclient.subprocess, "run"),
                mock.patch.object(
                    self.eden,
                    "assert_running_fuse_transports",
                    side_effect=unittest.SkipTest("fallback"),
                ),
            ):
                with self.assertRaisesRegex(unittest.SkipTest, "fallback"):
                    self.eden.run_cmd(command)

    def test_command_failure_is_not_skipped(self) -> None:
        with (
            mock.patch.object(
                self.eden, "get_edenfsctl_cmd_env", return_value=(["mount"], {})
            ),
            mock.patch.object(
                edenclient.subprocess,
                "run",
                side_effect=subprocess.CalledProcessError(
                    1, ["mount"], stderr="Cannot allocate memory"
                ),
            ),
            mock.patch.object(self.eden, "assert_running_fuse_transports") as verify,
        ):
            with self.assertRaises(edenclient.EdenCommandError):
                self.eden.run_cmd("mount")
            verify.assert_not_called()

    def test_restart_skip_preserves_process_for_cleanup(self) -> None:
        process = mock.Mock()

        def spawn(**kwargs: object) -> None:
            self.eden._process = process

        with (
            mock.patch.object(self.eden, "shutdown"),
            mock.patch.object(self.eden, "spawn_nowait", side_effect=spawn),
            mock.patch.object(edenclient.util, "wait_for_daemon_healthy"),
            mock.patch.object(self.eden, "kill") as kill,
            mock.patch.object(
                self.eden,
                "assert_running_fuse_transports",
                side_effect=unittest.SkipTest("fallback"),
            ),
        ):
            with self.assertRaisesRegex(unittest.SkipTest, "fallback"):
                try:
                    self.eden.restart()
                finally:
                    self.eden.cleanup()
            self.assertIs(process, self.eden._process)
            kill.assert_called_once_with()

    def test_takeover_reaps_old_process_before_skip(self) -> None:
        old_process = mock.Mock()
        old_process.wait.return_value = 0
        new_process = mock.Mock()
        self.eden._process = old_process

        def start(**kwargs: object) -> None:
            self.eden._process = new_process

        with (
            mock.patch.object(self.eden, "get_pid_via_thrift", return_value=123),
            mock.patch.object(self.eden, "start", side_effect=start),
            mock.patch.object(
                self.eden,
                "assert_running_fuse_transports",
                side_effect=unittest.SkipTest("fallback"),
            ),
        ):
            with self.assertRaisesRegex(unittest.SkipTest, "fallback"):
                self.eden.graceful_restart()
            old_process.wait.assert_called_once_with()
            self.assertIs(new_process, self.eden._process)


@testcase.eden_repo_test
class EdenClientTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_client_dir_for_mount(self) -> None:
        clone_path = pathlib.Path(self.tmp_dir, "test_checkout")
        self.eden.clone(self.repo.path, str(clone_path))
        self.assertEqual(
            self.eden.client_dir_for_mount(clone_path),
            pathlib.Path(self.eden_dir, "clients", "test_checkout"),
        )
