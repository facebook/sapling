#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import abc
import binascii
import errno
import getpass
import json
import os
import random
import re
import shlex
import stat
import subprocess
import sys
import time
import typing
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, TYPE_CHECKING, TypeVar, Union

import thrift.transport
from eden.thrift.legacy import EdenClient, EdenNotRunningError
from facebook.eden.ttypes import TreeInodeDebugInfo
from fb303_core.ttypes import fb303_status
from thrift import Thrift


if TYPE_CHECKING:
    from .config import EdenInstance

if sys.platform != "win32":
    import pwd

# These paths are relative to the user's client directory.
LOCK_FILE = "lock"
PID_FILE = "pid"


class EdenStartError(Exception):
    pass


class ShutdownError(Exception):
    pass


class NotAnEdenMountError(Exception):
    def __init__(self, path: str) -> None:
        self.path = path

    def __str__(self) -> str:
        return f"{self.path} does not appear to be inside an EdenFS checkout"


class HealthStatus(object):
    def __init__(
        self,
        status: fb303_status,
        pid: Optional[int],
        uptime: Optional[float],
        detail: str,
    ) -> None:
        self.status = status
        self.pid = pid  # The process ID, or None if not running
        self.uptime = uptime
        self.detail = detail  # a human-readable message

    def is_healthy(self) -> bool:
        return self.status == fb303_status.ALIVE

    def is_starting(self) -> bool:
        return self.status == fb303_status.STARTING

    def __str__(self) -> str:
        return "(%s, pid=%s, uptime=%s, detail=%r)" % (
            fb303_status._VALUES_TO_NAMES.get(self.status, str(self.status)),
            self.pid,
            self.uptime,
            self.detail,
        )


T = TypeVar("T")


def poll_until(
    function: Callable[[], Optional[T]],
    timeout: float,
    interval: float = 0.2,
    timeout_ex: Optional[Exception] = None,
) -> T:
    """
    Call the specified function repeatedly until it returns non-None.
    Returns the function result.

    Sleep 'interval' seconds between calls.  If 'timeout' seconds passes
    before the function returns a non-None result, raise an exception.
    If a 'timeout_ex' argument is supplied, that exception object is
    raised, otherwise a TimeoutError is raised.
    """
    end_time = time.time() + timeout
    while True:
        result = function()
        if result is not None:
            return result

        if time.time() >= end_time:
            if timeout_ex is not None:
                raise timeout_ex
            raise TimeoutError(
                "timed out waiting on function {}".format(function.__name__)
            )

        time.sleep(interval)


def get_pid_using_lockfile(config_dir: Path) -> int:
    """Read the pid from the EdenFS lockfile, throwing an exception upon failure."""
    if sys.platform == "win32":
        # On Windows read the separate pid file.  We will not be able to read the
        # lock file if EdenFS is running and holding the lock.
        lockfile = config_dir / PID_FILE
    else:
        # On other platforms we still prefer reading the pid from the lock file,
        # just to support older instances of EdenFS that only wrote the pid to the lock
        # file.
        lockfile = config_dir / LOCK_FILE

    with lockfile.open("r") as f:
        lockfile_contents = f.read()
    return int(lockfile_contents.rstrip())


def check_health_using_lockfile(config_dir: Path) -> HealthStatus:
    """Make a best-effort to produce a HealthStatus based on the PID in the
    EdenFS lockfile.
    """
    try:
        # Throws if it does not parse as an int.
        pid = get_pid_using_lockfile(config_dir)
    except Exception:
        # If we cannot read the PID from the lockfile for any reason, return
        # DEAD.
        return _create_dead_health_status()

    from . import proc_utils as proc_utils_mod

    proc_utils = proc_utils_mod.new()
    if proc_utils.is_edenfs_process(pid):
        return HealthStatus(
            fb303_status.STOPPED,
            pid,
            uptime=None,
            detail="EdenFS's Thrift server does not appear to be "
            "running, but the process is still alive (PID=%s)." % pid,
        )
    else:
        return _create_dead_health_status()


def _create_dead_health_status() -> HealthStatus:
    return HealthStatus(
        fb303_status.DEAD, pid=None, uptime=None, detail="EdenFS not running"
    )


