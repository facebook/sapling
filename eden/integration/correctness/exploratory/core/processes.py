# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Owned process groups: spawn one in its own session, kill it, and poll for it.

Every harness child that can be signaled runs as the leader of a new
session, so its PID is its process-group ID and one ``killpg`` reaches
everything it started in that group.
"""

from __future__ import annotations

import os
import signal
import subprocess
import time
from collections.abc import Callable, Sequence
from contextlib import suppress
from typing import Any, TYPE_CHECKING, TypeVar

if TYPE_CHECKING:
    from eden.integration.correctness.exploratory.core.target import ProcessRegistry

T = TypeVar("T")


def poll_until(predicate: Callable[[], T], deadline: float, interval: float) -> T:
    """Call ``predicate`` every ``interval`` seconds until it is truthy or ``deadline`` passes.

    Returns the first truthy value, or the falsy value of the call made at or
    after the deadline; ``predicate`` always runs at least once.
    """
    while True:
        value = predicate()
        if value or time.monotonic() >= deadline:
            return value
        time.sleep(interval)


def spawn(
    registry: ProcessRegistry, key: str, argv: Sequence[str], **popen: Any
) -> subprocess.Popen[Any]:
    """Start ``argv`` as a new session leader and publish its group under ``key``.

    A refused publish kills and reaps the group before the RuntimeError
    propagates, so no unpublished group is left running. Spawn errors
    (OSError, ValueError) propagate unchanged.
    """
    process = subprocess.Popen(argv, start_new_session=True, **popen)
    try:
        registry.publish(key, process, time.monotonic())
    except RuntimeError:
        kill_group(process.pid)
        process.wait()
        raise
    return process


def kill_group(pgid: int) -> bool:
    """SIGKILL one owned process group; return False when it was already gone.

    A pgid <= 1 is never an owned group (0 is the caller's own group, 1 is
    init's), so it is refused before any signal.
    """
    if int(pgid) <= 1:
        raise ValueError(f"refusing to signal process group {pgid!r}")
    try:
        os.killpg(pgid, signal.SIGKILL)
    except ProcessLookupError:
        return False
    return True


def alive(pid: int, *, group: bool = False) -> bool:
    """Return whether a process (or, with ``group``, any member of a group) still exists."""
    try:
        (os.killpg if group else os.kill)(pid, 0)
    except ProcessLookupError:
        return False
    return True


def confirm_gone(
    process: subprocess.Popen[Any], deadline: float, interval: float = 0.01
) -> bool:
    """Reap a killed group's leader, then wait until the group is empty, both by ``deadline``.

    Returns False when the leader or any member is still there at the deadline.
    """
    with suppress(subprocess.TimeoutExpired):
        process.wait(timeout=max(0.0, deadline - time.monotonic()))
    if process.returncode is None:
        return False
    return poll_until(lambda: not alive(process.pid, group=True), deadline, interval)
