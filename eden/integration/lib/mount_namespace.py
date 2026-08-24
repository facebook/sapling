#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import shutil
import subprocess
import sys


# Set in the re-executed process so we only re-exec once.
_INSIDE_ENV = "EDEN_TEST_PRIVATE_MOUNT_NS"
# Set by the user to disable the private mount namespace.
_DISABLE_ENV = "EDEN_TEST_NO_PRIVATE_MOUNT_NS"


def maybe_reexec_in_private_mount_namespace() -> None:
    """Best effort: re-exec the test process inside a private mount namespace.

    EdenFS integration tests create real FUSE/NFS mounts. If a test daemon and
    its privhelper are killed without unmounting (SIGKILL, test runner
    timeout), the mount leaks into the host mount table, where a mount with a
    dead server stalls everything that scans the mount table (df, and through
    it chef). Inside a private mount namespace the kernel tears down all test
    mounts automatically when the last test process exits, and they are never
    visible to the rest of the host.

    Creating the namespace requires root: NFS cannot be mounted from an
    unprivileged user namespace, and the setuid privhelper would lose its
    privileges there. So this only takes effect where passwordless sudo is
    available (e.g. devservers), and is silently skipped everywhere else.
    """
    if sys.platform != "linux":
        return
    if _INSIDE_ENV in os.environ:
        print(
            f"eden test: executing in private mount namespace"
            f" (set {_DISABLE_ENV}=1 to disable)\n"
            f'eden test: run "sudo nsenter -t {os.getpid()} -m" to enter namespace',
            file=sys.stderr,
        )
        return
    if _DISABLE_ENV in os.environ:
        return
    if "SANDCASTLE" in os.environ:
        # CI already runs tests in isolated containers.
        return
    if any(arg.startswith("--list") for arg in sys.argv[1:]):
        # No mounts are created when just listing tests.
        return

    executable = os.path.abspath(sys.argv[0])
    if not os.access(executable, os.X_OK):
        return
    if shutil.which("unshare") is None or shutil.which("env") is None:
        return

    # --propagation slave: host mount events still propagate into the
    # namespace, but mounts created inside never propagate back out.
    prefix = ["unshare", "--mount", "--propagation", "slave"]
    if os.geteuid() != 0:
        # pwd is Unix-only; importing it at module scope would break this
        # module (and every test importing it) on Windows.
        import pwd

        user = pwd.getpwuid(os.getuid()).pw_name
        prefix = ["sudo", "-n"] + prefix + ["sudo", "-n", "-u", user]

    # Pre-flight the full wrapper once, and skip if any part of it does not
    # work here (no passwordless sudo, unshare not permitted, ...).
    probe = subprocess.run(
        prefix + ["true"], capture_output=True, stdin=subprocess.DEVNULL
    )
    if probe.returncode != 0:
        return

    # sudo rewrites the environment; re-apply it explicitly via env(1).
    env_args = [f"{k}={v}" for k, v in os.environ.items()]
    env_args.append(f"{_INSIDE_ENV}=1")
    cmd = prefix + ["env"] + env_args + [executable] + sys.argv[1:]

    try:
        os.execvp(cmd[0], cmd)
    except OSError as ex:
        print(
            f"eden test: failed to re-exec in a private mount namespace: {ex}",
            file=sys.stderr,
        )
