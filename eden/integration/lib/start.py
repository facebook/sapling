#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


import contextlib
import os
import pathlib
import subprocess
from typing import Generator, List

from .find_executables import FindExe


@contextlib.contextmanager
def run_eden_start_with_real_daemon(
    eden_dir: pathlib.Path,
    etc_eden_dir: pathlib.Path,
    home_dir: pathlib.Path,
    systemd: bool,
) -> Generator[None, None, None]:
    env = dict(os.environ)
    if systemd:
        env["EDEN_EXPERIMENTAL_SYSTEMD"] = "1"
    else:
        env.pop("EDEN_EXPERIMENTAL_SYSTEMD", None)
    eden_cli_args: List[str] = [
        FindExe.EDEN_CLI,
        "--config-dir",
        str(eden_dir),
        "--etc-eden-dir",
        str(etc_eden_dir),
        "--home-dir",
        str(home_dir),
    ]

    start_cmd: List[str] = eden_cli_args + [
        "start",
        "--daemon-binary",
        FindExe.EDEN_DAEMON,
    ]
    if eden_start_needs_allow_root_option(systemd=systemd):
        start_cmd.extend(["--", "--allowRoot"])
    subprocess.check_call(start_cmd, env=env)

    yield

    stop_cmd = eden_cli_args + ["stop"]
    subprocess.check_call(stop_cmd, env=env)


def eden_start_needs_allow_root_option(systemd: bool) -> bool:
    return not systemd and "SANDCASTLE" in os.environ
