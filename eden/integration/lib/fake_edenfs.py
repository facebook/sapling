#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import contextlib
import os
import pathlib
import signal
import subprocess
import typing

from .find_executables import FindExe


class FakeEdenFS(typing.ContextManager[int]):
    """A running fake_edenfs process."""

    @classmethod
    def spawn(
        cls, eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
    ) -> "FakeEdenFS":
        command = [FindExe.FAKE_EDENFS, "--edenDir", str(eden_dir), "--edenfs"]
        command.extend(extra_arguments)
        subprocess.check_call(command)
        return cls.from_existing_process(eden_dir=eden_dir)

    @classmethod
    def spawn_via_cli(
        cls, eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
    ) -> "FakeEdenFS":
        command = [
            FindExe.EDEN_CLI,
            "--config-dir",
            str(eden_dir),
            "start",
            "--daemon-binary",
            FindExe.FAKE_EDENFS,
        ]
        if extra_arguments:
            command.append("--")
            command.extend(extra_arguments)
        subprocess.check_call(command)
        return cls.from_existing_process(eden_dir=eden_dir)

    @staticmethod
    def from_existing_process(eden_dir: pathlib.Path) -> "FakeEdenFS":
        edenfs_pid = int((eden_dir / "lock").read_text())
        return FakeEdenFS(process_id=edenfs_pid)

    def __init__(self, process_id: int) -> None:
        super().__init__()
        self.process_id = process_id

    def __enter__(self) -> int:
        return self.process_id

    def __exit__(self, exc_type, exc_val, exc_tb):
        with contextlib.suppress(ProcessLookupError):
            os.kill(self.process_id, signal.SIGTERM)
        return None


def read_fake_edenfs_argv_file(argv_file: pathlib.Path) -> typing.List[str]:
    try:
        return list(argv_file.read_text().splitlines())
    except FileNotFoundError as e:
        raise Exception(
            "fake_edenfs should have recognized the --commandArgumentsLogFile argument"
        ) from e