def check_health(
    get_client: Callable[..., EdenClient],
    config_dir: Path,
    timeout: Optional[float] = None,
) -> HealthStatus:
    """
    Get the status of the edenfs daemon.

    Returns a HealthStatus object containing health information.
    """
    if timeout is None:
        # Default to a 3 second timeout if an explicit value was not specified
        timeout = 3.0

    pid = None
    status = fb303_status.DEAD
    uptime = None
    try:
        with get_client(timeout=timeout) as client:
            info = client.getDaemonInfo()
            pid = info.pid
            status_value = info.status
            # Our wrapper client class always ensures that info.status is present.
            # It will explicitly call getStatus() to get this information if the
            # daemon is running an older version that does not return this from
            # getDaemonInfo()
            assert status_value is not None
            status = status_value
            uptime = info.uptime
    except (EdenNotRunningError, thrift.transport.TTransport.TTransportException):
        # It is possible that the edenfs process is running, but the Thrift
        # server is not running. This could be during the startup, shutdown,
        # or takeover of the edenfs process. As a backup to requesting the
        # PID from the Thrift server, we read it from the lockfile and try
        # to deduce the current status of EdenFS.
        return check_health_using_lockfile(config_dir)
    except Thrift.TException as ex:
        detail = "error talking to edenfs: " + str(ex)
        return HealthStatus(status, pid, uptime, detail)

    status_name = fb303_status._VALUES_TO_NAMES.get(status)
    detail = "edenfs running (pid {}); status is {}".format(pid, status_name)
    return HealthStatus(status, pid, uptime, detail)


def wait_for_daemon_healthy(
    proc: subprocess.Popen,
    config_dir: Path,
    get_client: Callable[..., EdenClient],
    timeout: float,
    exclude_pid: Optional[int] = None,
) -> HealthStatus:
    """
    Wait for edenfs to become healthy.
    """

    def check_daemon_health() -> Optional[HealthStatus]:
        # Check the thrift status
        health_info = check_health(get_client, config_dir)
        if health_info.is_healthy():
            if (exclude_pid is None) or (health_info.pid != exclude_pid):
                return health_info

        # Make sure that edenfs is still running
        status = proc.poll()
        if status is not None:
            if status < 0:
                msg = "terminated with signal {}".format(-status)
            else:
                msg = "exit status {}".format(status)
            raise EdenStartError("edenfs exited before becoming healthy: " + msg)

        # Still starting
        return None

    timeout_ex = EdenStartError("timed out waiting for edenfs to become healthy")
    return poll_until(check_daemon_health, timeout=timeout, timeout_ex=timeout_ex)


def wait_for_instance_healthy(instance: "EdenInstance", timeout: float) -> HealthStatus:
    """
    Wait for EdenFS to become healthy. This method differs from wait_for_daemon_healthy
    because wait_for_daemon_healthy is used for EdenFS instances spawned by a direct
    child, while this method is for use to wait on an existent process.
    """
    from . import proc_utils as proc_utils_mod

    proc_utils = proc_utils_mod.new()

    def check_daemon_health() -> Optional[HealthStatus]:
        # Check the thrift status
        health_info = instance.check_health()
        if health_info.is_healthy():
            return health_info

        # Make sure that the edenfs process is still alive
        pid = health_info.pid
        if pid is None or not proc_utils.is_process_alive(pid):
            raise EdenStartError("edenfs exited before becoming healthy")

        # Still starting
        return None

    timeout_ex = EdenStartError("timed out waiting for edenfs to become healthy")
    return poll_until(check_daemon_health, timeout=timeout, timeout_ex=timeout_ex)


def get_home_dir() -> Path:
    # NOTE: Path.home() should work on all platforms, but we would want
    # to be careful about making that change in case users have muddled with
    # their HOME env var or if the resolution is weird in a containairzed
    # environment. It would be worth having some external logging to count
    # mismatches between the two approaches
    home_dir = None
    if sys.platform == "win32":
        home_dir = os.getenv("USERPROFILE")
        if not home_dir:
            return Path.home()
    else:
        home_dir = os.getenv("HOME")
        if not home_dir:
            home_dir = pwd.getpwuid(os.getuid()).pw_dir
    return Path(home_dir)


