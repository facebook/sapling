#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import pathlib
import subprocess
import sys
import typing
import unittest
from typing import List, Optional

import pexpect
from eden.cli.config import EdenInstance
from eden.cli.util import HealthStatus
from eden.test_support.environment_variable import EnvironmentVariableMixin
from fb303_core.ttypes import fb303_status

from .lib import testcase
from .lib.edenfs_systemd import EdenFSSystemdMixin
from .lib.fake_edenfs import get_fake_edenfs_argv
from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin, wait_for_pexpect_process
from .lib.service_test_case import (
    ServiceTestCaseBase,
    SystemdServiceTestCaseMarker,
    service_test,
)
from .lib.systemd import SystemdUserServiceManagerMixin


class StartTest(testcase.EdenTestCase):
    def test_start_if_necessary(self) -> None:
        # Confirm there are no checkouts configured, then stop edenfs
        checkouts = self.eden.list_cmd_simple()
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

        checkouts = self.eden.list_cmd_simple()
        self.assertEqual({checkout_dir: "RUNNING"}, checkouts)

        # Stop edenfs
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())
        # `eden start --if-necessary` should start edenfs now
        if eden_start_needs_allow_root_option(systemd=False):
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

    def test_start_if_not_running(self) -> None:
        # EdenFS is already running when the test starts, so
        # `eden start --if-not-running` should have nothing to do
        output = self.eden.run_cmd("start", "--if-not-running", "--", "--allowRoot")
        self.assertRegex(output, r"EdenFS is already running \(pid [0-9]+\)\n")
        self.assertTrue(self.eden.is_healthy())

        # `eden start` should fail without `--if-not-running`
        proc = self.eden.run_unchecked(
            "start", stdout=subprocess.PIPE, stderr=subprocess.PIPE, encoding="utf-8"
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertRegex(
            proc.stderr, r"error: EdenFS is already running \(pid [0-9]+\)\n"
        )
        self.assertEqual("", proc.stdout)

        # If we stop eden, `eden start --if-not-running` should start it
        self.eden.run_cmd("stop")
        self.assertFalse(self.eden.is_healthy())
        self.eden.run_cmd("start", "--if-not-running", "--", "--allowRoot")
        self.assertTrue(self.eden.is_healthy())


@testcase.eden_repo_test
class StartWithRepoTest(
    testcase.EdenRepoTest,
    EnvironmentVariableMixin,
    SystemdUserServiceManagerMixin,
    EdenFSSystemdMixin,
):
    """Test 'eden start' with a repo and checkout already configured.
    """

    def setUp(self) -> None:
        super().setUp()
        self.eden.shutdown()

    def test_eden_start_mounts_checkouts(self) -> None:
        self.run_eden_start(systemd=False)
        self.assert_checkout_is_mounted()

    def test_eden_start_with_systemd_mounts_checkouts(self) -> None:
        self.set_up_edenfs_systemd_service()
        self.run_eden_start(systemd=True)
        self.assert_checkout_is_mounted()

    def run_eden_start(self, systemd: bool) -> None:
        run_eden_start_with_real_daemon(
            eden_dir=pathlib.Path(self.eden_dir),
            etc_eden_dir=pathlib.Path(self.etc_eden_dir),
            home_dir=pathlib.Path(self.home_dir),
            systemd=systemd,
        )

    def assert_checkout_is_mounted(self) -> None:
        file = pathlib.Path(self.mount) / "hello"
        self.assertTrue(file.is_file())
        self.assertEqual(file.read_text(), "hola\n")

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def select_storage_engine(self) -> str:
        """ we need to persist data across restarts """
        return "rocksdb"


class DirectInvokeTest(unittest.TestCase):
    def test_no_args(self) -> None:
        """Directly invoking edenfs with no arguments should fail."""
        self._check_error([])

    def test_eden_cmd_arg(self) -> None:
        """Directly invoking edenfs with an eden command should fail."""
        self._check_error(["restart"])

    def _check_error(self, args: List[str], err: Optional[str] = None) -> None:
        cmd = [typing.cast(str, FindExe.EDEN_DAEMON)]  # T38947910
        cmd.extend(args)
        out = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(os.EX_USAGE, out.returncode)
        self.assertEqual(b"", out.stdout)

        if err is None:
            err = """\
error: the edenfs daemon should not normally be invoked manually
Did you mean to run "eden" instead of "edenfs"?
"""
        self.maxDiff = 5000
        self.assertMultiLineEqual(err, out.stderr.decode("utf-8", errors="replace"))


class StartFakeEdenFSTestBase(ServiceTestCaseBase, PexpectAssertionMixin):
    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = pathlib.Path(self.make_temporary_directory())

    def spawn_start(
        self,
        eden_dir: typing.Optional[pathlib.Path] = None,
        extra_args: typing.Optional[typing.Sequence[str]] = None,
    ) -> "pexpect.spawn[str]":
        if eden_dir is None:
            eden_dir = self.eden_dir
        args = (
            ["--config-dir", str(eden_dir)]
            + self.get_required_eden_cli_args()
            + [
                "start",
                "--daemon-binary",
                typing.cast(str, FindExe.FAKE_EDENFS),  # T38947910
            ]
        )
        if extra_args:
            args.extend(extra_args)
        return pexpect.spawn(
            # pyre-fixme[6]: Expected `str` for 1st param but got `() -> str`.
            FindExe.EDEN_CLI,
            args,
            encoding="utf-8",
            logfile=sys.stderr,
        )


@service_test
class StartFakeEdenFSTest(StartFakeEdenFSTestBase, PexpectAssertionMixin):
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
            fb303_status.ALIVE,
            f"First edenfs process should be healthy, but it isn't: "
            f"{instance_1_health}",
        )

        instance_2_health: HealthStatus = EdenInstance(
            str(eden_dir_2), etc_eden_dir=None, home_dir=None
        ).check_health()
        self.assertEqual(
            instance_2_health.status,
            fb303_status.ALIVE,
            f"Second edenfs process should be healthy, but it isn't: "
            f"{instance_2_health}",
        )

        self.assertNotEqual(
            instance_1_health.pid,
            instance_2_health.pid,
            f"The edenfs process should have separate process IDs",
        )

    def test_daemon_command_arguments_should_forward_to_edenfs(self) -> None:
        extra_daemon_args = ["--allowExtraArgs", "--", "hello world", "--ignoredOption"]
        start_process = self.spawn_start(extra_args=["--"] + extra_daemon_args)
        wait_for_pexpect_process(start_process)

        argv = get_fake_edenfs_argv(self.eden_dir)
        self.assertEqual(
            argv[-len(extra_daemon_args) :],
            extra_daemon_args,
            f"fake_edenfs should have received arguments verbatim\nargv: {argv}",
        )

    def test_daemon_command_arguments_should_forward_to_edenfs_without_leading_dashdash(
        self
    ) -> None:
        start_process = self.spawn_start(
            extra_args=[
                "hello world",
                "another fake_edenfs argument",
                "--",
                "--allowExtraArgs",
                "arg_after_dashdash",
            ]
        )
        self.assert_process_succeeds(start_process)

        expected_extra_daemon_args = [
            "hello world",
            "another fake_edenfs argument",
            "--allowExtraArgs",
            "arg_after_dashdash",
        ]
        argv = get_fake_edenfs_argv(self.eden_dir)
        self.assertEqual(
            argv[-len(expected_extra_daemon_args) :],
            expected_extra_daemon_args,
            f"fake_edenfs should have received extra arguments\nargv: {argv}",
        )

    def test_eden_start_resolves_explicit_config_dir_symlinks(self) -> None:
        # Test resolution of symlinks in the Eden state directectory when the
        # --config-dir argument is specified to the Eden CLI.
        link1 = self.tmp_dir / "link1"
        link2 = self.tmp_dir / "link2"
        link1.symlink_to(self.eden_dir, target_is_directory=True)
        link2.symlink_to(link1)
        self._test_eden_start_resolves_config_symlinks(link2, self.eden_dir)

    def test_eden_start_resolves_auto_config_dir_symlinks(self) -> None:
        # Test resolution of symlinks in the Eden state directectory if we don't specify
        # --config-dir and let the Eden CLI automatically figure out the location.
        # This is how Eden normally runs in practice most of the time.
        #
        # Set up symlinks in the home directory location normally used by Eden.
        home_local_dir = self.home_dir / "local"
        data_dir = self.tmp_dir / "data"
        data_dir.mkdir()
        home_local_dir.symlink_to(data_dir, target_is_directory=True)

        resolved_eden_dir = data_dir / ".eden"
        self._test_eden_start_resolves_config_symlinks(None, resolved_eden_dir)

    def _test_eden_start_resolves_config_symlinks(
        self, input_path: Optional[pathlib.Path], resolved_path: pathlib.Path
    ) -> None:
        # Test that the eden CLI resolves symlinks in the Eden state directory path.
        #
        # These must be resolved by the CLI and not the edenfs process: in some cases
        # where the symlinks are on an NFS mount point they can be resolved by the user
        # but not by root.  The edenfs privhelper process runs as root, so it may not be
        # able to resolve these symlinks.  Making sure the symlinks are fully resolved
        # by the CLI enables Eden to still work in these situations.
        if input_path is not None:
            config_dir_args = ["--config-dir", str(input_path)]
        else:
            config_dir_args = []
        args = (
            self.get_required_eden_cli_args()
            + config_dir_args
            + [
                "start",
                "--daemon-binary",
                typing.cast(str, FindExe.FAKE_EDENFS),  # T38947910
            ]
        )
        start_process: pexpect.spawn[str] = pexpect.spawn(
            # pyre-fixme[6]: Expected `str` for 1st param but got `() -> str`.
            FindExe.EDEN_CLI,
            args,
            encoding="utf-8",
            logfile=sys.stderr,
        )
        wait_for_pexpect_process(start_process)

        argv = get_fake_edenfs_argv(resolved_path)
        self.assert_eden_dir(argv, resolved_path)

    def assert_eden_dir(self, argv: List[str], expected: pathlib.Path) -> None:
        try:
            index = argv.index("--edenDir")
        except ValueError:
            self.fail(f"--edenDir not present in arguments: {argv}")
        actual_config_dir = argv[index + 1]
        self.assertEqual(str(expected), actual_config_dir, f"bad config dir: {argv}")

    def test_eden_start_fails_if_edenfs_is_already_running(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir) as daemon_pid:
            start_process = self.spawn_start()
            start_process.expect_exact(f"EdenFS is already running (pid {daemon_pid})")
            self.assert_process_fails(start_process)

    def test_eden_start_fails_if_edenfs_fails_during_startup(self) -> None:
        start_process = self.spawn_start(extra_args=["--", "--failDuringStartup"])
        start_process.expect_exact(
            "Started successfully, but reporting failure because "
            "--failDuringStartup was specified"
        )
        self.assert_process_fails(start_process, 1)


