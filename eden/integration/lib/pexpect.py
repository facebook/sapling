#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import shlex
from typing import Any, Union

import pexpect


class PexpectAssertionMixin(metaclass=abc.ABCMeta):
    def assert_process_succeeds(self, process: pexpect.spawn):
        actual_exit_code = wait_for_pexpect_process(process)
        self.assertEqual(
            actual_exit_code,
            0,
            f"Command should return success: {pexpect_process_shell_command(process)}",
        )

    def assert_process_fails(self, process: pexpect.spawn, exit_code: int):
        assert exit_code != 0
        actual_exit_code = wait_for_pexpect_process(process)
        self.assertEqual(
            actual_exit_code,
            exit_code,
            f"Command should return an error code: "
            f"{pexpect_process_shell_command(process)}",
        )

    def assert_process_exit_code(self, process: pexpect.spawn, exit_code: int):
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


def pexpect_process_shell_command(process: pexpect.spawn) -> str:
    def str_from_strlike(s: Union[bytes, str]) -> str:
        if isinstance(s, str):
            return s
        else:
            return s.decode("utf-8")

    command_parts = [process.command] + [str_from_strlike(arg) for arg in process.args]
    return " ".join(map(shlex.quote, command_parts))


def wait_for_pexpect_process(process: pexpect.spawn) -> int:
    process.expect_exact(pexpect.EOF)
    return process.wait()