def mkdir_p(path: str) -> str:
    """Performs `mkdir -p <path>` and returns the path."""
    try:
        os.makedirs(path)
    except OSError as e:
        if e.errno != errno.EEXIST:
            raise
    return path


class Repo(abc.ABC):
    HEAD: str = "Must be defined by subclasses"

    def __init__(
        self, type: str, source: str, working_dir: Optional[str] = None
    ) -> None:
        # The repository type: 'hg' or 'git'
        self.type = type
        # The repository data source.
        # For mercurial this is the directory containing .hg/store
        # For git this is the repository .git directory
        self.source = source
        # The root of the working directory
        self.working_dir = working_dir

    def __repr__(self) -> str:
        return (
            f"Repo(type={self.type!r}, source={self.source!r}, "
            f"working_dir={self.working_dir!r})"
        )

    @abc.abstractmethod
    def get_commit_hash(self, commit: str) -> str:
        """
        Returns the commit hash for the given hg revision ID or git
        commit-ish.
        """
        pass


class HgRepo(Repo):
    HEAD = "."

    def __init__(self, source: str, working_dir: Optional[str] = None) -> None:
        super(HgRepo, self).__init__(
            "hg", source, source if working_dir is None else working_dir
        )
        self._env = os.environ.copy()
        self._env["HGPLAIN"] = "1"

        # These are set by the par machinery and interfere with Mercurial's
        # own dynamic library loading.
        self._env.pop("DYLD_INSERT_LIBRARIES", None)
        self._env.pop("DYLD_LIBRARY_PATH", None)

        # Find the path to hg.
        # The EDEN_HG_BINARY environment variable is normally set when running
        # Eden's integration tests.  Just find 'hg' from the path when it is
        # not set.
        self._hg_binary = os.environ.get("EDEN_HG_BINARY", "hg")

    def __repr__(self) -> str:
        return f"HgRepo(source={self.source!r}, " f"working_dir={self.working_dir!r})"

    def _run_hg(self, args: List[str], stderr_output=None) -> bytes:
        cmd = [self._hg_binary] + args
        out_bytes = subprocess.check_output(
            cmd, cwd=self.working_dir, env=self._env, stderr=stderr_output
        )
        # pyre-fixme[22]: The cast is redundant.
        out = typing.cast(bytes, out_bytes)
        return out

    def get_commit_hash(self, commit: str, stderr_output=None) -> str:
        out = self._run_hg(["log", "-r", commit, "-T{node}"], stderr_output)
        return out.strip().decode("utf-8")


class GitRepo(Repo):
    HEAD = "HEAD"

    def __init__(self, source: str, working_dir: Optional[str] = None) -> None:
        super(GitRepo, self).__init__("git", source, working_dir)

    def __repr__(self) -> str:
        return f"GitRepo(source={self.source!r}, " f"working_dir={self.working_dir!r})"

    def _run_git(self, args: List[str]) -> bytes:
        cmd = ["git"] + args
        # pyre-fixme[22]: The cast is redundant.
        out = typing.cast(bytes, subprocess.check_output(cmd, cwd=self.source))
        return out

    def get_commit_hash(self, commit: str) -> str:
        out = self._run_git(["rev-parse", commit])
        return out.strip().decode("utf-8")


class ReCasRepo(Repo):
    HEAD = "HEAD"

    def __init__(self, source: str, working_dir: Optional[str] = None) -> None:
        if working_dir is not None:
            raise RuntimeError("ReCas Repo is not expected a working_dir")
        super(ReCasRepo, self).__init__("recas", source, working_dir)

    def __repr__(self) -> str:
        return f"ReCasRepo(source={self.source!r})"

    def get_commit_hash(self, commit: str) -> str:
        raise NotImplementedError(
            "get_comit_hash is not supposed to be called for ReCasRepo"
        )


def mkscratch_bin() -> Path:
    # mkscratch is provided by the hg deployment at facebook, which has a
    # different installation prefix on macOS vs Linux, so we need to resolve
    # it via the PATH.  In the integration test environment we'll set the
    # MKSCRATCH_BIN to point to the binary under test
    return Path(os.environ.get("MKSCRATCH_BIN", "mkscratch"))


