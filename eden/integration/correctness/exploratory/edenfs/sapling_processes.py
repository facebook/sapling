# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""One rule for every Sapling process either differential side starts.

Sapling starts some helpers in a session of their own: spawn-ext's
``spawn_detached`` (``setsid`` and closed stdio; used for background hooks,
the backgroundfsync hook, config regeneration, remotefilelog background
prefetch, and the EdenFS redirect fixup after a checkout) leaves the
leader's process group but keeps its environment. So each process runs as
a new session leader with a unique ``EDEN_EXPLORATORY_REFERENCE_TOKEN`` in
its environment, writing its output to anonymous files rather than pipes.
Every token is the owner's run token, a dot, and a per-process suffix, so
the processes one owner started are told apart from any other run's on the
same host. When the leader exits, everything still in its group or
carrying its token is killed at once and reported, so a descendant that
holds the output or starts its own session is never waited on. A process
whose descendants survive SIGKILL stays in the live table, so cleanup
retries and counts it.
"""

from __future__ import annotations

import os
import select
import signal
import subprocess
import tempfile
import threading
import time
import uuid
from collections.abc import Callable, Iterable, Mapping, Sequence
from contextlib import suppress
from typing import IO

from eden.integration.correctness.exploratory.core.processes import (
    kill_group,
    poll_until,
    spawn,
)
from eden.integration.correctness.exploratory.core.target import ProcessRegistry

TOKEN_ENV = "EDEN_EXPLORATORY_REFERENCE_TOKEN"

SPAWN = "spawn"
REFERENCE_TIMEOUT = "reference-timeout"
LEFT_RUNNING = "left-running"
GROUP_LEAK = "group-leak"
CLOSED = "closed"

_CLOSE_WAIT_SECONDS = 10.0
_POLL_SECONDS = 0.01

Failure = Callable[[str, str], Exception]


def kill_wait_for(operation_seconds: float) -> float:
    """Return how long one operation's leftovers may take to die after SIGKILL.

    A tenth of the deadline, at most 10 s, so a side that also bounds its leader by half
    the deadline lands its typed failure before the paired join (three
    quarters) and the scheduler watchdog (the whole).
    """
    return min(10.0, operation_seconds / 10)


class _Live:
    """One Sapling process and the evidence that it is still its owner's.

    The leader stays unreaped until its process is finished, so its PID,
    and with it the process-group ID, cannot be reused while anything
    signals the group. ``pidfd`` reports the leader's exit without reaping
    it, and ``lock`` orders every group signal before the one reap.
    """

    def __init__(
        self, process: subprocess.Popen[bytes], token: str, pidfd: int
    ) -> None:
        self.process = process
        self.token = token
        self.pidfd = pidfd
        self.lock = threading.Lock()
        self.reaped = False
        self._returncode = 0

    def exited(self, timeout: float | None = 0.0) -> bool:
        """Return whether the leader has exited, waiting up to ``timeout`` (None: forever); never reaps.

        The wait is on a duplicate of the pidfd taken under the lock, so a
        concurrent ``reap`` closing the original never invalidates it.
        """
        with self.lock:
            if self.reaped:
                return True
            fd = os.dup(self.pidfd)
        try:
            wait = None if timeout is None else max(0.0, timeout)
            readable, _, _ = select.select([fd], [], [], wait)
        finally:
            os.close(fd)
        return bool(readable)

    def signal(self) -> None:
        """SIGKILL the group while its leader is unreaped, then every token carrier by pidfd."""
        with self.lock:
            if not self.reaped:
                with suppress(PermissionError):
                    kill_group(self.process.pid)
        signal_tokens(self.token)

    def remaining(self) -> list[int]:
        """Return this process's live descendants: unreaped group members and token carriers."""
        with self.lock:
            group = [] if self.reaped else _group_pids(self.process.pid)
        return sorted({*group, *_token_pids(self.token)})

    def reap(self) -> int:
        """Reap the exited leader once and release its pidfd; return its exit status."""
        with self.lock:
            if not self.reaped:
                self._returncode = self.process.wait()
                self.reaped = True
                os.close(self.pidfd)
            return self._returncode


