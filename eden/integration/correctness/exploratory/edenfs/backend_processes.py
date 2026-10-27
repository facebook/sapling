# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Owned child processes of the live backend: leases, directory streams, and kills.

Every child runs in its own process group registered with the environment's
process registry; ``_kill_process_group`` empties a group before returning.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from typing import Any, TYPE_CHECKING

from eden.integration.correctness.exploratory.core.containment import _runner_env
from eden.integration.correctness.exploratory.core.history import FAIL, OK
from eden.integration.correctness.exploratory.core.processes import (
    confirm_gone,
    kill_group,
    spawn,
)
from eden.integration.correctness.exploratory.core.runspec import Action
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    HarnessFailure,
    INVALID_BACKEND_RESPONSE,
)

if TYPE_CHECKING:
    from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend

_KILL_WAIT_SECONDS = 30.0

_BACKGROUND_READ = """
import ctypes, os, sys, time
if ctypes.CDLL(None, use_errno=True).prctl(1, 9) != 0:
    raise OSError(ctypes.get_errno(), 'parent-death signal')
if os.getppid() != int(sys.argv[3]):
    sys.exit(1)
def failed(error):
    raise error
deadline = time.monotonic() + float(sys.argv[2])
while time.monotonic() < deadline:
    for directory, directories, files in os.walk(sys.argv[1], onerror=failed):
        pass
    print('sweep', flush=True)
    time.sleep(0.01)
"""


# No parent-death signal: the scheduler thread that spawns the stream
# exits at the pause, and PDEATHSIG fires on thread exit. A closed stdin
# (runner gone) ends the pause instead, and unwind kills the group.
_READDIR_STREAM = """
import json, os, sys
fd = os.open(sys.argv[1], os.O_RDONLY | os.O_DIRECTORY)
def position():
    with open(f'/proc/self/fdinfo/{fd}') as info:
        return int(next(line for line in info if line.startswith('pos:')).split()[1])
entries = []
with os.scandir(fd) as stream:
    for entry in stream:
        entries.append(entry.name)
        if len(entries) == int(sys.argv[2]):
            break
    pos_at_pause = position()
    print(json.dumps({'paused_at': len(entries), 'pos_at_pause': pos_at_pause}), flush=True)
    if not sys.stdin.readline():
        sys.exit(2)
    pos_at_end = pos_at_pause
    for entry in stream:
        entries.append(entry.name)
        pos_at_end = position()
print(json.dumps({'entries': entries, 'pos_at_pause': pos_at_pause, 'pos_at_end': pos_at_end}), flush=True)
"""


def _kill_process_group(
    process: subprocess.Popen[Any], pgid: int, *, exited: bool = False
) -> bool:
    """SIGKILL one owned group, return only after it is empty, and say whether the leader had exited.

    ``exited`` says the leader was already reaped. A group already gone
    is success when ``exited`` or when ``process.poll()``, checked again
    after killpg found no group, shows the leader exited in between;
    otherwise it is a harness failure.
    """
    if not kill_group(pgid):
        if exited or process.poll() is not None:
            return True
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    if not confirm_gone(process, time.monotonic() + _KILL_WAIT_SECONDS):
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    return exited


