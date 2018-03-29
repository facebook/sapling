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
import pwd
import subprocess
import sys
import time
import typing

import eden.thrift
from thrift import Thrift
from fb303.ttypes import fb_status
from typing import Any, Callable, Optional, Tuple, TypeVar


# These paths are relative to the user's client directory.
LOCK_FILE = 'lock'


class TimeoutError(Exception):
    pass


class EdenStartError(Exception):
    pass


class HealthStatus(object):
    def __init__(self, status: int, pid: Optional[int], detail: str) -> None:
        self.status = status
        self.pid = pid  # The process ID, or None if not running
        self.detail = detail  # a human-readable message

    def is_healthy(self) -> bool:
        return self.status == fb_status.ALIVE


T = TypeVar('T')


def poll_until(
    function: Callable[[], Optional[T]],
    timeout: float,
    interval: float=0.2,
    timeout_ex: Optional[Exception]=None
) -> T:
    '''
    Call the specified function repeatedly until it returns non-None.
    Returns the function result.

    Sleep 'interval' seconds between calls.  If 'timeout' seconds passes
    before the function returns a non-None result, raise an exception.
    If a 'timeout_ex' argument is supplied, that exception object is
    raised, otherwise a TimeoutError is raised.
    '''
    end_time = time.time() + timeout
    while True:
        result = function()
        if result is not None:
            return result

        if time.time() >= end_time:
            if timeout_ex is not None:
                raise timeout_ex
            raise TimeoutError('timed out waiting on function {}'.format(
                function.__name__))

        time.sleep(interval)


def _check_health_using_lockfile(config_dir: str) -> HealthStatus:
    '''Make a best-effort to produce a HealthStatus based on the PID in the
    Eden lockfile.
    '''
    lockfile = os.path.join(config_dir, LOCK_FILE)
    try:
        with open(lockfile, 'r') as f:
            lockfile_contents = f.read()
        pid = lockfile_contents.rstrip()
        int(pid)  # Throw if this does not parse as an integer.
    except Exception:
        # If we cannot read the PID from the lockfile for any reason, return
        # DEAD.
        return _create_dead_health_status()

    try:
        stdout = subprocess.check_output(['ps', '-p', pid, '-o', 'comm='])
    except subprocess.CalledProcessError:
        # If there is no process with the specified id, return DEAD.
        return _create_dead_health_status()

    # Use heuristics to determine that the PID in the lockfile is associated
    # with an edenfs process as it is possible that edenfs is no longer
    # running and the PID in the lockfile has been assigned to a new process
    # unrelated to Eden.
    comm = stdout.rstrip().decode('utf8')
    # Note that the command may be just "edenfs" rather than a path, but it
    # works out fine either way.
    if os.path.basename(comm) == 'edenfs':
        return HealthStatus(fb_status.STOPPED, int(pid),
                            'Eden\'s Thrift server does not appear to be '
                            'running, but the process is still alive ('
                            'PID=%s).' % pid)
    else:
        return _create_dead_health_status()


def _create_dead_health_status() -> HealthStatus:
    return HealthStatus(fb_status.DEAD, pid=None,
                        detail='edenfs not running')


def check_health(
    get_client: Callable[[], eden.thrift.EdenClient],
    config_dir: str
) -> HealthStatus:
    '''
    Get the status of the edenfs daemon.

    Returns a HealthStatus object containing health information.
    '''
    pid = None
    status = fb_status.DEAD
    try:
        with get_client() as client:
            pid = client.getPid()
            status = client.getStatus()
    except eden.thrift.EdenNotRunningError:
        # It is possible that the edenfs process is running, but the Thrift
        # server is not running. This could be during the startup, shutdown,
        # or takeover of the edenfs process. As a backup to requesting the
        # PID from the Thrift server, we read it from the lockfile and try
        # to deduce the current status of Eden.
        return _check_health_using_lockfile(config_dir)
    except Thrift.TException as ex:
        detail = 'error talking to edenfs: ' + str(ex)
        return HealthStatus(status, pid, detail)

    status_name = fb_status._VALUES_TO_NAMES.get(status)
    detail = 'edenfs running (pid {}); status is {}'.format(
        pid, status_name)
    return HealthStatus(status, pid, detail)


