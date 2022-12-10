#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import stat
import subprocess
import sys
from typing import Dict, List, Optional, Tuple

from . import daemon_util, proc_utils as proc_utils_mod
from .config import EdenInstance
from .util import is_apple_silicon, poll_until, print_stderr, ShutdownError


# The amount of time to wait for the edenfs process to exit after we send SIGKILL.
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
    proc_utils: proc_utils_mod.ProcUtils = proc_utils_mod.new()

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
    except PermissionError as ex:
        raise ShutdownError(
            f"Received a permission error when attempting to kill edenfs: {ex}"
        )

    if timeout <= 0:
        return

    if not wait_for_process_exit(pid, timeout):
        raise ShutdownError(
            "edenfs process {} did not terminate within {} seconds of "
            "sending SIGKILL.".format(pid, timeout)
        )


def start_edenfs_service(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
) -> int:
    """Start the edenfs daemon."""
    return _start_edenfs_service(
        instance=instance,
        daemon_binary=daemon_binary,
        edenfs_args=edenfs_args,
        takeover=False,
    )


def gracefully_restart_edenfs_service(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
) -> int:
    """Gracefully restart the EdenFS service"""
    return _start_edenfs_service(
        instance=instance,
        daemon_binary=daemon_binary,
        edenfs_args=edenfs_args,
        takeover=True,
    )


def _start_edenfs_service(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
    takeover: bool = False,
) -> int:
    """Get the command and environment to use to start edenfs."""
    daemon_binary = daemon_util.find_daemon_binary(daemon_binary)
    cmd, privhelper = get_edenfs_cmd(instance, daemon_binary)

    if takeover:
        cmd.append("--takeover")
    if edenfs_args:
        cmd.extend(edenfs_args)

    eden_env = get_edenfs_environment()

    # Wrap the command in sudo, if necessary. See help text in
    # prepare_edenfs_privileges for more info.
    cmd, eden_env = prepare_edenfs_privileges(daemon_binary, cmd, eden_env, privhelper)

    creation_flags = 0

    return subprocess.call(
        cmd, stdin=subprocess.DEVNULL, env=eden_env, creationflags=creation_flags
    )


def get_edenfsctl_cmd() -> str:
    env = os.environ.get("EDENFS_CLI_PATH", None)
    if env:
        return env

    edenfsctl_real = os.path.abspath(sys.argv[0])
    if sys.platform == "win32":
        edenfsctl = os.path.join(edenfsctl_real, "../edenfsctl.exe")
    else:
        edenfsctl = os.path.join(edenfsctl_real, "../edenfsctl")
    return os.path.normpath(edenfsctl)


def get_edenfs_cmd(
    instance: EdenInstance,
    daemon_binary: str,
) -> Tuple[List[str], str]:
    """Get the command line arguments to use to start the edenfs daemon."""

    cmd = []
    if is_apple_silicon():
        # Prefer native arch on ARM64, fallback to x86_64 otherwise
        cmd += ["arch", "-arch", "arm64", "-arch", "x86_64"]

    cmd += [
        daemon_binary,
        "--edenfs",
        "--edenfsctlPath",
        get_edenfsctl_cmd(),
        "--edenDir",
        str(instance.state_dir),
        "--etcEdenDir",
        str(instance.etc_eden_dir),
        "--configPath",
        str(instance.user_config_path),
    ]

    privhelper_path = os.environ.get("EDENFS_PRIVHELPER_PATH")
    # TODO(cuev): Avoid hardcoding the privhelper path. Instead, we should
    # share candidate paths with FindExe (which is used in integration tests)
    if privhelper_path is None:
        # Default to using the system privhelper. See explanation below.
        privhelper_path = "/usr/local/libexec/eden/edenfs_privhelper"

    cmd += ["--privhelper_path", privhelper_path]

    return cmd, privhelper_path


