#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import importlib
import io
import sys
import types
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from typing import Any, Dict, List, Optional
from unittest import mock

from .. import file_handler_tools


class FakeAccessDenied(Exception):
    pass


class FakeNoSuchProcess(Exception):
    pass


HANDLE_OUTPUT: bytes = (
    "Hubbub.exe         pid: 24856  type: File            40: C:\\open\\fbsource\\a\n"
    "VS Code @ FB.exe   pid: 19044  type: File           34C: C:\\open\\fbsource\n"
    "dotnet.exe         pid: 30000  type: File            44: C:\\open\\fbsource\\b\n"
).encode()


class WinFileHandlerReleaserTest(unittest.TestCase):
    def setUp(self) -> None:
        self.killed: List[int] = []
        self.logged: List[Dict[str, Any]] = []
        test = self

        class FakeProcess:
            def __init__(self, pid: Optional[int] = None) -> None:
                self.pid: int = 1 if pid is None else pid

            def parents(self) -> List["FakeProcess"]:
                return []

            def kill(self) -> None:
                if self.pid == 24856:
                    raise FakeAccessDenied(f"access denied (pid={self.pid})")
                test.killed.append(self.pid)

            def wait(self) -> int:
                return 0

        fake_psutil = types.SimpleNamespace(
            Process=FakeProcess,
            NoSuchProcess=FakeNoSuchProcess,
            AccessDenied=FakeAccessDenied,
        )
        self.enterContext(mock.patch.dict(sys.modules, {"psutil": fake_psutil}))
        with mock.patch.object(sys, "platform", "win32"):
            self.module: types.ModuleType = importlib.reload(file_handler_tools)
        self.addCleanup(importlib.reload, file_handler_tools)
        self.enterContext(
            mock.patch.object(self.module, "prompt_confirmation", return_value=True)
        )
        self.enterContext(mock.patch("shutil.which", return_value="C:\\handle.exe"))
        self.enterContext(
            mock.patch("subprocess.check_output", return_value=HANDLE_OUTPUT)
        )

    def test_release_when_the_first_process_cannot_be_killed(self) -> None:
        instance = mock.MagicMock()
        instance.log_sample.side_effect = lambda event, **fields: self.logged.append(
            fields
        )
        releaser = self.module.WinFileHandlerReleaser(instance)

        with redirect_stdout(io.StringIO()):
            with self.assertRaises(AttributeError):
                releaser.try_release(Path("C:/open/fbsource"))

        self.assertEqual(self.killed, [])
        self.assertEqual(self.logged[-1]["unkillable_processes"], [])
        self.assertFalse(self.logged[-1]["success"])
