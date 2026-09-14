#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import concurrent.futures
import os
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from typing import Callable, Dict, List, Optional, Tuple
from unittest.mock import MagicMock, patch

from eden.fs.cli import configutil, daemon, daemon_util, util
from eden.fs.cli.config import EdenInstance

# (binary, cmd, env, privhelper) -> (cmd, env), matching
# prepare_edenfs_privileges.
PreparePrivileges = Callable[
    [str, List[str], Dict[str, str], Optional[str]],
    Tuple[List[str], Dict[str, str]],
]


def _leave_privileges_alone(
    _binary: str, cmd: List[str], env: Dict[str, str], _privhelper: Optional[str]
) -> Tuple[List[str], Dict[str, str]]:
    return cmd, env


def _write_stale_args_file(state_dir: Path) -> Path:
    args_file = state_dir / daemon.daemon_util.DAEMON_ARGS_FILENAME
    args_file.write_text('{"cmd": ["/previous/edenfs"], "env": {}}')
    return args_file


class EdenFSEnvironmentTest(unittest.TestCase):
    def setUp(self) -> None:
        self.instance: MagicMock = MagicMock(spec=EdenInstance)

    def test_unconfigured_environment_preserves_existing_behavior(self) -> None:
        self.instance.get_config_strs.side_effect = lambda _key, default: default

        with patch.dict(
            os.environ,
            {"HOME": "/home/test", "UNRELATED_VARIABLE": "not preserved"},
            clear=True,
        ):
            environment = daemon.get_edenfs_environment(self.instance, None)

        self.assertEqual(environment["HOME"], "/home/test")
        self.assertNotIn("UNRELATED_VARIABLE", environment)
        self.assertNotIn("MALLOC_CONF", environment)
        self.assertNotIn("JE_MALLOC_CONF", environment)

    def test_configured_environment_is_applied(self) -> None:
        self.instance.get_config_strs.return_value = configutil.Strs(
            [
                "EMPTY=",
                "MALLOC_CONF=narenas:16,dirty_decay_ms:1000",
                "PATH=/configured/path",
                "VALUE_WITH_EQUALS=left=right",
            ]
        )

        with patch.dict(os.environ, {"PATH": "/inherited/path"}, clear=True):
            environment = daemon.get_edenfs_environment(self.instance, None)

        self.assertEqual(environment["EMPTY"], "")
        self.assertEqual(environment["MALLOC_CONF"], "narenas:16,dirty_decay_ms:1000")
        self.assertEqual(environment["PATH"], "/configured/path")
        self.assertEqual(environment["VALUE_WITH_EQUALS"], "left=right")
        self.instance.get_config_strs.assert_called_once_with(
            "daemon.environment", default=configutil.Strs([])
        )

    def test_later_configured_environment_entry_wins(self) -> None:
        self.instance.get_config_strs.return_value = configutil.Strs(
            ["MALLOC_CONF=narenas:16", "MALLOC_CONF=narenas:4"]
        )

        with patch.dict(os.environ, {}, clear=True):
            environment = daemon.get_edenfs_environment(self.instance, None)

        self.assertEqual(environment["MALLOC_CONF"], "narenas:4")

    def test_explicitly_preserved_environment_overrides_config(self) -> None:
        self.instance.get_config_strs.return_value = configutil.Strs(
            ["MALLOC_CONF=narenas:16"]
        )

        with patch.dict(os.environ, {"MALLOC_CONF": "narenas:4"}, clear=True):
            environment = daemon.get_edenfs_environment(self.instance, ["MALLOC_CONF"])

        self.assertEqual(environment["MALLOC_CONF"], "narenas:4")

    def test_invalid_configured_environment_is_skipped(self) -> None:
        invalid_entries = [
            ("MISSING_SEPARATOR", "expected NAME=value with a non-empty name"),
            ("=missing_name", "expected NAME=value with a non-empty name"),
            ("NULL_IN_NAME\0=value", "names and values must not contain NUL"),
            ("NULL_IN_VALUE=bad\0value", "names and values must not contain NUL"),
            ("-u=root", "name must contain only ASCII letters"),
            ("HAS-DASH=value", "name must contain only ASCII letters"),
            ("1STARTS_WITH_DIGIT=value", "name must contain only ASCII letters"),
        ]

        for entry, expected_error in invalid_entries:
            with self.subTest(entry=entry):
                self.instance.get_config_strs.return_value = configutil.Strs(
                    [entry, "VALID=value"]
                )
                with (
                    patch.dict(os.environ, {}, clear=True),
                    patch.object(daemon, "print_stderr") as print_stderr,
                ):
                    environment = daemon.get_edenfs_environment(self.instance, None)

                self.assertEqual(environment["VALID"], "value")
                print_stderr.assert_called_once()
                self.assertIn(expected_error, print_stderr.call_args.args[0])


