#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import os
import pathlib
import subprocess
import sys
import unittest
from typing import Generator, List, Optional, Sequence

from eden.fs.cli.config import EdenInstance
from eden.fs.cli.util import HealthStatus
from fb303_core.ttypes import fb303_status

from .lib import testcase
from .lib.fake_edenfs import get_fake_edenfs_argv
from .lib.find_executables import FindExe
from .lib.service_test_case import (
    ServiceTestCaseBase,
    SystemdServiceTest,
    service_test,
    systemd_test,
)


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
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())
        self.eden.run_cmd("start", "--if-not-running", "--", "--allowRoot")
        self.assertTrue(self.eden.is_healthy())

        # Stop edenfs.  We didn't start it through self.eden.start()
        # so the self.eden class doesn't really know it is running and that
        # it needs to be shut down.
        self.eden.run_cmd("stop")


@testcase.eden_repo_test
class StartWithRepoTest(testcase.EdenRepoTest):
    """Test 'eden start' with a repo and checkout already configured.
    """

    def test_eden_start_mounts_checkouts(self) -> None:
        self.eden.shutdown()

        with run_eden_start_with_real_daemon(
            eden_dir=pathlib.Path(self.eden_dir),
            etc_eden_dir=pathlib.Path(self.etc_eden_dir),
            home_dir=pathlib.Path(self.home_dir),
            systemd=False,
        ):
            self.assert_checkout_is_mounted()

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


class DirectInvokeTest(testcase.IntegrationTestCase):
    def test_eden_cmd_arg(self) -> None:
        """Directly invoking edenfs with an edenfsctl subcommand should fail."""
        cmd: List[str] = [FindExe.EDEN_DAEMON, "restart"]  # pyre-ignore[9]: T38947910
        out = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(os.EX_USAGE, out.returncode)
        self.assertEqual(b"", out.stdout)

        expected_err = "error: unexpected trailing command line arguments\n"
        self.maxDiff = 5000
        self.assertMultiLineEqual(
            expected_err, out.stderr.decode("utf-8", errors="replace")
        )


class StartFakeEdenFSTestBase(ServiceTestCaseBase):
    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = self.make_temp_dir("eden")

    def _get_base_eden_args(self, eden_dir: Optional[pathlib.Path] = None) -> List[str]:
        if eden_dir is None:
            eden_dir = self.eden_dir
        return [
            FindExe.EDEN_CLI,
            "--config-dir",
            str(eden_dir),
        ] + self.get_required_eden_cli_args()

    def expect_start_failure(
        self,
        msg: str,
        eden_dir: Optional[pathlib.Path] = None,
        extra_args: Optional[Sequence[str]] = None,
    ) -> subprocess.CompletedProcess:
        start_cmd = self._get_base_eden_args(eden_dir) + [  # pyre-ignore[6]: T38947910
            "start",
            "--daemon-binary",
            FindExe.FAKE_EDENFS,
        ]
        if extra_args:
            start_cmd.extend(extra_args)
        proc = subprocess.run(
            start_cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            encoding="utf-8",
            errors="replace",
        )

        # Pass through the command output, to make the test easier to debug
        sys.stdout.write(proc.stdout)
        sys.stderr.write(proc.stderr)

        self.assertNotEqual(proc.returncode, 0)
        self.assertIn(msg, proc.stderr)
        return proc

    def start_edenfs(
        self,
        eden_dir: Optional[pathlib.Path] = None,
        extra_args: Optional[Sequence[str]] = None,
    ) -> None:
        base_args = self._get_base_eden_args(eden_dir)
        start_cmd = base_args + [  # pyre-ignore[6]: T38947910
            "start",
            "--daemon-binary",
            FindExe.FAKE_EDENFS,
        ]
        if extra_args:
            start_cmd.extend(extra_args)
        subprocess.check_call(start_cmd)

        def cleanup():
            stop_cmd = base_args + ["stop"]
            subprocess.call(stop_cmd)

        self.exit_stack.callback(cleanup)


