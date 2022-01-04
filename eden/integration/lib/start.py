#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
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
    edenfsctl, edenfsctl_env = FindExe.get_edenfsctl_env()
    env = dict(os.environ)
    env.update(edenfsctl_env)
    if systemd:
        env["EDEN_EXPERIMENTAL_SYSTEMD"] = "1"
    else:
        env.pop("EDEN_EXPERIMENTAL_SYSTEMD", None)
    eden_cli_args: List[str] = [
        edenfsctl,
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

    extra_daemon_args = []

    privhelper = FindExe.EDEN_PRIVHELPER
    if privhelper is not None:
        extra_daemon_args.extend(["--privhelper_path", privhelper])

    if eden_start_needs_allow_root_option(systemd=systemd):
        extra_daemon_args.append("--allowRoot")

    if extra_daemon_args:
        start_cmd.append("--")
        start_cmd.extend(extra_daemon_args)

    subprocess.check_call(start_cmd, env=env)

    yield

    stop_cmd = eden_cli_args + ["stop"]
    subprocess.check_call(stop_cmd, env=env)


def eden_start_needs_allow_root_option(systemd: bool) -> bool:
    return not systemd and "SANDCASTLE" in os.environ
