#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import os
import pathlib
import signal
import subprocess
import typing

import eden.thrift

from .find_executables import FindExe


class FakeEdenFS(typing.ContextManager[int]):
    """A running fake_edenfs process."""

    @classmethod
    def spawn(
        cls,
        eden_dir: pathlib.Path,
        etc_eden_dir: pathlib.Path,
        home_dir: pathlib.Path,
        extra_arguments: typing.Optional[typing.Sequence[str]] = None,
    ) -> "FakeEdenFS":
        command = [
            typing.cast(str, FindExe.FAKE_EDENFS),  # T38947910
            "--configPath",
            str(home_dir / ".edenrc"),
            "--edenDir",
            str(eden_dir),
            "--etcEdenDir",
            str(etc_eden_dir),
            "--edenfs",
        ]
        if extra_arguments:
            command.extend(extra_arguments)
        subprocess.check_call(command)
        return cls.from_existing_process(eden_dir=eden_dir)

    @classmethod
    def spawn_via_cli(
        cls,
        eden_dir: pathlib.Path,
        etc_eden_dir: pathlib.Path,
        home_dir: pathlib.Path,
        extra_arguments: typing.Optional[typing.Sequence[str]] = None,
    ) -> "FakeEdenFS":
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
            typing.cast(str, FindExe.FAKE_EDENFS),  # T38947910
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


def get_fake_edenfs_argv(eden_dir: pathlib.Path) -> typing.List[str]:
    with eden.thrift.create_thrift_client(str(eden_dir)) as client:
        return client.getDaemonInfo().commandLine
