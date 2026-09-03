#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os
import tempfile
import unittest
from pathlib import Path
from typing import Any, Dict, List, Optional
from unittest.mock import patch

from eden.fs.cli.daemon_util import (
    DAEMON_ARGS_FILENAME,
    start_daemon_from_args_file,
    SystemdStartDaemonError,
    write_daemon_args_file,
)


class WriteDaemonArgsFileTest(unittest.TestCase):
    def _write(self, restart_cmd: Optional[List[str]] = None) -> Dict[str, Any]:
        with tempfile.TemporaryDirectory() as temp_dir:
            write_daemon_args_file(
                Path(temp_dir),
                ["/usr/bin/sudo", "/usr/local/bin/edenfs", "--takeover"],
                {"PATH": "/usr/bin"},
                restart_cmd,
            )
            return json.loads((Path(temp_dir) / DAEMON_ARGS_FILENAME).read_text())

    def test_records_the_restart_command_alongside_the_launch_command(self) -> None:
        data = self._write(["/usr/local/bin/edenfs"])

        # The launch command is stored verbatim: systemd replays it as-is.
        self.assertEqual(
            data["cmd"], ["/usr/bin/sudo", "/usr/local/bin/edenfs", "--takeover"]
        )
        self.assertEqual(data["restart_cmd"], ["/usr/local/bin/edenfs"])

    def test_omits_the_restart_command_when_there_is_none(self) -> None:
        self.assertNotIn("restart_cmd", self._write())


class StartDaemonFromArgsFileTest(unittest.TestCase):
    def _write_args_file(self, data: object) -> str:
        f = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False)
        json.dump(data, f)
        f.close()
        self.addCleanup(os.unlink, f.name)
        return f.name

    def test_file_not_found(self) -> None:
        with self.assertRaises(SystemdStartDaemonError):
            start_daemon_from_args_file("/nonexistent/path/args.json")

    def test_invalid_json(self) -> None:
        f = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False)
        f.write("not valid json{{{")
        f.close()
        self.addCleanup(os.unlink, f.name)
        with self.assertRaises(SystemdStartDaemonError):
            start_daemon_from_args_file(f.name)

    def test_missing_cmd_key(self) -> None:
        path = self._write_args_file({"env": {"FOO": "bar"}})
        with self.assertRaises(SystemdStartDaemonError):
            start_daemon_from_args_file(path)

    def test_missing_env_key(self) -> None:
        path = self._write_args_file({"cmd": ["/bin/true"]})
        with self.assertRaises(SystemdStartDaemonError):
            start_daemon_from_args_file(path)

    def test_missing_notify_socket(self) -> None:
        path = self._write_args_file({"cmd": ["/bin/true"], "env": {}})
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(SystemdStartDaemonError):
                start_daemon_from_args_file(path)

    @patch("eden.fs.cli.daemon_util.subprocess.call", return_value=0)
    def test_happy_path(self, mock_call: unittest.mock.MagicMock) -> None:
        cmd = ["/usr/bin/edenfs", "--configDir", "/tmp/eden"]
        env = {"PATH": "/usr/bin", "HOME": "/home/test"}
        path = self._write_args_file({"cmd": cmd, "env": env})

        with patch.dict(os.environ, {"NOTIFY_SOCKET": "/run/user/1000/notify"}):
            rc = start_daemon_from_args_file(path)

        self.assertEqual(rc, 0)
        mock_call.assert_called_once()
        call_args = mock_call.call_args
        self.assertEqual(call_args[0][0], cmd)
        passed_env = call_args[1]["env"]
        self.assertEqual(passed_env["NOTIFY_SOCKET"], "/run/user/1000/notify")
        self.assertEqual(passed_env["PATH"], "/usr/bin")

    @patch(
        "eden.fs.cli.daemon_util.subprocess.call",
        side_effect=FileNotFoundError("No such file or directory: '/usr/bin/edenfs'"),
    )
    def test_binary_not_found(self, mock_call: unittest.mock.MagicMock) -> None:
        path = self._write_args_file(
            {"cmd": ["/usr/bin/edenfs", "--configDir", "/tmp"], "env": {}}
        )
        with patch.dict(os.environ, {"NOTIFY_SOCKET": "/run/user/1000/notify"}):
            with self.assertRaises(SystemdStartDaemonError):
                start_daemon_from_args_file(path)
