#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import sys
from typing import Optional

from eden.fs.cli.util import is_apple_silicon


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
    candidate = os.path.normpath(os.path.join(cli_dir, "../service/edenfs"))
    if os.access(candidate, permissions):
        return candidate

    # This is where the binary will be found relative to this file when it is
    # run out of a CMake-based build
    candidate = os.path.normpath(os.path.join(cli_dir, "../edenfs" + suffix))
    if os.access(candidate, permissions):
        return candidate

    return None
