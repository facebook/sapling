#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os
import tempfile
import unittest
from unittest.mock import patch

from eden.fs.cli.daemon_util import start_daemon_from_args_file


class StartDaemonFromArgsFileTest(unittest.TestCase):
    def _write_args_file(self, data: object) -> str:
        f = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False)
        json.dump(data, f)
        f.close()
        self.addCleanup(os.unlink, f.name)
        return f.name

    def test_file_not_found(self) -> None:
        rc = start_daemon_from_args_file("/nonexistent/path/args.json")
        self.assertEqual(rc, 1)

    def test_invalid_json(self) -> None:
        f = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False)
        f.write("not valid json{{{")
        f.close()
        self.addCleanup(os.unlink, f.name)
        rc = start_daemon_from_args_file(f.name)
        self.assertEqual(rc, 1)

    def test_missing_cmd_key(self) -> None:
        path = self._write_args_file({"env": {"FOO": "bar"}})
        rc = start_daemon_from_args_file(path)
        self.assertEqual(rc, 1)

    def test_missing_env_key(self) -> None:
        path = self._write_args_file({"cmd": ["/bin/true"]})
        rc = start_daemon_from_args_file(path)
        self.assertEqual(rc, 1)

    def test_missing_notify_socket(self) -> None:
        path = self._write_args_file({"cmd": ["/bin/true"], "env": {}})
        with patch.dict(os.environ, {}, clear=True):
            rc = start_daemon_from_args_file(path)
        self.assertEqual(rc, 1)

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
            rc = start_daemon_from_args_file(path)
        self.assertEqual(rc, 1)