def is_git_dir(path: str) -> bool:
    return (
        os.path.isdir(os.path.join(path, "objects"))
        and os.path.isdir(os.path.join(path, "refs"))
        and os.path.exists(os.path.join(path, "HEAD"))
    )


def _get_git_repo(path: str) -> Optional[GitRepo]:
    """
    If path points to a git repository, return a GitRepo object.
    Otherwise, if the path is not a git repository, return None.
    """
    if path.endswith(".git") and is_git_dir(path):
        return GitRepo(path)

    git_subdir = os.path.join(path, ".git")
    if is_git_dir(git_subdir):
        return GitRepo(git_subdir, path)

    return None


def get_hg_repo(path: str) -> Optional[HgRepo]:
    """
    If path points to a mercurial repository, return a HgRepo object.
    Otherwise, if path is not a mercurial repository, return None.
    """
    repo_path = path
    working_dir = path
    hg_dir = os.path.join(repo_path, ".hg")
    if not os.path.isdir(hg_dir):
        return None

    # Check to see if this is a shared working directory from another
    # repository
    try:
        with open(os.path.join(hg_dir, "sharedpath"), "r") as f:
            hg_dir = f.readline().rstrip("\n")
            hg_dir = os.path.realpath(hg_dir)
            repo_path = os.path.dirname(hg_dir)
    except EnvironmentError as ex:
        if ex.errno != errno.ENOENT:
            raise

    if not os.path.isdir(os.path.join(hg_dir, "store")):
        return None

    return HgRepo(repo_path, working_dir)


def get_recas_repo(path: str) -> Optional[ReCasRepo]:
    """
    If path points to a Re Cas dir, return a ReCasRepo object.
    Otherwise, return None.
    """
    return ReCasRepo(path)


def get_repo(path: str, backing_store_type: Optional[str] = None) -> Optional[Repo]:
    """
    Given a path inside a repository, return the repository source and type.
    """
    path = os.path.realpath(path)
    if not os.path.exists(path):
        return None

    if backing_store_type is not None and backing_store_type == "recas":
        recas_repo = get_recas_repo(path)
        if recas_repo is not None:
            return recas_repo

    while True:
        hg_repo = get_hg_repo(path)
        if hg_repo is not None:
            return hg_repo
        git_repo = _get_git_repo(path)
        if git_repo is not None:
            return git_repo

        parent = os.path.dirname(path)
        if parent == path:
            return None

        path = parent


def print_stderr(message: str, *args: Any, **kwargs: Any) -> None:
    """Prints the message to stderr."""
    if args or kwargs:
        message = message.format(*args, **kwargs)
    print(message, file=sys.stderr)


def stack_trace() -> str:
    import traceback

    return "".join(traceback.format_stack())


def is_valid_sha1(sha1: str) -> bool:
    """True iff sha1 is a valid 40-character SHA1 hex string."""
    if sha1 is None or len(sha1) != 40:
        return False
    import string

    return set(sha1).issubset(string.hexdigits)


def get_eden_mount_name(path_arg: str) -> str:
    """
    Get the path to the EdenFS checkout containing the specified path
    """
    if sys.platform == "win32":
        path = path_arg
        parent = os.path.dirname(path)
        while path != parent:
            if os.path.isdir(os.path.join(path, ".eden")):
                return os.path.realpath(path)
            if os.path.exists(os.path.join(path, ".hg")):
                break
            path = parent
            parent = os.path.dirname(path)

        raise NotAnEdenMountError(path_arg)
    else:
        path = os.path.join(path_arg, ".eden", "root")
        try:
            return os.readlink(path)
        except OSError as ex:
            if ex.errno == errno.ENOTDIR:
                path = os.path.join(os.path.dirname(path_arg), ".eden", "root")
                return os.readlink(path)
            elif ex.errno == errno.ENOENT:
                raise NotAnEdenMountError(path_arg)
            raise


def get_username() -> str:
    return getpass.getuser()


class LoadedNode(typing.NamedTuple):
    path: str
    is_write: bool
    file_size: int


