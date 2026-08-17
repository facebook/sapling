#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from eden.fs.cli import configutil, daemon
from eden.fs.cli.config import EdenInstance


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
        instance.state_dir = Path("/tmp/eden-systemd-env-test")
        instance.get_config_bool.return_value = True
        daemon_environment = {"PATH": "/daemon/bin", "LD_PRELOAD": "/daemon/lib.so"}
        control_environment = {"PATH": "/control/bin", "CONTROL_ONLY": "1"}

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
        daemon_environment = {"MALLOC_CONF": "narenas:16"}
        control_environment = {"DBUS_SESSION_BUS_ADDRESS": "unix:path=/test/bus"}
        daemon_command = ["/usr/local/bin/edenfs"]
        completed = MagicMock(returncode=0, stderr="")

        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            with (
                patch.object(
                    daemon.daemon_util, "write_systemd_args_file"
                ) as write_args,
                patch.object(daemon, "_get_systemd_unit", return_value="edenfs@test"),
                patch.object(daemon.subprocess, "run", return_value=completed) as run,
            ):
                self.assertEqual(
                    daemon._systemctl_start_or_reload(
                        instance,
                        daemon_command,
                        daemon_environment,
                        control_environment,
                        False,
                    ),
                    0,
                )

        write_args.assert_called_once_with(
            instance.state_dir, daemon_command, daemon_environment
        )
        run.assert_called_once_with(
            ["systemctl", "--user", "start", "edenfs@test"],
            capture_output=True,
            text=True,
            env=control_environment,
        )
