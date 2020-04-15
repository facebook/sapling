#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess
from typing import Dict, List, NoReturn, Optional, Tuple

from . import daemon_util, proc_utils as proc_utils_mod
from .config import EdenInstance
from .util import ShutdownError, poll_until, print_stderr


# The amount of time to wait for the EdenFS process to exit after we send SIGKILL.
# We normally expect the process to be killed and reaped fairly quickly in this
# situation.  However, in rare cases on very heavily loaded systems it can take a while
# for init/systemd to wait on the process and for everything to be fully cleaned up.
# Therefore we wait up to 30 seconds by default.  (I've seen it take up to a couple
# minutes on systems with extremely high disk I/O load.)
#
# If this timeout does expire this can cause `edenfsctl restart` to fail after
# killing the old process but without starting the new process, which is
# generally undesirable if we can avoid it.
DEFAULT_SIGKILL_TIMEOUT = 30.0


def wait_for_process_exit(pid: int, timeout: float) -> bool:
    """Wait for the specified process ID to exit.

    Returns True if the process exits within the specified timeout, and False if the
    timeout expires while the process is still alive.
    """
    proc_utils = proc_utils_mod.new()

    def process_exited() -> Optional[bool]:
        if not proc_utils.is_process_alive(pid):
            return True
        return None

    try:
        poll_until(process_exited, timeout=timeout)
        return True
    except TimeoutError:
        return False


def wait_for_shutdown(
    pid: int, timeout: float, kill_timeout: float = DEFAULT_SIGKILL_TIMEOUT
) -> bool:
    """Wait for a process to exit.

    If it does not exit within `timeout` seconds kill it with SIGKILL.
    Returns True if the process exited on its own or False if it only exited
    after SIGKILL.

    Throws a ShutdownError if we failed to kill the process with SIGKILL
    (either because we failed to send the signal, or if the process still did
    not exit within kill_timeout seconds after sending SIGKILL).
    """
    # Wait until the process exits on its own.
    if wait_for_process_exit(pid, timeout):
        return True

    # client.shutdown() failed to terminate the process within the specified
    # timeout.  Take a more aggressive approach by sending SIGKILL.
    print_stderr(
        "error: sent shutdown request, but edenfs did not exit "
        "within {} seconds. Attempting SIGKILL.",
        timeout,
    )
    sigkill_process(pid, timeout=kill_timeout)
    return False


def sigkill_process(pid: int, timeout: float = DEFAULT_SIGKILL_TIMEOUT) -> None:
    """Send SIGKILL to a process, and wait for it to exit.

    If timeout is greater than 0, this waits for the process to exit after sending the
    signal.  Throws a ShutdownError exception if the process does not exit within the
    specified timeout.

    Returns successfully if the specified process did not exist in the first place.
    This is done to handle situations where the process exited on its own just before we
    could send SIGKILL.
    """
    proc_utils: proc_utils_mod.ProcUtils = proc_utils_mod.new()
    try:
        proc_utils.kill_process(pid)
    except PermissionError:
        raise ShutdownError(
            "Received a permissions when attempting to kill EdenFS. "
            "Perhaps EdenFS failed to drop root privileges properly?"
        )

    if timeout <= 0:
        return

    if not wait_for_process_exit(pid, timeout):
        raise ShutdownError(
            "edenfs process {} did not terminate within {} seconds of "
            "sending SIGKILL.".format(pid, timeout)
        )


def exec_daemon(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
    takeover: bool = False,
    gdb: bool = False,
    gdb_args: Optional[List[str]] = None,
    strace_file: Optional[str] = None,
    foreground: bool = False,
) -> NoReturn:
    """Execute the edenfs daemon.

    This method uses os.exec() to replace the current process with the edenfs daemon.
    It does not return on success.  It may throw an exception on error.
    """
    try:
        cmd, env = _get_daemon_args(
            instance=instance,
            daemon_binary=daemon_binary,
            edenfs_args=edenfs_args,
            takeover=takeover,
            gdb=gdb,
            gdb_args=gdb_args,
            strace_file=strace_file,
            foreground=foreground,
        )
    except daemon_util.DaemonBinaryNotFound as e:
        print_stderr(f"error: {e}")
        os._exit(1)

    os.execve(cmd[0], cmd, env)
    # Throw an exception just to let mypy know that we should never reach here
    # and will never return normally.
    raise Exception("execve should never return")


def start_daemon(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
    takeover: bool = False,
) -> int:
    """Start the edenfs daemon."""
    if instance.should_use_experimental_systemd_mode():
        from . import systemd_service

        return systemd_service.start_systemd_service(
            instance=instance, daemon_binary=daemon_binary, edenfs_args=edenfs_args
        )

    try:
        cmd, env = _get_daemon_args(
            instance=instance,
            daemon_binary=daemon_binary,
            edenfs_args=edenfs_args,
            takeover=takeover,
        )
    except daemon_util.DaemonBinaryNotFound as e:
        print_stderr(f"error: {e}")
        return 1

    return subprocess.call(cmd, env=env)


def _get_daemon_args(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
    takeover: bool = False,
    gdb: bool = False,
    gdb_args: Optional[List[str]] = None,
    strace_file: Optional[str] = None,
    foreground: bool = False,
) -> Tuple[List[str], Dict[str, str]]:
    """Get the command and environment to use to start edenfs."""
    daemon_binary = daemon_util.find_daemon_binary(daemon_binary)
    return instance.get_edenfs_start_cmd(
        daemon_binary,
        edenfs_args,
        takeover=takeover,
        gdb=gdb,
        gdb_args=gdb_args,
        strace_file=strace_file,
        foreground=foreground,
    )