@service_test
class StartFakeEdenFSTest(StartFakeEdenFSTestBase):
    def test_eden_start_launches_separate_processes_for_separate_eden_dirs(
        self
    ) -> None:
        eden_dir_1 = self.eden_dir
        eden_dir_2 = self.make_temp_dir("eden2")

        self.start_edenfs(eden_dir=eden_dir_1)
        self.start_edenfs(eden_dir=eden_dir_2)

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
        self.start_edenfs(extra_args=["--"] + extra_daemon_args)

        argv = get_fake_edenfs_argv(self.eden_dir)
        self.assertEqual(
            argv[-len(extra_daemon_args) :],
            extra_daemon_args,
            f"fake_edenfs should have received arguments verbatim\nargv: {argv}",
        )

    def test_daemon_command_arguments_should_forward_to_edenfs_without_leading_dashdash(
        self
    ) -> None:
        self.start_edenfs(
            extra_args=[
                "hello world",
                "another fake_edenfs argument",
                "--",
                "--allowExtraArgs",
                "arg_after_dashdash",
            ]
        )

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
        eden_cli: str = FindExe.EDEN_CLI  # pyre-ignore[9]: T38947910
        fake_edenfs: str = FindExe.FAKE_EDENFS  # pyre-ignore[9]: T38947910
        base_args = [eden_cli] + self.get_required_eden_cli_args() + config_dir_args
        start_cmd = base_args + ["start", "--daemon-binary", fake_edenfs]
        stop_cmd = base_args + ["stop"]
        subprocess.check_call(start_cmd)
        try:
            argv = get_fake_edenfs_argv(resolved_path)
            self.assert_eden_dir(argv, resolved_path)
        finally:
            subprocess.call(stop_cmd)

    def assert_eden_dir(self, argv: List[str], expected: pathlib.Path) -> None:
        try:
            index = argv.index("--edenDir")
        except ValueError:
            self.fail(f"--edenDir not present in arguments: {argv}")
        actual_config_dir = argv[index + 1]
        self.assertEqual(str(expected), actual_config_dir, f"bad config dir: {argv}")

    def test_eden_start_fails_if_edenfs_is_already_running(self) -> None:
        with self.spawn_fake_edenfs(self.eden_dir) as daemon_pid:
            self.expect_start_failure(f"EdenFS is already running (pid {daemon_pid})")

    def test_eden_start_fails_if_edenfs_fails_during_startup(self) -> None:
        self.expect_start_failure(
            "Started successfully, but reporting failure because "
            "--failDuringStartup was specified",
            extra_args=["--", "--failDuringStartup"],
        )


@systemd_test
class StartWithSystemdTest(SystemdServiceTest, StartFakeEdenFSTestBase):
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

            proc = self.expect_start_failure(
                f"error: edenfs systemd service is already running"
            )
            # edenfsctl should show the output of 'systemctl status'.
            self.assertRegex(proc.stdout, r"\bfb-edenfs@.*?\.service\b")
            self.assertRegex(proc.stdout, r"Active:[^\n]*active \(running\)")


@contextlib.contextmanager
def run_eden_start_with_real_daemon(
    eden_dir: pathlib.Path,
    etc_eden_dir: pathlib.Path,
    home_dir: pathlib.Path,
    systemd: bool,
) -> Generator[None, None, None]:
    env = dict(os.environ)
    if systemd:
        env["EDEN_EXPERIMENTAL_SYSTEMD"] = "1"
    else:
        env.pop("EDEN_EXPERIMENTAL_SYSTEMD", None)
    eden_cli_args: List[str] = [  # pyre-ignore[9]: T38947910
        FindExe.EDEN_CLI,
        "--config-dir",
        str(eden_dir),
        "--etc-eden-dir",
        str(etc_eden_dir),
        "--home-dir",
        str(home_dir),
    ]

    start_cmd: List[str] = eden_cli_args + [  # pyre-ignore[6]: T38947910
        "start",
        "--daemon-binary",
        FindExe.EDEN_DAEMON,
    ]
    if eden_start_needs_allow_root_option(systemd=systemd):
        start_cmd.extend(["--", "--allowRoot"])
    subprocess.check_call(start_cmd, env=env)

    yield

    stop_cmd = eden_cli_args + ["stop"]
    subprocess.check_call(stop_cmd, env=env)


def eden_start_needs_allow_root_option(systemd: bool) -> bool:
    return not systemd and "SANDCASTLE" in os.environ
