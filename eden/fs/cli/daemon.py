#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import re
import stat
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, List, Optional, Tuple

from eden.fs.cli.util import (
    EdensparseMigrationStep,
    HEARTBEAT_FILE_PREFIX,
    maybe_edensparse_migration,
)

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

EDENFS_UNIT_NAME_TEMPLATE = "edenfs@{escaped_state_dir}.service"


def _sanitize_unit_name(eden_dir: str) -> str:
    """Build a systemd unit name from the eden state directory path.

    Systemd treats '-' and ':' as special characters in unit names, so replace
    them with '_'.
    Append the current UNIX timestamp to avoid collisions during graceful restart
    where old and new scopes coexist briefly.
    """
    sanitized = re.sub(r"[^a-zA-Z0-9_./]", "_", eden_dir).strip("/").replace("/", "_")
    return f"edenfs_{sanitized}_{os.getpid()}_{int(time.time())}"


def _build_systemd_run_cmd(edenfs_cmd: List[str], eden_dir: str) -> List[str]:
    """Wrap an edenfs command in systemd-run for cgroup isolation.

    Places edenfs in a transient scope under a dedicated eden.slice
    """
    unit_name = _sanitize_unit_name(eden_dir)
    return [
        "systemd-run",
        "--user",
        "--scope",
        "--quiet",
        "--collect",
        "--property=Delegate=yes",
        "--slice=eden",
        f"--unit={unit_name}",
    ] + edenfs_cmd


def _try_setup_systemd_cgroup(
    eden_env: Dict[str, str],
    instance: "EdenInstance",
) -> bool:
    """Ensure the D-Bus session env vars are available for systemd-run --user.

    get_edenfs_environment() preserves them from os.environ when present.
    When invoked from a system service (e.g. edenfs_restarter timer), they may
    be missing.  Fall back to the standard systemd paths derived from the UID.
    If the D-Bus socket does not exist, skip cgroup isolation entirely.

    Returns True if systemd-run should be used, False to fall back to the
    original start without systemd-run.
    """
    if "XDG_RUNTIME_DIR" not in eden_env or "DBUS_SESSION_BUS_ADDRESS" not in eden_env:
        uid = os.getuid()
        xdg_runtime_dir = eden_env.get("XDG_RUNTIME_DIR", f"/run/user/{uid}")
        dbus_socket = f"{xdg_runtime_dir}/bus"
        if not os.path.exists(dbus_socket):
            instance.log_sample(
                "systemd_cgroup_start",
                success=False,
                reason=f"dbus_socket_not_found at {dbus_socket}",
            )
            return False
        eden_env.setdefault("XDG_RUNTIME_DIR", xdg_runtime_dir)
        eden_env.setdefault("DBUS_SESSION_BUS_ADDRESS", f"unix:path={dbus_socket}")

    return True


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
    pid: int,
    config_dir: Path,
    timeout: float,
    kill_timeout: float = DEFAULT_SIGKILL_TIMEOUT,
    instance: Optional["EdenInstance"] = None,
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
    sigkill_process(pid, config_dir, timeout=kill_timeout, instance=instance)
    return False


def _send_sigkill(
    pid: int,
    instance: Optional["EdenInstance"] = None,
) -> None:
    """Send SIGKILL to edenfs via systemctl or direct signal."""
    if instance is not None and sys.platform == "linux":
        try:
            unit = _get_systemd_unit(instance)
            if _is_systemd_unit_active(unit):
                subprocess.call(["systemctl", "--user", "kill", "--signal=KILL", unit])
                subprocess.call(["systemctl", "--user", "stop", unit])
                return
        except (RuntimeError, OSError) as ex:
            print_stderr(
                f"Failed to kill edenfs via systemctl, falling back to "
                f"direct signal: {ex}"
            )

    proc_utils: proc_utils_mod.ProcUtils = proc_utils_mod.new()
    try:
        proc_utils.kill_process(pid)
    except PermissionError as ex:
        raise ShutdownError(
            f"Received a permission error when attempting to kill edenfs: {ex}"
        )