def prepare_edenfs_privileges(
    daemon_binary: str,
    cmd: List[str],
    env: Dict[str, str],
    privhelper_path: str,
) -> Tuple[List[str], Dict[str, str]]:
    """Update the EdenFS command and environment settings in order to run the
    privhelper as root. Note: in most cases, we don't need to do anything to
    run the privhelper as root since we ship it as a setuid-root binary. This
    is the default case/behavior.

    However, sometimes we need to test non-setuid-root privhelper binaries in
    dev instances of EdenFS or integration tests. In those cases, we need to
    wrap the command in sudo.

    This happens when a non-setuid-root binary is specified by the
    EDENFS_PRIVHELPER_PATH environment variable. In most cases, this env
    variable will not be set and we will simply use the system privhelper. This
    environment variable is set by some development Buck targets in
    fbcode/eden/fs/{cli, cli_rs}/TARGETS. It could also potentially be set by
    other external sources.
    """
    # Nothing to do on Windows
    if sys.platform == "win32":
        return (cmd, env)

    # If we already have root privileges we don't need to do anything.
    if os.geteuid() == 0:
        return (cmd, env)

    # If the EdenFS privhelper is installed as setuid root we don't need to use
    # sudo.
    try:
        s = os.stat(privhelper_path)
        if s.st_uid == 0 and (s.st_mode & stat.S_ISUID):
            return (cmd, env)
    except FileNotFoundError:
        # If the privhelper isn't found, EdenFS would just fail, let it fail
        # instead of here.
        return cmd, env

    # If we're still here, we need to run edenfs under sudo. This is
    # undesireable with passwordless sudo as it requires multiple password
    # prompts, but we will try to run with sudo anyway.
    # In some rare cases, we may want to test using a non-setuid-root
    # privhelper binary.
    sudo_cmd = ["/usr/bin/sudo"]
    # Add environment variable settings
    # Depending on the sudo configuration, these may not
    # necessarily get passed through automatically even when
    # using "sudo -E".
    for key, value in env.items():
        sudo_cmd.append("%s=%s" % (key, value))

    cmd = sudo_cmd + cmd
    return cmd, env


def get_edenfs_environment() -> Dict[str, str]:
    """Get the environment to use to start the edenfs daemon."""
    eden_env = {}

    # Errors from Rust will be logged to the edenfs log.
    eden_env["EDENSCM_LOG"] = "error"

    if sys.platform != "win32":
        # Reset $PATH to the following contents, so that everyone has the
        # same consistent settings.
        path_dirs = ["/opt/facebook/hg/bin", "/usr/local/bin", "/bin", "/usr/bin"]

        eden_env["PATH"] = ":".join(path_dirs)
    else:
        # On Windows, copy the existing PATH as it's not clear what locations
        # are needed.
        eden_env["PATH"] = os.environ["PATH"]

    if sys.platform == "darwin":
        # Prevent warning on mac, which will crash eden:
        # +[__NSPlaceholderDate initialize] may have been in progress in
        # another thread when fork() was called.
        eden_env["OBJC_DISABLE_INITIALIZE_FORK_SAFETY"] = "YES"

    # Preserve the following environment settings
    preserve = [
        "USER",
        "LOGNAME",
        "HOME",
        "EMAIL",
        "NAME",
        "ASAN_OPTIONS",
        # When we import data from mercurial, the remotefilelog extension
        # may need to SSH to a remote mercurial server to get the file
        # contents.  Preserve SSH environment variables needed to do this.
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "KRB5CCNAME",
        "SANDCASTLE_ALIAS",
        "SANDCASTLE_INSTANCE_ID",
        "SCRATCH_CONFIG_PATH",
        # These environment variables are used by Corp2Prod (C2P) Secure Thrift
        # clients to get the user certificates for authentication. (We use
        # C2P Secure Thrift to fetch metadata from SCS).
        "THRIFT_TLS_CL_CERT_PATH",
        "THRIFT_TLS_CL_KEY_PATH",
        # This helps with rust debugging
        "MISSING_FILES",
        "EDENSCM_LOG",
        "EDENSCM_EDENAPI",
        "RUST_BACKTRACE",
        "RUST_LIB_BACKTRACE",
        # Useful for environments that look like prod, but are actually corp
        "CONFIGERATOR_PRETEND_NOT_PROD",
        # Ensure EdenFS respects redirecting which cache directory to write to
        "XDG_CACHE_HOME",
    ]

    if sys.platform == "win32":
        preserve += [
            "APPDATA",
            "SYSTEMROOT",
            "USERPROFILE",
            "USERNAME",
            "PROGRAMDATA",
            "LOCALAPPDATA",
        ]

    for name, value in os.environ.items():
        # Preserve any environment variable starting with "TESTPILOT_".
        # TestPilot uses a few environment variables to keep track of
        # processes started during test runs, so it can track down and kill
        # runaway processes that weren't cleaned up by the test itself.
        # We want to make sure this behavior works during the eden
        # integration tests.
        # Similarly, we want to preserve EDEN* env vars which are
        # populated by our own test infra to relay paths to important
        # build artifacts in our build tree.
        if name.startswith("TESTPILOT_") or name.startswith("EDEN"):
            eden_env[name] = value
        elif name in preserve:
            eden_env[name] = value
        else:
            # Drop any environment variable not matching the above cases
            pass

    return eden_env