def wait_for_daemon_healthy(
    proc: subprocess.Popen,
    config_dir: str,
    get_client: Callable[[], eden.thrift.EdenClient],
    timeout: float,
    exclude_pid: Optional[int]=None
) -> HealthStatus:
    '''
    Wait for edenfs to become healthy.
    '''
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
                msg = 'terminated with signal {}'.format(-status)
            else:
                msg = 'exit status {}'.format(status)
            raise EdenStartError('edenfs exited before becoming healthy: ' +
                                    msg)

        # Still starting
        return None

    timeout_ex = EdenStartError('timed out waiting for edenfs to become '
                                'healthy')
    return poll_until(check_daemon_health,
                      timeout=timeout,
                      timeout_ex=timeout_ex)


def get_home_dir() -> str:
    home_dir = None
    if os.name == 'nt':
        home_dir = os.getenv('USERPROFILE')
    else:
        home_dir = os.getenv('HOME')
    if not home_dir:
        home_dir = pwd.getpwuid(os.getuid()).pw_dir
    return home_dir


def mkdir_p(path: str) -> str:
    '''Performs `mkdir -p <path>` and returns the path.'''
    try:
        os.makedirs(path)
    except OSError as e:
        if e.errno != errno.EEXIST:
            raise
    return path


def is_git_dir(path: str) -> bool:
    return (os.path.isdir(os.path.join(path, 'objects')) and
            os.path.isdir(os.path.join(path, 'refs')) and
            os.path.exists(os.path.join(path, 'HEAD')))


def get_git_dir(path: str) -> Optional[str]:
    '''
    If path points to a git repository, return the path to the repository .git
    directory.  Otherwise, if the path is not a git repository, return None.
    '''
    path = os.path.realpath(path)
    if path.endswith('.git') and is_git_dir(path):
        return path

    git_subdir = os.path.join(path, '.git')
    if is_git_dir(git_subdir):
        return git_subdir

    return None


def get_git_commit(git_dir: str, bookmark: str) -> str:
    '''
    returns git commit SHA for label (e.g., 'HEAD', 'master', etc.)
    '''
    cmd = ['git', 'rev-parse', bookmark]
    out = typing.cast(bytes, subprocess.check_output(cmd, cwd=git_dir))
    return out.strip().decode('utf-8', errors='surrogateescape')


def get_hg_repo(path: str) -> Optional[str]:
    '''
    If path points to a mercurial repository, return a normalized path to the
    repository root.  Otherwise, if path is not a mercurial repository, return
    None.
    '''
    repo_path = os.path.realpath(path)
    hg_dir = os.path.join(repo_path, '.hg')
    if not os.path.isdir(hg_dir):
        return None

    # Check to see if this is a shared working directory from another
    # repository
    try:
        with open(os.path.join(hg_dir, 'sharedpath'), 'r') as f:
            hg_dir = f.readline().rstrip('\n')
            hg_dir = os.path.realpath(hg_dir)
            repo_path = os.path.dirname(hg_dir)
    except EnvironmentError as ex:
        if ex.errno != errno.ENOENT:
            raise

    if not os.path.isdir(os.path.join(hg_dir, 'store')):
        return None

    return repo_path


def get_hg_commit(repo: str, bookmark: str) -> str:
    env = os.environ.copy()
    env['HGPLAIN'] = '1'
    cmd = ['hg', '--cwd', repo, 'log', '-r', bookmark, '-T{node}']
    out = typing.cast(bytes, subprocess.check_output(cmd, env=env))
    return out.decode('utf-8', errors='strict')


def get_repo_source_and_type(path: str) -> Tuple[str, Optional[str]]:
    repo_source = ''
    repo_type = None
    git_dir = get_git_dir(path)
    if git_dir:
        repo_source = git_dir
        repo_type = 'git'
    else:
        hg_repo = get_hg_repo(path)
        if hg_repo:
            repo_source = hg_repo
            repo_type = 'hg'
    return (repo_source, repo_type)


def print_stderr(message: str, *args: Any, **kwargs: Any) -> None:
    '''Prints the message to stderr.'''
    if args or kwargs:
        message = message.format(*args, **kwargs)
    print(message, file=sys.stderr)


def stack_trace() -> str:
    import traceback
    return ''.join(traceback.format_stack())


def is_valid_sha1(sha1: str) -> bool:
    '''True iff sha1 is a valid 40-character SHA1 hex string.'''
    if sha1 is None or len(sha1) != 40:
        return False
    import string
    return set(sha1).issubset(string.hexdigits)


def read_all(path: str) -> str:
    '''One-liner to read the contents of a file and properly close the fd.'''
    with open(path, 'r') as f:
        return f.read()