def sigkill_process(
    pid: int,
    config_dir: Path,
    timeout: float = DEFAULT_SIGKILL_TIMEOUT,
    instance: Optional["EdenInstance"] = None,
) -> None:
    """Send SIGKILL to a process, and wait for it to exit.

    If timeout is greater than 0, this waits for the process to exit after sending the
    signal.  Throws a ShutdownError exception if the process does not exit within the
    specified timeout.

    Returns successfully if the specified process did not exist in the first place.
    This is done to handle situations where the process exited on its own just before we
    could send SIGKILL.
    """

    # On Windows, EdenFS daemon doesn't have any heartbeat flag.
    if sys.platform != "win32":
        # This SIGKILL is not triggered by the OS due to memory issues, so we should clean up
        # the heartbeat file. This ensures that the SIGKILL won't be mislogged as a silent
        # exit when the next EdenFS daemon starts.
        # The thrift server may be unresponsive at this point, so clean up the file directly
        # instead of sending a thrift request.
        heartbeat_file = config_dir / f"{HEARTBEAT_FILE_PREFIX}{pid}"
        try:
            heartbeat_file.unlink()
        except FileNotFoundError:
            pass
        except Exception as e:
            print_stderr(f"Failed to delete heartbeat file {heartbeat_file}: {e}")

    _send_sigkill(pid, instance)

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
    preserved_env: Optional[List[str]] = None,
) -> int:
    """Start the edenfs daemon."""
    return _start_edenfs_service(
        instance=instance,
        daemon_binary=daemon_binary,
        edenfs_args=edenfs_args,
        preserved_env=preserved_env,
        takeover=False,
    )


def gracefully_restart_edenfs_service(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
    preserved_env: Optional[List[str]] = None,
) -> int:
    """
    Gracefully restart the EdenFS service
    This function ensures a seamless transition from the old EdenFS instance to the new one.
    It prevents the auto unmount recovery task from interfering with the restart process.
    """
    # Set the intentionally unmounted flag for all mounts to prevent auto unmount recovery
    # during the restart process. This ensures that the old EdenFS mounts are not recovered
    # while the new instance is taking over.
    instance.set_intentionally_unmounted_for_all_mounts()
    # Start the new EdenFS service, taking over from the old instance.
    result = _start_edenfs_service(
        instance=instance,
        daemon_binary=daemon_binary,
        edenfs_args=edenfs_args,
        preserved_env=preserved_env,
        takeover=True,
    )
    # Clear the intentionally unmounted flag after the restart is complete.
    # This allows the auto unmount recovery task to resume its normal operation.
    instance.clear_intentionally_unmounted_for_all_mounts()
    return result


def _start_edenfs_service(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
    preserved_env: Optional[List[str]] = None,
    takeover: bool = False,
) -> int:
    """Get the command and environment to use to start edenfs."""
    daemon_binary = daemon_util.find_daemon_binary(daemon_binary)
    cmd, privhelper = get_edenfs_cmd(instance, daemon_binary)

    if takeover:
        cmd.append("--takeover")
    if edenfs_args:
        cmd.extend(edenfs_args)

    eden_env = get_edenfs_environment(preserved_env)

    # Wrap the command in sudo, if necessary. See help text in
    # prepare_edenfs_privileges for more info.
    cmd, eden_env = prepare_edenfs_privileges(daemon_binary, cmd, eden_env, privhelper)

    if is_systemd_enabled(instance):
        return _systemctl_start_or_reload(instance, cmd, eden_env, takeover)

    if (
        sys.platform == "linux"
        and instance.get_config_bool(
            "experimental.systemd-cgroup-isolation", default=False
        )
        and _try_setup_systemd_cgroup(eden_env, instance)
    ):
        cmd = _build_systemd_run_cmd(cmd, str(instance.state_dir))
        use_systemd_cgroup = True
    else:
        use_systemd_cgroup = False

    creation_flags = 0

    maybe_edensparse_migration(instance, EdensparseMigrationStep.PRE_EDEN_START)
    exit_code = subprocess.call(
        cmd, stdin=subprocess.DEVNULL, env=eden_env, creationflags=creation_flags
    )
    maybe_edensparse_migration(instance, EdensparseMigrationStep.POST_EDEN_START)

    if use_systemd_cgroup:
        instance.log_sample(
            "systemd_cgroup_start",
            success=exit_code == 0,
            is_takeover=takeover,
            exit_signal=exit_code,
        )

    return exit_code


def _get_systemd_unit(instance: EdenInstance) -> str:
    """Compute the systemd unit name for this instance.

    Uses systemd-escape to produce the canonical path encoding for the state
    directory.  Raises if systemd-escape is not available or fails.
    """
    config_dir = str(instance.state_dir)
    try:
        escaped = subprocess.check_output(
            ["systemd-escape", "--path", config_dir],
            text=True,
        ).strip()
    except FileNotFoundError:
        raise RuntimeError(
            "systemd-escape is not installed; cannot construct systemd unit name"
        )
    except subprocess.CalledProcessError as e:
        raise RuntimeError(f"systemd-escape failed for {config_dir}: {e}") from e
    return EDENFS_UNIT_NAME_TEMPLATE.format(escaped_state_dir=escaped)