def background_read(backend: LiveBackend, action: Action) -> BackendResult:
    """Keep one registered directory-walking process alive for the lease."""
    root = (backend._env.mount_path / action.payload["root"]).resolve(strict=True)
    if not root.is_relative_to(backend._env.mount_path.resolve()) or not root.is_dir():
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    with backend._reader_lock:
        if backend._readers_closed:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        process = spawn(
            backend._env.processes,
            action.id,
            [
                sys.executable,
                "-c",
                _BACKGROUND_READ,
                str(root),
                str(action.payload["seconds"]),
                str(os.getpid()),
            ],
            env=_runner_env(),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        backend._readers[action.id] = process
    try:
        stdout, stderr = process.communicate()
    finally:
        if process.poll() is None:
            _kill_process_group(process, process.pid)
        with backend._reader_lock:
            backend._readers.pop(action.id)
            backend._env.processes.retire(action.id, process)
    return BackendResult(
        OK if process.returncode == 0 else FAIL,
        {
            "pgid": process.pid,
            "returncode": process.returncode,
            "stdout": stdout,
            "stderr": stderr,
            "sweeps": stdout.count("sweep\n"),
        },
    )


def spawn_owned(
    backend: LiveBackend, action: Action, argv: list[str]
) -> subprocess.Popen[str]:
    """Start one registered child in its own group, pipes on every end."""
    with backend._reader_lock:
        if backend._readers_closed:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        process = spawn(
            backend._env.processes,
            action.id,
            [sys.executable, "-c", *argv],
            env=_runner_env(),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        backend._readers[action.id] = process
    return process


def readdir_stream(backend: LiveBackend, action: Action) -> BackendResult:
    """Open one directory stream and complete once its child has paused."""
    root = (backend._env.mount_path / action.payload["path"]).resolve(strict=True)
    if not root.is_relative_to(backend._env.mount_path.resolve()) or not root.is_dir():
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    process = spawn_owned(
        backend,
        action,
        [_READDIR_STREAM, str(root), str(action.payload["pause_after"])],
    )
    stdout = process.stdout
    line = stdout.readline() if stdout is not None else ""
    if not line:
        retire_stream(backend, action.id, process)
        return stream_died(backend, action.id, "stream exited before pausing", "")
    return BackendResult(OK, {"pgid": process.pid, **json.loads(line)})


def retire_stream(
    backend: LiveBackend, action_id: str, process: subprocess.Popen[str]
) -> None:
    if process.poll() is None:
        _kill_process_group(process, process.pid)
    with backend._reader_lock:
        backend._readers.pop(action_id)
        backend._env.processes.retire(action_id, process)


def stream_died(
    backend: LiveBackend, stream_id: str, reason: str, stderr: str
) -> BackendResult:
    """A dead stream is a FAIL only when a spec kill_process aimed at it."""
    killed_by_spec = any(
        other.kind == "kill_process" and other.payload["target_action"] == stream_id
        for other in backend._spec.actions
    )
    if not killed_by_spec:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    return BackendResult(FAIL, {"reason": reason, "stderr": stderr})


def readdir_resume(backend: LiveBackend, action: Action) -> BackendResult:
    """Release a paused stream and report the names it drained.

    The child reports the directory fd's ``pos`` at the pause and after
    the last drained entry (scandir rewinds an fd it exhausts); equal
    values mean the pause never straddled a getdents call, so the
    stream proves nothing and the run is a harness failure.
    """
    stream_id = action.payload["stream"]
    with backend._reader_lock:
        process = backend._readers.get(stream_id)
    if process is None:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    try:
        stdout, stderr = process.communicate("go\n")
    except (OSError, ValueError):
        stdout, stderr = "", ""
    finally:
        retire_stream(backend, stream_id, process)
    if process.returncode != 0 or not stdout:
        return stream_died(
            backend, stream_id, f"stream exited {process.returncode}", stderr
        )
    facts = json.loads(stdout)
    if facts["pos_at_end"] == facts["pos_at_pause"]:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    return BackendResult(OK, {"stream": stream_id, **facts})


def _retired_kill(target_id: str, pgid: int | None) -> BackendResult:
    """Return the kill of a target whose owner retired it; a target never spawned fails."""
    if pgid is None:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    return BackendResult(
        OK,
        {
            "target_action": target_id,
            "pgid": pgid,
            "group_empty": False,
            "already_exited": True,
        },
    )


def kill_process(backend: LiveBackend, action: Action) -> BackendResult:
    """Kill the target action's group after its spawn-relative delay.

    A target that already exited before the kill (``poll()`` is not None),
    or that exits before killpg reaches its group, is ``ok`` with
    ``already_exited: true``; any survivors in its group are still killed.
    An sl process is killed by its ``SaplingProcesses`` owner, which keeps
    its leader's one reap, so it is never polled here. A target its owner
    already retired is ``already_exited`` and is not signalled: its pgid may
    have been reused, so nothing checks its group (``group_empty: false``).
    """
    target_id = action.payload["target_action"]
    registry = backend._env.processes
    owned = registry.wait(target_id, backend._spec.budgets.runner_deadline_seconds)
    if owned is None:
        return _retired_kill(target_id, registry.finished(target_id))
    deadline = owned.spawned_at + action.payload["after_ms"] / 1000.0
    remaining = deadline - time.monotonic()
    if remaining > 0:
        time.sleep(remaining)
    current = registry.lookup(target_id)
    if current is None:
        return _retired_kill(target_id, registry.finished(target_id))
    if current != owned.pgid:
        raise HarnessFailure(INVALID_BACKEND_RESPONSE)
    already_exited = backend._sl.kill(target_id, _KILL_WAIT_SECONDS)
    if already_exited is None:
        already_exited = _kill_process_group(
            owned.process, owned.pgid, exited=owned.process.poll() is not None
        )
    facts: dict[str, Any] = {
        "target_action": target_id,
        "pgid": owned.pgid,
        "group_empty": True,
    }
    if already_exited:
        facts["already_exited"] = True
    return BackendResult(OK, facts)