@service_test
class StartWithSystemdTest(StartFakeEdenFSTestBase, SystemdServiceTestCaseMarker):
    def test_eden_start_fails_if_service_is_running(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir):
            # Make fake_edenfs inaccessible and undetectable (without talking to
            # systemd), but keep the systemd service alive.
            (self.eden_dir / "lock").unlink()
            (self.eden_dir / "socket").unlink()
            health: HealthStatus = EdenInstance(
                str(self.eden_dir), etc_eden_dir=None, home_dir=None
            ).check_health()
            self.assertEqual(health.status, fb303_status.DEAD)
            service = self.get_edenfs_systemd_service(eden_dir=self.eden_dir)
            self.assertEqual(service.query_active_state(), "active")

            start_process = self.spawn_start()
            start_process.expect_exact(
                f"error: edenfs systemd service is already running"
            )
            # edenfsctl should show the output of 'systemctl status'.
            start_process.expect(r"\bfb-edenfs@.*?\.service\b")
            start_process.expect(r"Active:[^\n]*active \(running\)")
            self.assert_process_fails(start_process, 1)


def run_eden_start_with_real_daemon(
    eden_dir: pathlib.Path,
    etc_eden_dir: pathlib.Path,
    home_dir: pathlib.Path,
    systemd: bool,
) -> None:
    env = dict(os.environ)
    if systemd:
        env["EDEN_EXPERIMENTAL_SYSTEMD"] = "1"
    else:
        env.pop("EDEN_EXPERIMENTAL_SYSTEMD", None)
    command = [
        typing.cast(str, FindExe.EDEN_CLI),  # T38947910
        "--config-dir",
        str(eden_dir),
        "--etc-eden-dir",
        str(etc_eden_dir),
        "--home-dir",
        str(home_dir),
        "start",
        "--daemon-binary",
        typing.cast(str, FindExe.EDEN_DAEMON),  # T38947910
    ]
    if eden_start_needs_allow_root_option(systemd=systemd):
        command.extend(["--", "--allowRoot"])
    subprocess.check_call(command, env=env)


def eden_start_needs_allow_root_option(systemd: bool) -> bool:
    return not systemd and "SANDCASTLE" in os.environ
