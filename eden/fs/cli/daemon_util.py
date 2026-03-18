#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional

from eden.fs.cli.util import is_apple_silicon, write_file_atomically


SYSTEMD_ARGS_FILENAME = ".edenfs_start_args"
SYSTEMD_STARTUP_LOG_FILENAME = ".edenfs_startup.log"


class DaemonBinaryNotFound(Exception):
    def __init__(self) -> None:
        super().__init__("unable to find edenfs executable")


def find_daemon_binary(explicit_daemon_binary: Optional[str]) -> str:
    if explicit_daemon_binary is not None:
        return explicit_daemon_binary

    try:
        return os.environ["EDENFS_SERVER_PATH"]
    except KeyError:
        pass

    daemon_binary = _find_default_daemon_binary()
    if daemon_binary is None:
        raise DaemonBinaryNotFound()
    return daemon_binary


def _find_default_daemon_binary() -> Optional[str]:
    # We search for the daemon executable relative to the edenfsctl CLI tool.
    cli_dir = os.path.dirname(os.path.abspath(sys.argv[0]))

    # Check the normal release installation location first
    if sys.platform != "win32":
        # On non-Windows platforms, the edenfs binary is installed under
        # <prefix>/libexec/eden/, while edenfsctl is in <prefix>/bin/
        suffix = ""
        candidate = os.path.normpath(os.path.join(cli_dir, "../libexec/eden/edenfs"))
    else:
        # On Windows, edenfs.exe is installed in the libexec sibling directory
        suffix = ".exe"
        candidate = os.path.normpath(os.path.join(cli_dir, "../libexec/edenfs.exe"))
    permissions = os.R_OK | os.X_OK
    if os.access(candidate, permissions):
        return candidate

    if is_apple_silicon():
        # This is where the binary will be found relative to this file when it is
        # run out of buck-out in debug mode for ARM64
        candidate = os.path.normpath(
            os.path.join(cli_dir, "../service/edenfs#macosx-arm64")
        )
        if os.access(candidate, permissions):
            return candidate

    # This is where the binary will be found relative to this file when it is
    # run out of buck-out in debug mode.
    candidate = os.path.normpath(
        os.path.join(cli_dir, "../../service/__edenfs__/edenfs")
    )
    if os.access(candidate, permissions):
        return candidate

    # This is where the binary will be found relative to this file when it is
    # run out of a CMake-based build
    candidate = os.path.normpath(os.path.join(cli_dir, "../edenfs" + suffix))
    if os.access(candidate, permissions):
        return candidate

    return None


def write_systemd_args_file(
    state_dir: Path, cmd: List[str], eden_env: Dict[str, str]
) -> Path:
    """Write the daemon command and environment to a JSON file.

    This file is read by the 'eden daemonctl' subcommand which is invoked
    by systemd's ExecStart/ExecReload.
    """
    args_file = state_dir / SYSTEMD_ARGS_FILENAME
    data = {"cmd": cmd, "env": eden_env}
    write_file_atomically(args_file, json.dumps(data).encode())
    return args_file


def start_daemon_from_args_file(args_file: str) -> int:
    """Read the daemon command and environment from an args file, then spawn the daemon.

    This is called by the `eden daemonctl` subcommand. When invoked via systemd
    (ExecStart/ExecReload), it inherits ``NOTIFY_SOCKET`` from the current environment
    so the daemon can report readiness.
    """
    try:
        with open(args_file) as f:
            data = json.load(f)
    except FileNotFoundError:
        print(
            f"error: args file {args_file} does not exist. "
            "Run 'eden start' to generate it.",
            file=sys.stderr,
        )
        return 1
    except json.JSONDecodeError as e:
        print(
            f"error: args file {args_file} contains invalid JSON: {e}. "
            "Run 'eden start' to regenerate it.",
            file=sys.stderr,
        )
        return 1

    try:
        cmd: List[str] = data["cmd"]
        eden_env: Dict[str, str] = data["env"]
    except KeyError as e:
        print(
            f"error: args file {args_file} is missing required key {e}. "
            "Run 'eden start' to regenerate it.",
            file=sys.stderr,
        )
        return 1

    # Inherit NOTIFY_SOCKET from the current environment. systemd sets this
    # for ExecStart/ExecReload processes in Type=notify services so the daemon
    # can signal readiness and report its main PID.
    notify_socket = os.environ.get("NOTIFY_SOCKET")
    if not notify_socket:
        print(
            "error: NOTIFY_SOCKET is not set. "
            "This command must be run by systemd as part of a Type=notify service.",
            file=sys.stderr,
        )
        return 1
    eden_env["NOTIFY_SOCKET"] = notify_socket

    # stdout/stderr are redirected by the systemd unit file
    # (StandardOutput=file:/%I/.edenfs_startup.log) so the CLI can read
    # the startup output after systemctl start returns.
    try:
        return subprocess.call(cmd, stdin=subprocess.DEVNULL, env=eden_env)
    except OSError as e:
        print(
            f"error: failed to execute {cmd[0]}: {e}",
            file=sys.stderr,
        )
        return 1
