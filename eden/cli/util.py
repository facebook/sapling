#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import errno
import getpass
import json
import os
import random
import subprocess
import sys
import time
import typing
from pathlib import Path
from typing import Any, Callable, List, Optional, TypeVar, Union

import eden.thrift
import thrift.transport
from fb303_core.ttypes import fb303_status
from thrift import Thrift


if os.name != "nt":
    import pwd

# These paths are relative to the user's client directory.
LOCK_FILE = "lock"


class EdenStartError(Exception):
    pass


class ShutdownError(Exception):
    pass


class NotAnEdenMountError(Exception):
    def __init__(self, path: str) -> None:
        self.path = path

    def __str__(self) -> str:
        return f"{self.path} does not appear to be inside an Eden checkout"


class HealthStatus(object):
    def __init__(self, status: fb303_status, pid: Optional[int], detail: str) -> None:
        self.status = status
        self.pid = pid  # The process ID, or None if not running
        self.detail = detail  # a human-readable message

    def is_healthy(self) -> bool:
        return self.status == fb303_status.ALIVE

    def __str__(self) -> str:
        return "(%s, pid=%s, detail=%r)" % (
            fb303_status._VALUES_TO_NAMES.get(self.status, str(self.status)),
            self.pid,
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
    """Read the pid from the Eden lockfile, throwing an exception upon failure.
    """
    lockfile = config_dir / LOCK_FILE
    with lockfile.open("r") as f:
        lockfile_contents = f.read()
    return int(lockfile_contents.rstrip())


def check_health_using_lockfile(config_dir: Path) -> HealthStatus:
    """Make a best-effort to produce a HealthStatus based on the PID in the
    Eden lockfile.
    """
    try:
        # Throws if it does not parse as an int.
        pid = get_pid_using_lockfile(config_dir)
    except Exception:
        # If we cannot read the PID from the lockfile for any reason, return
        # DEAD.
        return _create_dead_health_status()

    try:
        stdout = subprocess.check_output(["ps", "-p", str(pid), "-o", "comm="])
    except subprocess.CalledProcessError:
        # If there is no process with the specified id, return DEAD.
        return _create_dead_health_status()

    # Use heuristics to determine that the PID in the lockfile is associated
    # with an edenfs process as it is possible that edenfs is no longer
    # running and the PID in the lockfile has been assigned to a new process
    # unrelated to Eden.
    comm = stdout.rstrip().decode("utf8")
    # Note that the command may be just "edenfs" rather than a path, but it
    # works out fine either way.
    if os.path.basename(comm) in ("edenfs", "fake_edenfs"):
        return HealthStatus(
            fb303_status.STOPPED,
            pid,
            "Eden's Thrift server does not appear to be "
            "running, but the process is still alive ("
            "PID=%s)." % pid,
        )
    else:
        return _create_dead_health_status()


def _create_dead_health_status() -> HealthStatus:
    return HealthStatus(fb303_status.DEAD, pid=None, detail="edenfs not running")


def check_health(
    get_client: Callable[[], eden.thrift.EdenClient],
    config_dir: Path,
    timeout: Optional[float] = None,
) -> HealthStatus:
    """
    Get the status of the edenfs daemon.

    Returns a HealthStatus object containing health information.
    """
    pid = None
    status = fb303_status.DEAD
    try:
        with get_client() as client:
            client.set_timeout(timeout)
            # TODO: getDaemonInfo() could return both pid and status
            pid = client.getPid()
            status = client.getStatus()
    except (
        eden.thrift.EdenNotRunningError,
        thrift.transport.TTransport.TTransportException,
    ):
        # It is possible that the edenfs process is running, but the Thrift
        # server is not running. This could be during the startup, shutdown,
        # or takeover of the edenfs process. As a backup to requesting the
        # PID from the Thrift server, we read it from the lockfile and try
        # to deduce the current status of Eden.
        return check_health_using_lockfile(config_dir)
    except Thrift.TException as ex:
        detail = "error talking to edenfs: " + str(ex)
        return HealthStatus(status, pid, detail)

    status_name = fb303_status._VALUES_TO_NAMES.get(status)
    detail = "edenfs running (pid {}); status is {}".format(pid, status_name)
    return HealthStatus(status, pid, detail)


def wait_for_daemon_healthy(
    proc: subprocess.Popen,
    config_dir: Path,
    get_client: Callable[[], eden.thrift.EdenClient],
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

    timeout_ex = EdenStartError("timed out waiting for edenfs to become " "healthy")
    return poll_until(check_daemon_health, timeout=timeout, timeout_ex=timeout_ex)


def get_home_dir() -> Path:
    home_dir = None
    if os.name == "nt":
        home_dir = os.getenv("USERPROFILE")
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

    @abc.abstractmethod
    def cat_file(self, commit: str, path: str) -> bytes:
        """
        Returns the file contents for the given file at the given commit.
        """
        pass


class HgRepo(Repo):
    HEAD = "."

    def __init__(self, source: str, working_dir: str) -> None:
        super(HgRepo, self).__init__("hg", source, working_dir)
        self._env = os.environ.copy()
        self._env["HGPLAIN"] = "1"

        # Find the path to hg.
        # The EDEN_HG_BINARY environment variable is normally set when running
        # Eden's integration tests.  Just find 'hg' from the path when it is
        # not set.
        self._hg_binary = os.environ.get("EDEN_HG_BINARY", "hg")

    def __repr__(self) -> str:
        return f"HgRepo(source={self.source!r}, " f"working_dir={self.working_dir!r})"

    def _run_hg(self, args: List[str]) -> bytes:
        cmd = [self._hg_binary] + args
        out_bytes = subprocess.check_output(cmd, cwd=self.working_dir, env=self._env)
        out = typing.cast(bytes, out_bytes)
        return out

    def get_commit_hash(self, commit: str) -> str:
        out = self._run_hg(["log", "-r", commit, "-T{node}"])
        return out.strip().decode("utf-8")

    def cat_file(self, commit: str, path: str) -> bytes:
        return self._run_hg(["cat", "-r", commit, path])


class GitRepo(Repo):
    HEAD = "HEAD"

    def __init__(self, source: str, working_dir: Optional[str] = None) -> None:
        super(GitRepo, self).__init__("git", source, working_dir)

    def __repr__(self) -> str:
        return f"GitRepo(source={self.source!r}, " f"working_dir={self.working_dir!r})"

    def _run_git(self, args: List[str]) -> bytes:
        cmd = ["git"] + args
        out = typing.cast(bytes, subprocess.check_output(cmd, cwd=self.source))
        return out

    def get_commit_hash(self, commit: str) -> str:
        out = self._run_git(["rev-parse", commit])
        return out.strip().decode("utf-8")

    def cat_file(self, commit: str, path: str) -> bytes:
        return self._run_git(["cat-file", "blob", ":".join((commit, path))])


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


def _get_hg_repo(path: str) -> Optional[HgRepo]:
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


def get_repo(path: str) -> Optional[Repo]:
    """
    Given a path inside a repository, return the repository source and type.
    """
    path = os.path.realpath(path)
    if not os.path.exists(path):
        return None

    while True:
        hg_repo = _get_hg_repo(path)
        if hg_repo is not None:
            return hg_repo
        git_repo = _get_git_repo(path)
        if git_repo is not None:
            return git_repo

        parent = os.path.dirname(path)
        if parent == path:
            return None

        path = parent


def get_project_id(repo: Repo, rev: Optional[str]) -> Optional[str]:
    contents = None
    if rev is not None:
        try:
            contents = repo.cat_file(rev, ".arcconfig")
        except subprocess.CalledProcessError:
            # Most likely .arcconfig does not exist.
            pass

    if contents is None:
        try:
            contents = repo.cat_file(repo.HEAD, ".arcconfig")
        except subprocess.CalledProcessError:
            # Most likely .arcconfig does not exist.
            # We cannot determine the project ID.
            return None

    try:
        data = json.loads(contents)
    except Exception:
        # .arcconfig does not contain valid JSON data for some reason.
        return None

    return typing.cast(Optional[str], data.get("project_id", None))


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


def readlink_retry_estale(path: Union[Path, str]) -> str:
    attempts = 10
    while True:
        try:
            return os.readlink(path)
        except OSError as ex:
            if attempts == 0 or ex.errno != errno.ESTALE:
                raise
            pass
        attempts -= 1
        time.sleep(random.uniform(0.001, 0.01))


def get_eden_mount_name(path_arg: str) -> str:
    """
    Get the path to the Eden checkout containing the specified path
    """
    path = os.path.join(path_arg, ".eden", "root")
    try:
        return readlink_retry_estale(path)
    except OSError as ex:
        if ex.errno == errno.ENOTDIR:
            path = os.path.join(os.path.dirname(path_arg), ".eden", "root")
            return readlink_retry_estale(path)
        elif ex.errno == errno.ENOENT:
            raise NotAnEdenMountError(path_arg)
        raise


def get_username() -> str:
    return getpass.getuser()