def make_loaded_node(path: str, is_write: bool, file_size: Optional[int]) -> LoadedNode:
    assert file_size is not None, "File should have associated file size"
    return LoadedNode(path=path, is_write=is_write, file_size=file_size)


def split_inodes_by_operation_type(
    inode_results: typing.Sequence[TreeInodeDebugInfo],
) -> typing.Tuple[
    typing.List[typing.Tuple[str, int]], typing.List[typing.Tuple[str, int]]
]:
    loaded_node_info = [
        make_loaded_node(
            path=os.path.join(os.fsdecode(tree.path), os.fsdecode(n.name)),
            is_write=n.materialized or not n.hash,
            file_size=n.fileSize,
        )
        for tree in inode_results
        for n in tree.entries
        if n.loaded and stat.S_IFMT(n.mode) == stat.S_IFREG
    ]

    read_files = [(o.path, o.file_size) for o in loaded_node_info if not o.is_write]
    written_files = [(o.path, o.file_size) for o in loaded_node_info if o.is_write]
    return read_files, written_files


def fdatasync(fd: int) -> None:
    getattr(os, "fdatasync", os.fsync)(fd)


def write_file_atomically(path: Path, contents: bytes) -> None:
    "Atomically writes or replaces a file at path with the given contents."
    tmp = path.with_suffix(".tmp" + hex(random.getrandbits(64))[2:])
    try:
        with tmp.open("xb") as f:
            f.write(contents)
            f.flush()
            fdatasync(f.fileno())
        os.replace(tmp, path)
    except Exception:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


def format_cmd(cmd: bytes) -> str:
    args = os.fsdecode(cmd)

    # remove trailing null which would cause the command to show up with an
    # exta empty string on the end
    args = re.sub("\x00$", "", args)

    args = args.split("\x00")

    # Focus on just the basename as the paths can be quite long
    cmd_str: str = args[0]
    if os.path.isabs(cmd):
        cmd_str = os.path.basename(cmd_str)

    # Show cmdline args too, if they exist
    return " ".join(shlex.quote(p) for p in [cmd_str] + args[1:])


def format_mount(mount: bytes) -> str:
    return os.fsdecode(os.path.basename(mount))


def underlined(message: str) -> str:
    line = "-" * len(message)
    return f"\n{message}\n{line}\n"


def is_edenfs_mount_device(device: bytes) -> bool:
    return device == b"eden" or device == b"edenfs" or device.startswith(b"edenfs:")


def get_eden_cli_cmd(argv: List[str] = sys.argv) -> List[str]:
    # We likely only need to do this on windows to make sure we run the
    # edenfsctl in a python environment that isn't frozen. But this should
    # be safe to do on all platforms, so for sake of uniformity, do it
    # everywhere.
    return [sys.executable] + argv[:1]


# some processes like hg and arc are sensitive about their environments, we
# clear variables that might make problems for their dynamic linking.
# note buck is even more sensitive see buck.run_buck_command
def get_environment_suitable_for_subprocess() -> Dict[str, str]:
    env = os.environ.copy()

    # Clean out par related environment so that we don't cause problems
    # for our child process
    for k in os.environ.keys():
        if k in ("DYLD_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES", "PAR_LAUNCH_TIMESTAMP"):
            del env[k]
        elif k.startswith("FB_PAR") or k.startswith("PYTHON"):
            del env[k]

    return env


def is_sandcastle() -> bool:
    return "SANDCASTLE" in os.environ


def is_apple_silicon() -> bool:
    if sys.platform == "darwin":
        return "ARM64" in os.uname().version
    else:
        return False


def get_protocol(nfs: bool) -> str:
    if sys.platform == "win32":
        return "prjfs"
    else:
        return "nfs" if nfs else "fuse"


def get_tip_commit_hash(repo: Path) -> bytes:
    # Try to get the tip commit ID.  If that fails, use the null commit ID.
    args = ["hg", "log", "-T", "{node}", "-r", "tip"]
    env = dict(os.environ, HGPLAIN="1")
    result = subprocess.run(
        args,
        env=env,
        cwd=str(repo),
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return binascii.unhexlify(result.stdout.strip())