def is_systemd_enabled(instance: EdenInstance) -> bool:
    """Check whether this EdenFS instance should use systemd for lifecycle management."""
    return sys.platform == "linux" and instance.get_config_bool(
        "experimental.systemd-managed-lifecycle", default=False
    )


def _is_systemd_unit_active(unit: str) -> bool:
    """Check whether the systemd unit for this instance is currently active."""
    try:
        result = subprocess.run(
            ["systemctl", "--user", "is-active", "--quiet", unit],
            check=False,
        )
        return result.returncode == 0
    except OSError:
        return False


def _systemctl_start_or_reload(
    instance: EdenInstance,
    cmd: List[str],
    eden_env: Dict[str, str],
    takeover: bool,
) -> int:
    """Start or reload the edenfs systemd service.

    Writes the daemon command and environment to an args file, then calls
    systemctl start (fresh start) or systemctl reload (takeover).
    """
    daemon_util.write_systemd_args_file(instance.state_dir, cmd, eden_env)
    unit = _get_systemd_unit(instance)
    if takeover and _is_systemd_unit_active(unit):
        action = "reload"
    else:
        action = "start"
    rc = subprocess.call(["systemctl", "--user", action, unit])

    # Display the daemon's startup output captured by systemd (StandardOutput=file:).
    startup_log = instance.state_dir / daemon_util.SYSTEMD_STARTUP_LOG_FILENAME
    try:
        sys.stderr.write(startup_log.read_text())
    except OSError:
        pass
    return rc


def get_edenfsctl_cmd() -> str:
    env = os.environ.get("EDENFS_CLI_PATH", None)
    if env:
        return env

    env = os.environ.get("__EDENFSCTL_RUST", None)
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
    # undesirable with passwordless sudo as it requires multiple password
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


def get_edenfs_environment(
    extra_preserve: Optional[List[str]],
) -> Dict[str, str]:
    """Get the environment to use to start the edenfs daemon."""
    eden_env = {}

    # Errors from Rust will be logged to the edenfs log.
    eden_env["SL_LOG"] = (
        "clienttelemetry=info,error,walkdetector=info,backingstore::prefetch=info,indexedlog::rotate=info"
    )

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

    # Setup EdenFs service id
    eden_env["FB_SERVICE_ID"] = "scm/edenfs"

    # Preserve the following environment settings
    #
    # NOTE: This list should be expanded sparingly. Prefer ad-hoc passing of
    # preserved environment variables via the Eden CLI. For example:
    #   edenfsctl --preserved-vars EXAMPE_VAR EXAMPLE_VAR2 start
    # This will ensure that environment variables are only preserved when
    # explicitly requested.
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
        "ATLAS",
        # Identifier of dev docker containers
        "ATLAS_ENV_ID",
        "SANDCASTLE",
        "SANDCASTLE_ALIAS",
        "SANDCASTLE_INSTANCE_ID",
        "SANDCASTLE_VCS",
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
        "SL_LOG",  # alias for EDENSCM_LOG
        # Useful for environments that look like prod, but are actually corp
        "CONFIGERATOR_PRETEND_NOT_PROD",
        # Ensure EdenFS respects redirecting which cache directory to write to
        "XDG_CACHE_HOME",
        # EdenFS should be able to pick-up Mercurial's test config
        "HG_TEST_REMOTE_CONFIG",
        # In some environment, this is used instead of the USER variable
        "CLOUD2PROD_IDENTITY",
        # The following 4 are used on sl's .t tests
        "HGRCPATH",
        "SL_CONFIG_PATH",
        "TESTTMP",
        "HGUSER",
        # The following are used to identify RE platform
        "REMOTE_EXECUTION_SCM_REPO",
        "INSIDE_RE_WORKER",
        # Used by tests to trigger error conditions in instrumented Rust code.
        "FAILPOINTS",
        # systemd-run --user needs these to connect to the user session bus
        # when starting edenfs with cgroup isolation.
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        # Used to identify if edenfs was started by a coding agent
        "CODING_AGENT_METADATA",
    ]

    # Add user-specified environment variables to preserve
    #
    # TODO: If users specify problematic environment variables, we may consider
    # adding a blocklist to prevent them from being preserved.
    if extra_preserve is not None:
        preserve.extend(extra_preserve)

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
