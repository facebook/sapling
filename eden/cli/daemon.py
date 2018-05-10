#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
import os
import signal
import sys
from typing import List, Optional

from . import config as config_mod
from .util import ShutdownError, poll_until, print_stderr


def wait_for_shutdown(
    config: config_mod.Config, pid: int, timeout: float, kill_timeout: float = 5.0
) -> bool:
    """
    Wait for a process to exit.

    If it does not exit within `timeout` seconds kill it with SIGKILL.
    Returns True if the process exited on its own or False if it only exited
    after SIGKILL.

    Throws a ShutdownError if we failed to kill the process with SIGKILL
    (either because we failed to send the signal, or if the process still did
    not exit within kill_timeout seconds after sending SIGKILL).
    """
    # Wait until the process exits on its own.
    def process_exited() -> Optional[bool]:
        try:
            os.kill(pid, 0)
        except OSError as ex:
            if ex.errno == errno.ESRCH:
                # The process has exited
                return True
            # EPERM is okay (and means the process is still running),
            # anything else is unexpected
            elif ex.errno != errno.EPERM:
                raise
        # Still running
        return None

    try:
        poll_until(process_exited, timeout=timeout)
        return True
    except TimeoutError:
        pass

    # client.shutdown() failed to terminate the process within the specified
    # timeout.  Take a more aggressive approach by sending SIGKILL.
    print_stderr(
        "error: sent shutdown request, but edenfs did not exit "
        "within {} seconds. Attempting SIGKILL.",
        timeout,
    )
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError as ex:
        if ex.errno == errno.ESRCH:
            # The process exited before the SIGKILL was received.
            # Treat this just like a normal shutdown since it exited on its
            # own.
            return True
        elif ex.errno == errno.EPERM:
            raise ShutdownError(
                "Received EPERM when sending SIGKILL. "
                "Perhaps edenfs failed to drop root privileges properly?"
            )
        else:
            raise

    try:
        poll_until(process_exited, timeout=kill_timeout)
        return False
    except TimeoutError:
        raise ShutdownError(
            "edenfs process {} did not terminate within {} seconds of "
            "sending SIGKILL.".format(pid, kill_timeout)
        )


def _ensure_dot_eden_folder_exists(config: config_mod.Config) -> None:
    """Creates the ~/.eden folder as specified by --config-dir/$EDEN_CONFIG_DIR.
    If the ~/.eden folder already exists, it will be left alone.
    """
    config.get_or_create_path_to_rocks_db()


def _find_default_daemon_binary() -> Optional[str]:
    # By default, we look for the daemon executable alongside this file.
    script_dir = os.path.dirname(os.path.abspath(sys.argv[0]))
    candidate = os.path.join(script_dir, "edenfs")
    permissions = os.R_OK | os.X_OK
    if os.access(candidate, permissions):
        return candidate

    # This is where the binary will be found relative to this file when it is
    # run out of buck-out in debug mode.
    candidate = os.path.normpath(os.path.join(script_dir, "../fs/service/edenfs"))
    if os.access(candidate, permissions):
        return candidate
    else:
        return None


def start_daemon(
    config: config_mod.Config,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
    takeover: bool = False,
    gdb: bool = False,
    gdb_args: Optional[List[str]] = None,
    strace_file: Optional[str] = None,
    foreground: bool = False,
    timeout: Optional[float] = None,
) -> int:
    # If this is the first time running the daemon, the ~/.eden directory
    # structure needs to be set up.
    # TODO(mbolin): Check whether the user is running as sudo/root. In general,
    # we want to avoid creating ~/.eden as root.
    _ensure_dot_eden_folder_exists(config)

    if daemon_binary is None:
        valid_daemon_binary = _find_default_daemon_binary()
        if valid_daemon_binary is None:
            print_stderr("error: unable to find edenfs executable")
            return 1
    else:
        valid_daemon_binary = daemon_binary

    # If the user put an "--" argument before the edenfs args, argparse passes
    # that through to us.  Strip it out.
    if edenfs_args and edenfs_args[0] == "--":
        edenfs_args = edenfs_args[1:]

    try:
        health_info = config.spawn(
            valid_daemon_binary,
            edenfs_args,
            takeover=takeover,
            gdb=gdb,
            gdb_args=gdb_args,
            strace_file=strace_file,
            foreground=foreground,
            timeout=timeout,
        )
    except config_mod.EdenStartError as ex:
        print_stderr("error: {}", ex)
        return 1
    print(
        "Started edenfs (pid {}). Logs available at {}".format(
            health_info.pid, config.get_log_path()
        )
    )
    return 0
