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
import pathlib
import signal
import subprocess
import sys
import time
from typing import Dict, List, NoReturn, Optional, Tuple, Union

from .config import EdenInstance
from .logfile import forward_log_file
from .systemd import EdenFSSystemdServiceConfig, edenfs_systemd_service_name
from .util import ShutdownError, poll_until, print_stderr


def wait_for_shutdown(pid: int, timeout: float, kill_timeout: float = 5.0) -> bool:
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
        if did_process_exit(pid):
            return True
        else:
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
        # pyre-ignore[6]: T38216313
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


def is_zombie_process(pid: int) -> bool:
    try:
        with open(f"/proc/{pid}/stat", "rb") as proc_stat:
            line = proc_stat.read()
            pieces = line.split()
            if len(pieces) > 2 and pieces[2] == b"Z":
                return True
    except FileNotFoundError:
        pass

    return False


def did_process_exit(pid: int) -> bool:
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
    if is_zombie_process(pid):
        return True
    # Still running
    return False


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


class DaemonBinaryNotFound(Exception):
    def __init__(self) -> None:
        super().__init__("unable to find edenfs executable")


def _find_daemon_binary(explicit_daemon_binary: Optional[str]) -> str:
    if explicit_daemon_binary is not None:
        return explicit_daemon_binary
    daemon_binary = _find_default_daemon_binary()
    if daemon_binary is None:
        raise DaemonBinaryNotFound()
    return daemon_binary


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
    except DaemonBinaryNotFound as e:
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
) -> int:
    """Start the edenfs daemon."""
    try:
        cmd, env = _get_daemon_args(
            instance=instance, daemon_binary=daemon_binary, edenfs_args=edenfs_args
        )
    except DaemonBinaryNotFound as e:
        print_stderr(f"error: {e}")
        return 1

    return subprocess.call(cmd, env=env)


def start_systemd_service(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
) -> int:
    try:
        daemon_binary = _find_daemon_binary(daemon_binary)
    except DaemonBinaryNotFound as e:
        print_stderr(f"error: {e}")
        return 1

    service_config = EdenFSSystemdServiceConfig(
        eden_dir=instance.state_dir,
        edenfs_executable_path=pathlib.Path(daemon_binary),
        extra_edenfs_arguments=edenfs_args or [],
    )
    service_config.write_config_file()
    service_name = edenfs_systemd_service_name(instance.state_dir)

    startup_log_path = service_config.startup_log_file_path
    startup_log_path.write_bytes(b"")
    with forward_log_file(  # pyre-ignore (T37455202)
        startup_log_path, sys.stderr.buffer
    ) as log_forwarder:
        with subprocess.Popen(
            ["systemctl", "--user", "start", "--", service_name]
        ) as start_process:
            while True:
                log_forwarder.poll()
                exit_code = start_process.poll()
                if exit_code is not None:
                    log_forwarder.poll()
                    return exit_code
                time.sleep(0.1)


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
    daemon_binary = _find_daemon_binary(daemon_binary)
    return instance.get_edenfs_start_cmd(
        daemon_binary,
        edenfs_args,
        takeover=takeover,
        gdb=gdb,
        gdb_args=gdb_args,
        strace_file=strace_file,
        foreground=foreground,
    )