# The code under test only runs on macOS, but nothing here needs a real one,
# and gating the class on darwin would leave it unexercised by CI. fcntl is
# what keeps it off Windows.
@unittest.skipIf(sys.platform == "win32", "restart sentinel needs fcntl")
class SigkillRestartSentinelTest(unittest.TestCase):
    def setUp(self) -> None:
        darwin = patch.object(daemon.sys, "platform", "darwin")
        darwin.start()
        self.addCleanup(darwin.stop)
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        self.config_dir = Path(temp_dir.name)
        # Named as EdenStateDir::getRestartSentinelPath() builds it, rather
        # than from the prefix the code under test uses, so that the two
        # drifting apart fails here.
        self.sentinel: Path = (
            self.config_dir / ".edenfs_restart_armed.1234.000000000badf00d"
        )

    def _sentinel_for(self, pid: int, token: str) -> Path:
        return self.config_dir / f".edenfs_restart_armed.{pid}.{token}"

    def _sigkill(self, pid: int, lock_contents: str | None) -> None:
        if lock_contents is not None:
            (self.config_dir / util.LOCK_FILE).write_text(lock_contents)
        with patch.object(daemon, "_send_sigkill"):
            daemon.sigkill_process(pid=pid, config_dir=self.config_dir, timeout=0)

    def test_sigkill_removes_the_restart_sentinel(self) -> None:
        self.sentinel.touch()

        self._sigkill(pid=1234, lock_contents="1234\n")

        self.assertFalse(self.sentinel.exists())

    def test_sigkill_removes_a_sentinel_no_daemon_owns(self) -> None:
        self.sentinel.touch()

        self._sigkill(pid=1234, lock_contents=None)

        self.assertFalse(self.sentinel.exists())

    def test_sigkill_keeps_a_sentinel_owned_by_another_daemon(self) -> None:
        self.sentinel.touch()

        self._sigkill(pid=1234, lock_contents="5678\n")

        self.assertTrue(self.sentinel.exists())

    def test_sigkill_keeps_a_sentinel_behind_an_unparseable_lock(self) -> None:
        self.sentinel.touch()

        self._sigkill(pid=1234, lock_contents="not a pid\n")

        self.assertTrue(self.sentinel.exists())

    def test_sigkill_tolerates_a_missing_sentinel(self) -> None:
        self._sigkill(pid=1234, lock_contents="1234\n")

        self.assertFalse(self.sentinel.exists())

    def test_sigkill_removes_every_sentinel_the_pid_armed(self) -> None:
        # A daemon that re-arms after a failed takeover keeps its pid and
        # draws a fresh token, so one generation can leave several behind.
        first = self._sentinel_for(1234, "0000000000000001")
        second = self._sentinel_for(1234, "0000000000000002")
        first.touch()
        second.touch()

        self._sigkill(pid=1234, lock_contents="1234\n")

        self.assertFalse(first.exists())
        self.assertFalse(second.exists())

    def test_sigkill_keeps_another_pids_sentinel(self) -> None:
        # 1234 must not match 12345: the pid is terminated by a separator.
        neighbour = self._sentinel_for(12345, "000000000badf00d")
        neighbour.touch()
        self.sentinel.touch()

        self._sigkill(pid=1234, lock_contents="1234\n")

        self.assertFalse(self.sentinel.exists())
        self.assertTrue(neighbour.exists())

    def test_sigkill_rechecks_owner_after_restart_lock_contention(self) -> None:
        self.sentinel.touch()
        owner_path = self.config_dir / util.LOCK_FILE
        owner_path.write_text("1234\n")
        restart_lock_path = self.config_dir / daemon_util.RESTART_SENTINEL_LOCK_NAME
        lock_attempted = threading.Event()
        real_flock = daemon.fcntl.flock

        def observe_flock(fd: int, operation: int) -> None:
            if operation & daemon.fcntl.LOCK_EX:
                lock_attempted.set()
            real_flock(fd, operation)

        with restart_lock_path.open("a") as held_lock:
            real_flock(held_lock.fileno(), daemon.fcntl.LOCK_EX)
            with (
                concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor,
                patch.object(daemon.fcntl, "flock", side_effect=observe_flock),
                patch.object(daemon, "_send_sigkill") as send_sigkill,
            ):
                future = executor.submit(
                    daemon.sigkill_process,
                    1234,
                    self.config_dir,
                    0,
                )
                attempted = lock_attempted.wait(timeout=5)
                sentinel_existed_while_blocked = self.sentinel.exists()
                kill_was_blocked = not send_sigkill.called
                owner_path.write_text("5678\n")
                real_flock(held_lock.fileno(), daemon.fcntl.LOCK_UN)
                future.result(timeout=5)

        self.assertTrue(attempted)
        self.assertTrue(sentinel_existed_while_blocked)
        self.assertTrue(kill_was_blocked)
        self.assertTrue(self.sentinel.exists())
        send_sigkill.assert_called_once_with(1234, None)

    def test_sigkill_holds_restart_lock_through_kill(self) -> None:
        self.sentinel.touch()
        (self.config_dir / util.LOCK_FILE).write_text("1234\n")
        restart_lock_path = self.config_dir / daemon_util.RESTART_SENTINEL_LOCK_NAME

        def check_lock(pid: int, instance: Optional[EdenInstance]) -> None:
            self.assertEqual(1234, pid)
            self.assertIsNone(instance)
            self.assertFalse(self.sentinel.exists())
            with restart_lock_path.open("a") as contender:
                with self.assertRaises(BlockingIOError):
                    daemon.fcntl.flock(
                        contender.fileno(), daemon.fcntl.LOCK_EX | daemon.fcntl.LOCK_NB
                    )

        with patch.object(daemon, "_send_sigkill", side_effect=check_lock):
            daemon.sigkill_process(pid=1234, config_dir=self.config_dir, timeout=0)

        with restart_lock_path.open("a") as contender:
            daemon.fcntl.flock(
                contender.fileno(), daemon.fcntl.LOCK_EX | daemon.fcntl.LOCK_NB
            )

    def test_sigkill_aborts_when_restart_lock_fails(self) -> None:
        self.sentinel.touch()
        (self.config_dir / util.LOCK_FILE).write_text("1234\n")

        with (
            patch.object(daemon.fcntl, "flock", side_effect=OSError("cannot lock")),
            patch.object(daemon, "_send_sigkill") as send_sigkill,
            self.assertRaisesRegex(daemon.ShutdownError, "Failed to acquire"),
        ):
            daemon.sigkill_process(pid=1234, config_dir=self.config_dir, timeout=0)

        self.assertTrue(self.sentinel.exists())
        send_sigkill.assert_not_called()


