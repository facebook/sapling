#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import shlex
import sys
from typing import Any, Optional, Union

import pexpect


if sys.platform == "win32":
    import pexpect.popen_spawn

    PexpectSpawnType = pexpect.popen_spawn.PopenSpawn

    pexpect_spawn = pexpect.popen_spawn.PopenSpawn
else:
    import pexpect.pty_spawn

    PexpectSpawnType = pexpect.pty_spawn.spawn

    pexpect_spawn = pexpect.pty_spawn.spawn


class PexpectAssertionMixin(metaclass=abc.ABCMeta):
    def assert_process_succeeds(self, process: PexpectSpawnType) -> None:
        actual_exit_code = wait_for_pexpect_process(process)
        self.assertEqual(
            actual_exit_code,
            0,
            f"Command should return success: {pexpect_process_shell_command(process)}",
        )

    def assert_process_fails(
        self, process: PexpectSpawnType, exit_code: Optional[int] = None
    ) -> None:
        if exit_code is None:
            actual_exit_code = wait_for_pexpect_process(process)
            self.assertNotEqual(
                actual_exit_code,
                0,
                f"Command should return an error code: "
                f"{pexpect_process_shell_command(process)}",
            )
        else:
            self.assert_process_exit_code(process, exit_code)

    def assert_process_exit_code(
        self, process: PexpectSpawnType, exit_code: int
    ) -> None:
        actual_exit_code = wait_for_pexpect_process(process)
        self.assertEqual(
            actual_exit_code,
            exit_code,
            f"Command should exit with code {exit_code}: "
            f"{pexpect_process_shell_command(process)}",
        )

    @abc.abstractmethod
    def assertEqual(self, first: Any, second: Any, msg: Any = ...) -> None:
        raise NotImplementedError()

    @abc.abstractmethod
    def assertNotEqual(self, first: Any, second: Any, msg: Any = ...) -> None:
        raise NotImplementedError()


def pexpect_process_shell_command(process: PexpectSpawnType) -> str:
    def str_from_strlike(s: Union[bytes, str]) -> str:
        if isinstance(s, str):
            return s
        else:
            return s.decode("utf-8")

    command = process.command
    args = process.args
    if command is None:
        return "<no pexpect command set>"
    else:
        assert args is not None
        command_parts = [command] + [str_from_strlike(arg) for arg in args]
        return " ".join(map(shlex.quote, command_parts))


def wait_for_pexpect_process(process: PexpectSpawnType) -> int:
    process.expect_exact(pexpect.EOF)
    return process.wait()
