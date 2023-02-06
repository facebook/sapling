#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import os
import pathlib
import signal
import subprocess
import typing

from eden.thrift.legacy import create_thrift_client

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
        command: typing.List[str] = [
            FindExe.FAKE_EDENFS,
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
        edenfsctl, env = FindExe.get_edenfsctl_env()
        command: typing.List[str] = [
            edenfsctl,
            "--config-dir",
            str(eden_dir),
            "--etc-eden-dir",
            str(etc_eden_dir),
            "--home-dir",
            str(home_dir),
            "start",
            "--daemon-binary",
            FindExe.FAKE_EDENFS,
        ]
        if extra_arguments:
            command.append("--")
            command.extend(extra_arguments)
        subprocess.check_call(command, env=env)
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

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        with contextlib.suppress(ProcessLookupError):
            os.kill(self.process_id, signal.SIGTERM)
        return None


def get_fake_edenfs_argv(eden_dir: pathlib.Path) -> typing.List[str]:
    with create_thrift_client(str(eden_dir)) as client:
        argv = client.getDaemonInfo().commandLine
        # StartupLogger may add `--startupLoggerFd 5` as a parameter.
        # The 5 is a file descriptor number and has no guarantees as
        # to which number is selected by the kernel.
        # We perform various test assertions on these arguments.
        # To make those easier, we rewrite the fd number to always be 5
        if "--startupLoggerFd" in argv:
            argv[argv.index("--startupLoggerFd") + 1] = "5"
        return argv