class EdenFSSystemdEnvironmentTest(unittest.TestCase):
    def test_systemd_run_sets_daemon_environment_explicitly(self) -> None:
        with patch.object(
            daemon, "_sanitize_unit_name", return_value="edenfs_test.scope"
        ):
            command = daemon._build_systemd_run_cmd(
                ["/usr/local/bin/edenfs", "--foreground"],
                "/tmp/eden-test",
                {"EMPTY": "", "VALUE": "left=right"},
            )

        self.assertEqual(
            command,
            [
                "systemd-run",
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--property=Delegate=yes",
                "--slice=edenfs",
                "--unit=edenfs_test.scope",
                "-E",
                "EMPTY=",
                "-E",
                "VALUE=left=right",
                "--",
                "/usr/local/bin/edenfs",
                "--foreground",
            ],
        )

    def test_systemd_run_uses_control_environment(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)
        instance.get_config_bool.return_value = True
        daemon_environment = {"PATH": "/daemon/bin", "LD_PRELOAD": "/daemon/lib.so"}
        control_environment = {"PATH": "/control/bin", "CONTROL_ONLY": "1"}
        state_dir = tempfile.TemporaryDirectory()
        self.addCleanup(state_dir.cleanup)
        instance.state_dir = Path(state_dir.name)

        with (
            patch.object(daemon.sys, "platform", "linux"),
            patch.dict(os.environ, control_environment, clear=True),
            patch.object(
                daemon.daemon_util,
                "find_daemon_binary",
                return_value="/usr/local/bin/edenfs",
            ),
            patch.object(
                daemon,
                "get_edenfs_cmd",
                return_value=(["/usr/local/bin/edenfs"], "/usr/local/bin/privhelper"),
            ),
            patch.object(
                daemon,
                "get_edenfs_environment",
                return_value=daemon_environment,
            ),
            patch.object(
                daemon,
                "prepare_edenfs_privileges",
                return_value=(["/usr/local/bin/edenfs"], daemon_environment),
            ),
            patch.object(
                daemon, "should_use_systemd_lifecycle_management", return_value=False
            ),
            patch.object(daemon, "_try_setup_systemd_env", return_value=True),
            patch.object(
                daemon, "_sanitize_unit_name", return_value="edenfs_test.scope"
            ),
            patch.object(daemon, "maybe_edensparse_migration"),
            patch.object(daemon, "_set_edenfs_slice_oomd_avoid"),
            patch.object(daemon.subprocess, "call", return_value=0) as call,
        ):
            self.assertEqual(daemon._start_edenfs_service(instance), 0)

        command = call.call_args.args[0]
        launch_environment = call.call_args.kwargs["env"]
        self.assertEqual(launch_environment, control_environment)
        self.assertNotIn("LD_PRELOAD", launch_environment)
        self.assertIn("LD_PRELOAD=/daemon/lib.so", command)

    def test_systemctl_uses_control_environment(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)
        control_environment = {"DBUS_SESSION_BUS_ADDRESS": "unix:path=/test/bus"}
        completed = MagicMock(returncode=0, stderr="")

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            with (
                patch.object(daemon, "_get_systemd_unit", return_value="edenfs@test"),
                patch.object(daemon.subprocess, "run", return_value=completed) as run,
            ):
                self.assertEqual(
                    daemon._systemctl_start_or_reload(
                        instance, control_environment, False
                    ),
                    0,
                )

        run.assert_called_once_with(
            ["systemctl", "--user", "start", "edenfs@test"],
            capture_output=True,
            text=True,
            env=control_environment,
        )

    def _start_edenfs_service(
        self,
        instance: MagicMock,
        prepare_privileges: PreparePrivileges = _leave_privileges_alone,
        platform: str = "darwin",
        write_args_error: Optional[OSError] = None,
        systemd: bool = False,
        privhelper_restart: bool = True,
    ) -> Tuple[int, MagicMock, MagicMock]:
        """Run _start_edenfs_service with its collaborators stubbed out.

        Returns its exit code, the `write_daemon_args_file` mock and the
        `subprocess.call` mock.
        """
        instance.get_config_bool.side_effect = (
            lambda key, default: privhelper_restart
            if key == "privhelper.restart-edenfs-on-crash"
            else default
        )
        with (
            patch.object(daemon.sys, "platform", platform),
            patch.object(
                daemon.daemon_util,
                "find_daemon_binary",
                return_value="/usr/local/bin/edenfs",
            ),
            patch.object(
                daemon,
                "get_edenfs_cmd",
                return_value=(
                    ["/usr/local/bin/edenfs", "--edenfs"],
                    "/usr/local/libexec/eden/edenfs_privhelper",
                ),
            ),
            patch.object(
                daemon,
                "get_edenfs_environment",
                return_value={"MALLOC_CONF": "narenas:16"},
            ),
            patch.object(
                daemon, "prepare_edenfs_privileges", side_effect=prepare_privileges
            ),
            patch.object(
                daemon,
                "should_use_systemd_lifecycle_management",
                return_value=systemd,
            ),
            patch.object(daemon, "_try_setup_systemd_env", return_value=systemd),
            patch.object(daemon, "_systemctl_start_or_reload", return_value=0),
            patch.object(daemon, "maybe_edensparse_migration"),
            patch.object(daemon.subprocess, "call", return_value=0) as call,
            patch.object(
                daemon.daemon_util,
                "write_daemon_args_file",
                side_effect=write_args_error,
            ) as write_args,
        ):
            exit_code = daemon._start_edenfs_service(instance, takeover=True)
        return exit_code, write_args, call

    def test_start_writes_the_args_file_without_systemd(self) -> None:
        # The daemon reads the args file back to arm the privhelper.
        instance: MagicMock = MagicMock(spec=EdenInstance)

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            exit_code, write_args, _call = self._start_edenfs_service(instance)

        self.assertEqual(exit_code, 0)
        write_args.assert_called_once_with(
            instance.state_dir,
            ["/usr/local/bin/edenfs", "--edenfs", "--takeover"],
            {"MALLOC_CONF": "narenas:16"},
            ["/usr/local/bin/edenfs", "--edenfs"],
        )

    def test_start_records_a_restart_command_without_sudo_or_takeover(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            _exit_code, write_args, _call = self._start_edenfs_service(
                instance,
                prepare_privileges=lambda _binary, cmd, env, _privhelper: (
                    ["/usr/bin/sudo", "MALLOC_CONF=narenas:16"] + cmd,
                    env,
                ),
            )

        # The restart command was snapshotted before sudo and --takeover were
        # added, because the privhelper replays it after dropping privileges.
        launch_cmd = write_args.call_args.args[1]
        self.assertEqual(launch_cmd[0], "/usr/bin/sudo")
        self.assertIn("--takeover", launch_cmd)
        self.assertEqual(
            write_args.call_args.args[3], ["/usr/local/bin/edenfs", "--edenfs"]
        )

    def test_start_skips_the_args_file_on_windows(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            _exit_code, write_args, _call = self._start_edenfs_service(
                instance, platform="win32"
            )

        write_args.assert_not_called()

    def test_start_survives_an_unwritable_args_file(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            with patch.object(daemon, "print_stderr") as print_stderr:
                exit_code, _write_args, call = self._start_edenfs_service(
                    instance, write_args_error=OSError("read-only file system")
                )

        self.assertEqual(exit_code, 0)
        call.assert_called_once()
        self.assertIn("read-only file system", print_stderr.call_args.args[0])

    def test_start_under_systemd_fails_on_an_unwritable_args_file(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            with patch.object(daemon, "print_stderr") as print_stderr:
                exit_code, _write_args, _call = self._start_edenfs_service(
                    instance,
                    platform="linux",
                    systemd=True,
                    write_args_error=OSError("read-only file system"),
                )

        self.assertEqual(exit_code, 1)
        self.assertIn("read-only file system", print_stderr.call_args.args[0])

    def test_start_removes_a_stale_args_file_when_the_write_fails(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            stale = _write_stale_args_file(instance.state_dir)
            with patch.object(daemon, "print_stderr") as print_stderr:
                self._start_edenfs_service(
                    instance, write_args_error=OSError("read-only file system")
                )
            self.assertFalse(stale.exists())

        self.assertIn("will not be auto-restarted", print_stderr.call_args.args[0])

    def test_start_warns_when_a_stale_args_file_survives(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)
        surviving: MagicMock = MagicMock(spec=Path)
        surviving.unlink.side_effect = OSError("read-only file system")
        surviving.exists.return_value = True

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            with (
                patch.object(daemon, "_daemon_args_file", return_value=surviving),
                patch.object(daemon, "print_stderr") as print_stderr,
            ):
                exit_code, _write_args, _call = self._start_edenfs_service(
                    instance, write_args_error=OSError("read-only file system")
                )

        self.assertEqual(exit_code, 0)
        surviving.unlink.assert_called_once_with(missing_ok=True)
        self.assertIn(
            "may be auto-restarted from an earlier start's command",
            print_stderr.call_args.args[0],
        )

    def test_start_skips_the_args_file_when_privhelper_restart_is_off(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            _exit_code, write_args, _call = self._start_edenfs_service(
                instance, privhelper_restart=False
            )

        write_args.assert_not_called()

    def test_start_under_systemd_ignores_the_privhelper_restart_config(self) -> None:
        # systemd is launched from the file, so its write is not the one the
        # restart-edenfs-on-crash knob gates.
        instance: MagicMock = MagicMock(spec=EdenInstance)

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            _exit_code, write_args, _call = self._start_edenfs_service(
                instance, platform="linux", systemd=True, privhelper_restart=False
            )

        write_args.assert_called_once()