class SaplingProcesses:
    """Every Sapling process of one side: spawned, killed, and counted alike.

    Each process is published in ``registry`` under its caller's key and
    raises ``failure(kind, detail)`` when it yields no trustworthy result.
    """

    def __init__(
        self,
        registry: ProcessRegistry,
        failure: Failure,
        *,
        umask: int = -1,
        scratch: str | None = None,
    ) -> None:
        self._registry = registry
        self._failure = failure
        self._umask = umask
        self._scratch = scratch
        self._lock = threading.Lock()
        self._live: dict[str, _Live] = {}
        self._issued: set[str] = set()
        self._closed = False
        self._token = uuid.uuid4().hex

    @property
    def token(self) -> str:
        """Return the run token every process token of this owner begins with."""
        return self._token

    def live(self) -> tuple[str, ...]:
        """Return the keys of processes not yet finished."""
        with self._lock:
            return tuple(sorted(self._live))

    def leftovers(self) -> list[int]:
        """Return every live process carrying a token this owner issued.

        Ownership is read from each process's own environment at the time
        of the scan, so another run's processes and an unrelated process
        that reused a recorded PID are never counted.
        """
        return _carrier_pids(_run_marker(self._token))

    def kill(self, key: str, kill_wait: float) -> bool | None:
        """SIGKILL one process's group and token carriers; return whether its leader had already exited.

        Returns None when ``key`` names no process this owner started, and
        True when that process is already finished. The leader's exit is
        read through its pidfd and its group signaled under the owner lock,
        so ``_finish`` cannot reap it in between; the one reap stays with
        ``run``. Anything that survives SIGKILL for ``kill_wait`` raises
        ``group-leak``.
        """
        with self._lock:
            if key not in self._issued:
                return None
            entry = self._live.get(key)
            if entry is None:
                return True
            exited = entry.exited()
            entry.signal()
        if not poll_until(
            lambda: entry.exited(_POLL_SECONDS) and not entry.remaining(),
            time.monotonic() + kill_wait,
            0.0,
        ):
            raise self._failure(GROUP_LEAK, f"{key} survived SIGKILL")
        return exited

    def run(
        self,
        key: str,
        command: Sequence[str],
        kill_wait: float,
        *,
        cwd: str,
        env: Mapping[str, str],
        until: float | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Run one published Sapling process and require that it leaves nothing running.

        A leader still running at ``until`` is killed with everything it
        started and raises ``reference-timeout``; with no ``until`` the
        leader runs until it exits or cleanup kills it. When the leader
        exits, anything it started that is still running is killed and
        raises ``left-running``. Anything that survives SIGKILL for
        ``kill_wait`` raises ``group-leak`` and stays live for
        ``close_resources``.
        """
        timeout = None if until is None else max(0.0, until - time.monotonic())
        token = f"{self._token}.{uuid.uuid4().hex}"
        with (
            tempfile.TemporaryFile(dir=self._scratch) as stdout,
            tempfile.TemporaryFile(dir=self._scratch) as stderr,
        ):
            entry = self._spawn(key, command, cwd, env, stdout, stderr, token)
            timed_out = not entry.exited(timeout)
            left = self._kill_leftovers(entry, kill_wait)
            returncode = self._finish(key, entry)
            with self._lock:
                closed = self._closed
            if closed:
                raise self._failure(CLOSED, f"{key} was killed by cleanup")
            if timed_out:
                raise self._failure(
                    # pyrefly: ignore[invalid-argument]
                    REFERENCE_TIMEOUT,
                    # pyrefly: ignore[invalid-argument]
                    f"{key} ran past {timeout:.1f}s: {left}",
                )
            if left:
                raise self._failure(
                    LEFT_RUNNING, f"{key} left processes running: {left}"
                )
            return subprocess.CompletedProcess(
                list(command), returncode, _read(stdout), _read(stderr)
            )

    def _spawn(
        self,
        key: str,
        command: Sequence[str],
        cwd: str,
        env: Mapping[str, str],
        stdout: IO[bytes],
        stderr: IO[bytes],
        token: str,
    ) -> _Live:
        """Start, publish, and pin one process, or raise a typed ``spawn`` failure."""
        with self._lock:
            if self._closed:
                raise self._failure(CLOSED, key)
            try:
                process = spawn(
                    self._registry,
                    key,
                    command,
                    cwd=cwd,
                    env={**env, TOKEN_ENV: token},
                    stdin=subprocess.DEVNULL,
                    stdout=stdout,
                    stderr=stderr,
                    umask=self._umask,
                )
            except RuntimeError as error:
                signal_tokens(token)
                raise self._failure(SPAWN, f"{key}: {error}") from error
            except (OSError, ValueError) as error:
                raise self._failure(SPAWN, f"{key}: {error}") from error
            try:
                pidfd = os.pidfd_open(process.pid)
            except OSError as error:
                with suppress(PermissionError):
                    kill_group(process.pid)
                process.wait()
                self._registry.retire(key, process)
                raise self._failure(SPAWN, f"{key}: pidfd: {error}") from error
            entry = _Live(process, token, pidfd)
            self._live[key] = entry
            self._issued.add(key)
            return entry

    def _finish(self, key: str, entry: _Live) -> int:
        """Reap a finished process's leader and retire it; idempotent across owners."""
        with self._lock:
            returncode = entry.reap()
            if self._live.get(key) is not entry:
                return returncode
            del self._live[key]
        self._registry.retire(key, entry.process)
        return returncode

    def _kill_leftovers(self, entry: _Live, kill_wait: float) -> list[str]:
        """Kill and describe whatever is running once the leader exited or overran.

        Returns the command lines of the processes killed (an overrunning
        leader among them), empty when the leader exited and left nothing.
        Raises ``group-leak`` when any survive SIGKILL for ``kill_wait``;
        the leader then stays unreaped.
        """
        left = [_describe(pid) for pid in entry.remaining()]
        if not left and entry.exited():
            return []

        def gone() -> bool:
            entry.signal()
            return entry.exited(_POLL_SECONDS) and not entry.remaining()

        if not poll_until(gone, time.monotonic() + kill_wait, 0.0):
            raise self._failure(GROUP_LEAK, f"{left} survived SIGKILL")
        return left

    def close_resources(self, deadline: float | None = None) -> int:
        """SIGKILL every live process and descendant, and reap them by the deadline.

        New processes are refused from here on. Once every live process is
        finished, any other process still carrying one of this owner's
        tokens is killed too. Returns how many processes still had a
        descendant at the deadline, counting surviving stray carriers as one
        more; live processes stay for a later retry. A None deadline allows
        a bounded default wait.
        """
        with self._lock:
            self._closed = True
            live = tuple(self._live.items())
        until = time.monotonic() + _CLOSE_WAIT_SECONDS if deadline is None else deadline
        for _key, entry in live:
            entry.signal()
        leaks = 0
        for key, entry in live:

            def gone(key: str = key, entry: _Live = entry) -> bool:
                with self._lock:
                    if self._live.get(key) is not entry:
                        return True
                if entry.exited() and not entry.remaining():
                    self._finish(key, entry)
                    return True
                entry.signal()
                return False

            leaks += not poll_until(gone, until, _POLL_SECONDS)
        if leaks:
            return leaks
        marker = _run_marker(self._token)
        if not poll_until(lambda: not _signal_carriers(marker), until, _POLL_SECONDS):
            leaks += 1
        return leaks


def _read(handle: IO[bytes]) -> str:
    handle.seek(0)
    return handle.read().decode("utf-8", errors="replace")


def _proc_entries() -> list[int]:
    return [int(entry) for entry in os.listdir("/proc") if entry.isdigit()]


def _carries(pid: int, marker: bytes) -> bool:
    try:
        with open(f"/proc/{pid}/environ", "rb") as handle:
            return marker in b"\0" + handle.read()
    except OSError:
        return False


def _describe(pid: int) -> str:
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as handle:
            argv = handle.read().split(b"\0")
    except OSError:
        return str(pid)
    return " ".join(os.fsdecode(arg) for arg in argv if arg) or str(pid)


def _marker(token: str) -> bytes:
    return f"\0{TOKEN_ENV}={token}\0".encode()


def _run_marker(run_token: str) -> bytes:
    return f"\0{TOKEN_ENV}={run_token}.".encode()


def _carrier_pids(marker: bytes) -> list[int]:
    """Return every live process other than this one whose environment holds ``marker``."""
    me = os.getpid()
    return [pid for pid in _proc_entries() if pid != me and _carries(pid, marker)]


def _token_pids(token: str) -> list[int]:
    """Return every live process whose environment carries one process's token."""
    return _carrier_pids(_marker(token))


def _group_pids(pgid: int) -> list[int]:
    """Return the live, non-zombie members of one process group."""
    pids: list[int] = []
    for pid in _proc_entries():
        try:
            with open(f"/proc/{pid}/stat", encoding="utf-8", errors="replace") as h:
                fields = h.read().rpartition(")")[2].split()
            if fields[0] != "Z" and int(fields[2]) == pgid:
                pids.append(pid)
        except (OSError, IndexError, ValueError):
            continue
    return pids


def signal_tokens(token: str) -> None:
    """SIGKILL every process carrying one process's token, each through its own pidfd.

    The token is checked again after the pidfd is open, so a PID that was
    listed but has since been reused by an unrelated process is never
    signaled.
    """
    _kill_carriers(_token_pids(token), _marker(token))


def _signal_carriers(marker: bytes) -> list[int]:
    """SIGKILL every process whose environment holds ``marker``; return those signaled."""
    return _kill_carriers(_carrier_pids(marker), marker)


def _kill_carriers(pids: Iterable[int], marker: bytes) -> list[int]:
    """SIGKILL each listed PID through a pidfd, only if it still holds ``marker``.

    Returns the PIDs signaled.
    """
    signaled: list[int] = []
    for pid in pids:
        try:
            fd = os.pidfd_open(pid)
        except OSError:
            continue
        try:
            if _carries(pid, marker):
                signal.pidfd_send_signal(fd, signal.SIGKILL)
                signaled.append(pid)
        except OSError:
            pass
        finally:
            os.close(fd)
    return signaled
