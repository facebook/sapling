# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Subprocess containment for exploratory runs: the supervisor owns the artifact
dir, writes run-spec.json before spawn, and streams stdout JSONL to a
flushed history.jsonl until the runner's final record or the runner
deadline. It then SIGKILLs the runner and everything the run started,
unmounts and removes the run dir, appends a `run-closed` record once no
process of the run is left, and always writes execution.json and
replay.json, then verdict.json (the canonical verdict document for the
durable prefix, bound to the final execution record). Stdlib only
otherwise: no EdenFS/namespaces/Scuba.
"""

from __future__ import annotations

import ctypes
import faulthandler
import fcntl
import json
import math
import os
import re
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import traceback
from collections import deque
from collections.abc import Callable, Iterable, Mapping
from contextlib import redirect_stdout
from dataclasses import asdict, dataclass, replace
from typing import Any, TextIO

from eden.integration.correctness.exploratory.core import jsonio
from eden.integration.correctness.exploratory.core.checker import (
    check,
    encode_verdict,
    registry,
    semantic_digest,
    Verdict,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    DAEMON_EXIT,
    Event,
    event_from_json,
    event_line,
    event_to_json,
    FINAL_CLEANUP_KEYS,
    INFO,
    MAX_EVENT_BYTES,
    RUN_CLOSED,
)
from eden.integration.correctness.exploratory.core.processes import (
    kill_group,
    poll_until,
)
from eden.integration.correctness.exploratory.core.runspec import (
    from_json,
    RunSpec,
    SpecError,
    to_json,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    bounded_error,
    BROKEN_HISTORY_TRANSPORT,
    HARNESS_REASONS,
    run_actions,
)
from eden.integration.correctness.exploratory.core.target import (
    EnvironmentDegraded,
    RUN_DIR_ENV,
    Target,
)

RUN_SPEC_NAME = "run-spec.json"
HISTORY_NAME = "history.jsonl"
EXECUTION_NAME = "execution.json"
VERDICT_NAME = "verdict.json"
REPLAY_NAME = "replay.json"
STDERR_NAME = "stderr.log"
# Run dirs are `tempfile.mkdtemp(prefix=RUN_DIR_PREFIX)` in the default temp dir.
RUN_DIR_PREFIX = "exploratory."
# Held with a shared flock by the supervisor from before spawn until its close
# of the run dir, and by the runner for its lifetime; free only when both died.
RUN_LOCK_NAME = "run.lock"
RUNNER_DEADLINE_ENV = "EDEN_EXPLORATORY_RUNNER_DEADLINE"

COMPLETED = "completed"
HARNESS_FAILURE = "harness-failure"
CONTAINMENT_TIMEOUT = "containment-timeout"
ENVIRONMENT_DEGRADED = "environment-degraded"
ENVIRONMENT_DEGRADED_EXIT = 75
SETUP_ERROR_PREFIX = "runner: setup-error: "
SETUP_ERROR_TAIL_LINES = 40
SETUP_ERROR_MESSAGE_CHARS = 4000
# Cleanup text begins with this when the run's own FUSE connection had to be aborted.
MOUNT_WEDGED_PREFIX = "supervisor: mount-wedged: "
# Reason prefix of a run whose unwind or run-dir close left something behind.
CLEANUP_FAILED = "cleanup-failed: "
# prctl(2) option that delivers a signal to the caller when its parent thread exits.
_PR_SET_PDEATHSIG = 1
_MOUNTINFO = "/proc/self/mountinfo"
_FUSE_CONNECTIONS = "/sys/fs/fuse/connections"
_PROC = "/proc"
# SIGKILL cannot be caught, so the runner's exit always follows the kill; this
# bound only stops a supervisor waiting on a runner wedged in the kernel.
_REAP_GUARD_SECONDS = 60.0
# How long the supervisor reads what a runner without a final record already wrote.
_DRAIN_SECONDS = 0.1


@dataclass(frozen=True)
class Execution:
    """Final containment state for one exploration."""

    status: str
    reason: str
    events: int
    returncode: int | None
    runner_phase: str | None = None
    cleanup: str | None = None
    setup: dict[str, Any] | None = None
    daemon_exit: dict[str, Any] | None = None
    setup_error: dict[str, Any] | None = None
    backend_error: str | None = None


@dataclass(frozen=True)
class RunDirClose:
    """What `close_run_dir` did.

    `killed`: the kill scan left no process of the run alive. `leaked`: a
    process, a mount, or the dir itself outlived the close.
    """

    text: str
    killed: bool
    leaked: bool


@dataclass(frozen=True)
class ExplorationResult:
    """Artifacts of one run: execution state, checker verdict, and dir."""

    execution: Execution
    verdict: Verdict
    dir: str


class _StdoutSink:
    """History sink that prints one JSON object per line, flushed per line."""

    def __init__(self, stream: TextIO) -> None:
        self._stream = stream
        self._lock = threading.Lock()

    def emit(self, event: Event) -> None:
        line = event_line(event).decode("utf-8")
        with self._lock:
            self._stream.write(line)
            self._stream.flush()


def _setup_environment(environment: Any, sink: _StdoutSink) -> int | None:
    """Set up one target, or emit its typed degradation as the final record and stop.

    What setup started is left to the supervisor's run-dir close.
    """
    try:
        environment.setup()
    except EnvironmentDegraded as error:
        sink.emit(
            Event(
                type=CLEANUP,
                time=time.monotonic(),
                data={
                    "unwind": {"setup": error.facts},
                    "environment_degraded": error.reason,
                },
            )
        )
        print(
            f"runner: environment-degraded: {error.reason}",
            file=sys.stderr,
            flush=True,
        )
        return ENVIRONMENT_DEGRADED_EXIT
    except Exception as error:  # noqa: BLE001 - recorded in stderr.log
        print(traceback.format_exc(), end="", file=sys.stderr)
        print(
            f"runner: setup-failed: {type(error).__name__}: {error}",
            file=sys.stderr,
        )
        record = {
            "type": type(error).__name__,
            "message": str(error)[:SETUP_ERROR_MESSAGE_CHARS],
        }
        print(
            SETUP_ERROR_PREFIX + json.dumps(record, sort_keys=True),
            file=sys.stderr,
            flush=True,
        )
        return 1
    return None


def _mark_runner_phase(phase: str) -> None:
    """Write a flushed phase marker to the runner's stderr stream."""
    print(f"runner-phase: {phase}", file=sys.stderr, flush=True)


def _enable_fault_handler() -> None:
    """Send fatal Python signal tracebacks to the runner's stderr descriptor."""
    try:
        faulthandler.enable(file=sys.stderr, all_threads=True)
    except (AttributeError, OSError, ValueError):
        # In-process tests may replace stderr with a stream without fileno().
        pass


def runner_argv_for(
    spec_path: str, target_name: str, red_control: str | None = None
) -> list[str]:
    """Supervisor spawn vector: sys.executable plus the same par in runner mode."""
    entry = os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "cli.py"
    )
    if not os.path.isfile(entry):
        entry = os.path.abspath(sys.argv[0])
    argv = [
        sys.executable,
        entry,
        "--runner",
        "--target",
        target_name,
        "--spec",
        spec_path,
    ]
    if red_control is not None:
        argv += ["--red-control", red_control]
    return argv


def _die_with_spawning_thread() -> Callable[[], None]:
    """Return a pre-exec hook that SIGKILLs the child when the spawning thread exits.

    The hook runs in the child between fork and exec: it sets
    PR_SET_PDEATHSIG(SIGKILL), then exits at once if the parent already
    died before the setting took effect. The signal fires when the thread
    that forked exits, not the process, so that thread must outlive the
    child; `run_exploration` spawns and supervises the runner on one
    thread and reaps it before returning. A setuid exec clears the setting.
    """
    prctl = ctypes.CDLL(None, use_errno=True).prctl
    parent = os.getpid()

    def hook() -> None:
        if prctl(_PR_SET_PDEATHSIG, signal.SIGKILL, 0, 0, 0) != 0:
            os._exit(127)
        if os.getppid() != parent:
            os._exit(127)

    return hook


def _runner_env() -> dict[str, str]:
    """Runner env with this module's eden/ root prepended to PYTHONPATH.

    The par runtime scrubs LD_LIBRARY_PATH from os.environ after startup,
    so rebuild the runtime lib dir (sibling of sys.executable's directory)
    for the runner: without it the runner binary cannot load its native
    extensions and dies with exit 127 before running any Python.
    """
    root = os.path.abspath(__file__)
    for _ in range(__name__.count(".") + 1):
        root = os.path.dirname(root)
    env = dict(os.environ)
    env.pop(RUNNER_DEADLINE_ENV, None)
    previous = env.get("PYTHONPATH")
    env["PYTHONPATH"] = root if not previous else root + os.pathsep + previous
    bin_dir = os.path.dirname(os.path.abspath(sys.executable))
    lib_dir = os.path.normpath(os.path.join(bin_dir, "..", "lib"))
    if os.path.isdir(lib_dir):
        prior_lib = env.get("LD_LIBRARY_PATH")
        env["LD_LIBRARY_PATH"] = (
            lib_dir if not prior_lib else lib_dir + os.pathsep + prior_lib
        )
    return env


def _mounts_under(run_dir: str) -> list[str]:
    """Return /proc/mounts mount points beneath run_dir, deepest first."""
    prefix = run_dir.rstrip("/") + "/"
    with open("/proc/mounts") as handle:
        points = [line.split(" ")[1] for line in handle]
    return sorted((point for point in points if point.startswith(prefix)), reverse=True)


def _pids_owning(run_dir: str) -> list[int]:
    """Return live pids, other than ours, whose environment carries this run's RUN_DIR_ENV.

    The runner gets RUN_DIR_ENV at spawn and its daemon and helpers inherit
    it; a process that only names run_dir (an inspector's `tail -f`) does not.
    """
    marker, own_pid = os.fsencode(f"{RUN_DIR_ENV}={run_dir}"), os.getpid()
    pids: list[int] = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit() or int(entry) == own_pid:
            continue
        try:
            with open(f"/proc/{entry}/environ", "rb") as handle:
                if marker in handle.read().split(b"\0"):
                    pids.append(int(entry))
        except OSError:
            pass
    return pids


def _start_time(pid: int) -> str | None:
    """Return a live process's start time, or None once each of its threads is a zombie or gone.

    A killed process's leader can be a zombie while another of its threads
    still waits in the kernel, holding its files.
    """
    try:
        with open(f"{_PROC}/{pid}/stat") as handle:
            fields = handle.read().rpartition(")")[2].split()
        tids = os.listdir(f"{_PROC}/{pid}/task")
    except OSError:
        return None
    for tid in tids:
        try:
            with open(f"{_PROC}/{pid}/task/{tid}/stat") as handle:
                state = handle.read().rpartition(")")[2].split()[:1]
        except OSError:
            continue
        if state not in (["Z"], ["X"]) and len(fields) > 19:
            return fields[19]
    return None


def _killed_all(run_dir: str, refused: set[int], signaled: dict[int, str]) -> bool:
    """SIGKILL the run's processes; True when none is left. Adds refusals to `refused`.

    A signaled process counts until it is a zombie or gone: an exiting
    process loses its environment, so `_pids_owning` stops finding it,
    before it releases its files and the mounts they hold. `signaled`
    maps each signaled pid to its start time, so a reused pid never counts.
    """
    pids = [pid for pid in _pids_owning(run_dir) if pid not in refused]
    for pid in pids:
        started = _start_time(pid)
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            continue
        except PermissionError:
            refused.add(pid)
            continue
        if started is not None:
            signaled[pid] = started
    exiting = [pid for pid, started in signaled.items() if _start_time(pid) == started]
    return not pids and not exiting


def _unescape_mount_field(field: str) -> str:
    return re.sub(r"\\([0-7]{3})", lambda match: chr(int(match.group(1), 8)), field)


def _fuse_connections_under(run_dir: str) -> list[tuple[str, str]]:
    """Return (mount point, FUSE connection id) of each FUSE mount at or beneath run_dir.

    The connection id is the anonymous device's minor number in
    mountinfo; a FUSE mount always has major 0, so any other major is
    skipped rather than mapped to a connection.
    """
    prefix = run_dir.rstrip("/") + "/"
    found: list[tuple[str, str]] = []
    with open(_MOUNTINFO) as handle:
        for line in handle:
            fields, _, tail = line.partition(" - ")
            parts, fstype = fields.split(), tail.split()[:1]
            if len(parts) < 5 or not fstype or not fstype[0].startswith("fuse"):
                continue
            major, _, minor = parts[2].partition(":")
            point = _unescape_mount_field(parts[4])
            if major == "0" and minor.isdigit():
                if point == run_dir.rstrip("/") or point.startswith(prefix):
                    found.append((point, minor))
    return found


def _blocked_threads(pids: list[int]) -> list[str]:
    """Return ``pid/tid:wchan`` for each thread of ``pids`` in uninterruptible sleep."""
    blocked: list[str] = []
    for pid in pids:
        try:
            tids = sorted(os.listdir(f"{_PROC}/{pid}/task"), key=int)
        except (OSError, ValueError):
            continue
        for tid in tids:
            task = f"{_PROC}/{pid}/task/{tid}"
            try:
                with open(f"{task}/stat") as handle:
                    state = handle.read().rpartition(")")[2].split()[:1]
                with open(f"{task}/wchan") as handle:
                    wchan = handle.read().strip()
            except OSError:
                continue
            if state == ["D"]:
                blocked.append(f"{pid}/{tid}:{wchan}")
    return blocked


def _abort_own_connections(run_dir: str, signaled: Iterable[int]) -> str | None:
    """Abort the FUSE connection of each mount under run_dir; describe what was stuck.

    Only connections whose mount point lies under run_dir are aborted,
    never another mount's. Returns None when no such connection exists;
    otherwise each connection's ``waiting`` request count and the D-state
    threads of the run and of the `signaled` pids, read before the abort.
    """
    try:
        connections = _fuse_connections_under(run_dir)
    except OSError:
        return None
    if not connections:
        return None
    pids = sorted(set(signaled).union(_pids_owning(run_dir)))
    stuck = [f"blocked threads {_blocked_threads(pids)}"]
    for point, connection in connections:
        directory = os.path.join(_FUSE_CONNECTIONS, connection)
        try:
            with open(os.path.join(directory, "waiting")) as handle:
                waiting = handle.read().strip()
            with open(os.path.join(directory, "abort"), "w") as handle:
                handle.write("1")
        except OSError as error:
            stuck.append(f"{point} connection {connection}: abort failed: {error}")
            continue
        stuck.append(f"{point} connection {connection} waiting={waiting} aborted")
    return "; ".join(stuck)


def close_run_dir(run_dir: str) -> RunDirClose:  # noqa: C901
    """SIGKILL, unmount, and remove what the runner left under run_dir.

    Never raises. The kill scan always runs, even when run_dir is already
    gone ("runner"). A process that refuses SIGKILL or outlives the scan
    keeps the dir and its mounts ("kept"). EDEN_TEST_CLEANUP=no keeps the
    dir, its mounts, and its processes for inspection, so nothing is killed.

    A process blocked on the run's own FUSE mount survives SIGKILL and a
    busy mount refuses unmount; either way the run's own FUSE connection
    is aborted once, what was stuck is recorded after
    ``MOUNT_WEDGED_PREFIX``, and the kill or unmount is retried.
    """
    cleanup_mode = os.environ.get("EDEN_TEST_CLEANUP", "always").lower()
    if cleanup_mode in ("0", "no", "false"):
        return RunDirClose(
            f"supervisor: kept (EDEN_TEST_CLEANUP={cleanup_mode})", False, False
        )
    refused: set[int] = set()
    signaled: dict[int, str] = {}
    wedged: str | None = None

    def killed_all() -> bool:
        # A dying daemon still holds its mount busy, so wait until none is left.
        return poll_until(
            lambda: _killed_all(run_dir, refused, signaled),
            time.monotonic() + 10.0,
            0.1,
        )

    killed = killed_all()
    if not killed and not refused:
        wedged = _abort_own_connections(run_dir, signaled)
        killed = wedged is not None and killed_all()
    prefix = "supervisor: " if wedged is None else f"{MOUNT_WEDGED_PREFIX}{wedged}; "
    if refused or not killed:
        left = sorted(refused.union(_pids_owning(run_dir)))
        return RunDirClose(f"{prefix}cannot kill {left}; kept", False, True)
    if not os.path.lexists(run_dir):
        return RunDirClose(
            "runner" if wedged is None else f"{prefix}runner", True, False
        )
    try:
        mounts = _mounts_under(run_dir)
        for mount in mounts:
            result = _fusermount(mount)
            if result.returncode != 0 and wedged is None:
                wedged = _abort_own_connections(run_dir, signaled)
                if wedged is not None:
                    prefix = f"{MOUNT_WEDGED_PREFIX}{wedged}; "
                    result = _fusermount(mount)
            if result.returncode != 0 and mount in _mounts_under(run_dir):
                text = f"{prefix}fusermount -u {mount} failed: {result.stderr.strip()}"
                return RunDirClose(text, True, True)
    except OSError as error:
        return RunDirClose(f"{prefix}unmount failed: {error}", True, True)
    try:
        shutil.rmtree(run_dir)
    except OSError as error:
        text = f"{prefix}unmounted {len(mounts)}, remove failed: {error}"
        return RunDirClose(text, True, True)
    return RunDirClose(f"{prefix}unmounted {len(mounts)}, removed", True, False)


def hold_run_lock(run_dir: str) -> int:
    """Take a shared flock on run_dir's lock file; return its close-on-exec fd.

    The kernel drops the lock when the last holder's fd closes, including
    when the holder dies; children never inherit it.
    """
    fd = os.open(
        os.path.join(run_dir, RUN_LOCK_NAME), os.O_RDWR | os.O_CREAT | os.O_CLOEXEC
    )
    try:
        fcntl.flock(fd, fcntl.LOCK_SH)
    except OSError:
        os.close(fd)
        raise
    return fd


def sweep_orphaned_run_dirs() -> dict[str, str]:
    """Close every run dir in the temp dir whose lock no live process holds.

    Returns each swept dir's `close_run_dir` text. A dir whose lock is held
    belongs to a live run and is never touched. A dir without a lock file
    is left alone: its supervisor died before spawning, or it is not a run
    dir. The sweeper holds the lock exclusively while it closes the dir, so
    concurrent sweeps never close one dir twice.
    """
    root = tempfile.gettempdir()
    swept: dict[str, str] = {}
    for name in sorted(os.listdir(root)):
        if not name.startswith(RUN_DIR_PREFIX):
            continue
        run_dir = os.path.join(root, name)
        try:
            fd = os.open(os.path.join(run_dir, RUN_LOCK_NAME), os.O_RDWR | os.O_CLOEXEC)
        except OSError:
            continue
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            os.close(fd)
            continue
        try:
            swept[run_dir] = close_run_dir(run_dir).text
        finally:
            os.close(fd)
    return swept


def _fusermount(mount: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["fusermount", "-u", mount], capture_output=True, text=True)


def _leaks(record: Mapping[str, object]) -> int:
    """Return the leak count of the final record's unwind summary; a malformed count is one leak."""
    unwind = record.get("unwind")
    leaks = unwind.get("leaks", 0) if isinstance(unwind, dict) else 0
    return leaks if type(leaks) is int else 1


def _private_transport() -> TextIO:
    """Move the history transport off fd 1 and point fd 1 at stderr.

    The transport moves to a private descriptor that children do not
    inherit, so a child that writes to its inherited stdout (an `sl commit`
    run with stdout=None) reaches stderr.log, never the history. A
    replaced sys.stdout without a descriptor (an in-process caller) is the
    transport as is.
    """
    try:
        fd = sys.stdout.fileno()
    except (AttributeError, OSError, ValueError):
        return sys.stdout
    sys.stdout.flush()
    transport = os.dup(fd)
    os.dup2(sys.stderr.fileno(), fd)
    # The transport stays open until the process exits: closing it early
    # would end the supervisor's stream before the runner has exited.
    return open(transport, "w", encoding="utf-8", closefd=False)


def runner_main(  # noqa: C901
    spec_path: str, target: Target, red_control: str | None = None
) -> int:
    """Run one target environment and backend; stdout stays pure JSONL.

    The runner never closes what it started. Its final cleanup record (the
    scheduler status and unwind summary, or the setup degradation) ends
    the run: the supervisor stops reading there and closes the run dir, so
    nothing the runner writes later is recorded.
    """
    # The loader read LD_LIBRARY_PATH (`_runner_env`'s runtime lib dir) at
    # startup; the runner's children must not load their libraries from it.
    os.environ.pop("LD_LIBRARY_PATH", None)
    _enable_fault_handler()
    run_dir = os.environ.get(RUN_DIR_ENV)
    if run_dir is not None:
        try:
            hold_run_lock(run_dir)
        except OSError as error:
            print(f"runner: run lock: {error}", file=sys.stderr)
            return 2
    _mark_runner_phase("spawned")
    try:
        with open(spec_path) as handle:
            spec = from_json(handle.read(), target)
    except (OSError, SpecError) as error:
        print(f"runner: {error}", file=sys.stderr)
        return 2
    if red_control is not None and red_control not in target.red_controls:
        print(
            f"runner: unknown red_control {red_control!r}",
            file=sys.stderr,
            flush=True,
        )
        return 2
    try:
        deadline = float(
            os.environ.pop(
                RUNNER_DEADLINE_ENV,
                time.monotonic() + spec.budgets.runner_deadline_seconds,
            )
        )
        if not math.isfinite(deadline):
            raise ValueError("nonfinite runner deadline")
    except ValueError as error:
        print(f"runner: invalid deadline: {error}", file=sys.stderr)
        return 2
    sink = _StdoutSink(_private_transport())
    with redirect_stdout(sys.stderr):
        try:
            environment = target.make_environment(spec, sink)
        except Exception as error:  # noqa: BLE001 - recorded in stderr.log
            print(
                f"runner: environment-failed: {type(error).__name__}: {error}",
                file=sys.stderr,
                flush=True,
            )
            return 1
        _mark_runner_phase("setup-begin")
        setup_exit = _setup_environment(environment, sink)
        if setup_exit is not None:
            return setup_exit
        _mark_runner_phase("setup-done")
        try:
            backend = target.make_backend(environment, spec, red_control)
        except Exception as error:  # noqa: BLE001 - recorded in stderr.log
            print(traceback.format_exc(), end="", file=sys.stderr)
            print(
                f"runner: backend-failed: {type(error).__name__}: {error}",
                file=sys.stderr,
                flush=True,
            )
            return 1
        outcome: dict[str, object] = {}

        def _unwind() -> None:
            unwind = getattr(backend, "unwind_faults", None) or backend.unwind
            outcome["unwind"] = unwind(sink, time.monotonic)

        _mark_runner_phase("actions-begin")
        status = run_actions(
            spec,
            backend,
            sink,
            time.monotonic,
            unwind=_unwind,
            runner_deadline=deadline,
        )
        sink.emit(
            Event(
                type=CLEANUP,
                time=time.monotonic(),
                data={**outcome, "scheduler": asdict(status)},
            )
        )
    if status.status != COMPLETED:
        print(
            f"runner: harness-failure: {status.reason or 'unknown'}",
            file=sys.stderr,
            flush=True,
        )
        return 1
    if _leaks(outcome) > 0:
        print(
            f"runner: cleanup-failed: unwind={outcome.get('unwind')}",
            file=sys.stderr,
            flush=True,
        )
        return 1
    return 0


def load_history(path: str) -> list[Event]:
    """Parse a history.jsonl file; raise SpecError on the first bad line.

    A repeated key or a non-finite number is a bad line.
    """
    events: list[Event] = []
    with open(path, "rb") as handle:
        for lineno, line in enumerate(handle, 1):
            try:
                events.append(event_from_json(jsonio.loads(line.decode("utf-8"))))
            except ValueError as error:
                raise SpecError(f"{path}:{lineno}: bad line: {error}") from error
    return events


def durable_history(path: str | os.PathLike[str]) -> list[Event]:
    """Return the durable prefix of a contained run's history.jsonl, never raising on content.

    These are exactly the events containment ingested and bound into
    replay.json and verdict.json: each line in order while it is strict
    UTF-8, strict JSON, and a well-formed event. The first line that is not
    (a torn record, a record cut off at the runner deadline, invalid UTF-8,
    or stray output) and everything after it is the untrusted tail, kept as
    raw evidence and never read as events. Consumers of run artifacts use
    this reader, so a broken tail never aborts a campaign, an admission, or
    a findings pass.
    """
    with open(path, "rb") as handle:
        data = handle.read()
    lines = data.split(b"\n")
    if lines and not lines[-1]:
        lines.pop()
    events: list[Event] = []
    for line in lines:
        try:
            events.append(event_from_json(jsonio.loads(line.decode("utf-8"))))
        except ValueError:
            break
    return events


def load_replay(path: str, target: Target) -> tuple[RunSpec, list[Event]]:
    """Load a replay.json bundle back into a spec plus durable events.

    A repeated key or a non-finite number anywhere in the bundle is refused.
    """
    try:
        with open(path) as handle:
            doc = jsonio.loads(handle.read())
        spec = from_json(json.dumps(doc["spec"]), target)
        events = [event_from_json(item) for item in doc["history"]]
    except (KeyError, TypeError, ValueError) as error:
        raise SpecError(f"bad replay file {path!r}: {error}") from error
    return spec, events


def _ingest(history_file: TextIO, durable: list[Event], line: str) -> bool:
    """Append one stdout line verbatim (flushed); True if it parses strictly."""
    history_file.write(line if line.endswith("\n") else line + "\n")
    history_file.flush()
    try:
        durable.append(event_from_json(jsonio.loads(line)))
    except ValueError:
        return False
    return True


def _append_run_closed(history_path: str) -> Event:
    """Durably append the supervisor's `run-closed` record to history.jsonl and return it.

    Its CLOCK_MONOTONIC time is read after every process of the run died,
    so it is no earlier than any runner event on this host.
    """
    event = Event(type=RUN_CLOSED, time=time.monotonic())
    with open(history_path, "ab") as handle:
        handle.write(event_line(event))
        handle.flush()
        os.fsync(handle.fileno())
    return event


def _setup_facts(events: list[Event]) -> dict[str, Any] | None:
    """Return optional target setup facts from the durable cleanup event."""
    for event in reversed(events):
        if event.type != CLEANUP:
            continue
        unwind = event.data.get("unwind")
        if type(unwind) is dict and type(unwind.get("setup")) is dict:
            return dict(unwind["setup"])
        break
    return None


def _daemon_exit(events: list[Event]) -> dict[str, Any] | None:
    """Return the first durable daemon-exit report, if the target emitted one."""
    for event in events:
        if event.type == INFO and event.data.get("reason") == DAEMON_EXIT:
            return {
                "time": event.time,
                **{
                    key: event.data.get(key)
                    for key in ("returncode", "signal", "log_tail", "core_path")
                },
            }
    return None


def _backend_error(events: list[Event]) -> str | None:
    """Return the scheduler record's bounded backend exception text, if any."""
    for event in reversed(events):
        scheduler = event.data.get("scheduler") if event.type == CLEANUP else None
        if isinstance(scheduler, dict) and type(scheduler.get("error")) is str:
            return bounded_error(scheduler["error"])
    return None


def _is_final(event: Event) -> bool:
    """Return whether `event` is the runner's final cleanup record."""
    return event.type == CLEANUP and not FINAL_CLEANUP_KEYS.isdisjoint(event.data)


def _final_status(record: Event) -> tuple[str, str]:
    """Classify a run from the runner's final cleanup record."""
    degraded = record.data.get("environment_degraded")
    if type(degraded) is str:
        return ENVIRONMENT_DEGRADED, degraded
    scheduler = record.data.get("scheduler")
    if not isinstance(scheduler, dict):
        return HARNESS_FAILURE, "runner-failed: final record has no scheduler status"
    if scheduler.get("status") == COMPLETED:
        leaks = _leaks(record.data)
        if leaks > 0:
            return HARNESS_FAILURE, f"{CLEANUP_FAILED}unwind reported {leaks} leak(s)"
        return COMPLETED, "ok"
    reason = scheduler.get("reason")
    if type(reason) is str and reason in HARNESS_REASONS:
        return HARNESS_FAILURE, reason
    return HARNESS_FAILURE, f"runner-failed: scheduler reported {scheduler!r}"


def _runner_exit_status(events: list[Event], returncode: int | None) -> tuple[str, str]:
    """Classify a runner that exited before writing its final record."""
    if returncode == 0:
        return COMPLETED, "ok"
    runner_reasons = {
        event.data["reason"]
        for event in events
        if event.type == INFO and type(event.data.get("reason")) is str
    }
    detail = f": {'; '.join(sorted(runner_reasons))}" if runner_reasons else ""
    return HARNESS_FAILURE, f"runner-failed: runner exited {returncode}{detail}"


def _last_runner_phase(stderr_path: str) -> str | None:
    """Return the last valid phase marker emitted by the runner."""
    phases = {"spawned", "setup-begin", "setup-done", "actions-begin"}
    try:
        with open(stderr_path) as stderr_file:
            seen = [
                line.strip().removeprefix("runner-phase: ")
                for line in stderr_file
                if line.startswith("runner-phase: ")
            ]
    except OSError:
        return None
    return next((phase for phase in reversed(seen) if phase in phases), None)


def _setup_error(stderr_path: str) -> dict[str, Any] | None:
    """Return the runner's last setup-error record plus the stderr.log tail."""
    record: dict[str, Any] | None = None
    tail: deque[str] = deque(maxlen=SETUP_ERROR_TAIL_LINES)
    try:
        with open(stderr_path, errors="replace") as stderr_file:
            for line in stderr_file:
                line = line.rstrip("\n")
                tail.append(line)
                if line.startswith(SETUP_ERROR_PREFIX):
                    try:
                        parsed = json.loads(line.removeprefix(SETUP_ERROR_PREFIX))
                    except ValueError:
                        continue
                    if type(parsed) is dict:
                        record = parsed
    except OSError:
        return None
    if record is None:
        return None
    return {**record, "log_tail": list(tail)}


@dataclass(frozen=True)
class _Stream:
    """What supervision read: the durable prefix, the final record, and the stream's status.

    `intact`: history.jsonl holds exactly the durable prefix.
    """

    durable: list[Event]
    final: Event | None
    status: str
    reason: str
    intact: bool


def _supervise(  # noqa: C901 - stream and containment share one durable prefix
    spec: RunSpec,
    proc: subprocess.Popen[bytes],
    history_file: TextIO,
    runner_deadline: float,
) -> _Stream:
    """Stream stdout to history.jsonl until the final record, EOF, or the deadline.

    Nothing after the runner's final record is read, since a runner thread
    blocked in a call can hold the transport open. The runner is killed
    and reaped after supervision, by `close_run_dir` with the rest of the run.
    """
    deadline = spec.budgets.runner_deadline_seconds
    assert proc.stdout is not None
    stdout = proc.stdout
    fd = stdout.fileno()
    os.set_blocking(fd, False)
    transport_broken = False
    buffered = b""
    durable: list[Event] = []
    final: Event | None = None
    status, reason = COMPLETED, "ok"

    def preserve_untrusted() -> None:
        nonlocal buffered
        history_file.write(buffered.decode("utf-8", errors="surrogateescape"))
        history_file.flush()
        buffered = b""

    def consume(data: bytes) -> bool:
        nonlocal buffered, final, status, reason, transport_broken
        buffered += data
        if transport_broken:
            preserve_untrusted()
            return False
        while b"\n" in buffered:
            line, buffered = buffered.split(b"\n", 1)
            try:
                text = line.decode("utf-8") + "\n"
            except UnicodeDecodeError:
                history_file.write(
                    line.decode("utf-8", errors="surrogateescape") + "\n"
                )
                history_file.flush()
                problem = f"invalid UTF-8: {line!r}"
            else:
                if _ingest(history_file, durable, text):
                    if _is_final(durable[-1]):
                        final, buffered = durable[-1], b""
                        return False
                    continue
                problem = repr(text.strip())
            transport_broken = True
            if status == COMPLETED:
                status = HARNESS_FAILURE
                reason = f"{BROKEN_HISTORY_TRANSPORT}: {problem}"
            preserve_untrusted()
            return False
        if len(buffered) > MAX_EVENT_BYTES:
            transport_broken = True
            if status == COMPLETED:
                status = HARNESS_FAILURE
                reason = (
                    f"{BROKEN_HISTORY_TRANSPORT}: line exceeds {MAX_EVENT_BYTES} bytes"
                )
            preserve_untrusted()
            return False
        return True

    try:
        while True:
            remaining = runner_deadline - time.monotonic()
            if remaining <= 0 or not select.select([fd], [], [], remaining)[0]:
                status, reason = CONTAINMENT_TIMEOUT, f"runner exceeded {deadline}s"
                break
            data = os.read(fd, 65536)
            if not data or not consume(data):
                break
    finally:
        drain_deadline = time.monotonic() + _DRAIN_SECONDS
        while final is None and time.monotonic() < drain_deadline:
            try:
                data = os.read(fd, 65536)
            except BlockingIOError:
                break
            if not data:
                break
            consume(data)
        if buffered:
            consume(b"\n")
        stdout.close()
        history_file.flush()
        os.fsync(history_file.fileno())
        history_file.close()
    return _Stream(durable, final, status, reason, not transport_broken)


def _reap(proc: subprocess.Popen[bytes]) -> int | None:
    """Wait for the killed runner; None only if it outlived the reap guard."""
    try:
        return proc.wait(timeout=_REAP_GUARD_SECONDS)
    except subprocess.TimeoutExpired:
        return None


def _execution(stream: _Stream, returncode: int | None, stderr_path: str) -> Execution:
    """Classify a supervised run: by its final record, else by the runner's exit.

    A runner killed after its final record has returncode None.
    """
    durable, status, reason = stream.durable, stream.status, stream.reason
    if stream.final is not None:
        returncode = None
        if status == COMPLETED:
            status, reason = _final_status(stream.final)
    elif status == COMPLETED and returncode != 0:
        status, reason = _runner_exit_status(durable, returncode)
    return Execution(
        status,
        reason,
        len(durable),
        returncode,
        runner_phase=_last_runner_phase(stderr_path),
        setup=_setup_facts(durable),
        daemon_exit=_daemon_exit(durable),
        setup_error=_setup_error(stderr_path) if returncode != 0 else None,
        backend_error=_backend_error(durable),
    )


def judge(
    target: Target, spec: RunSpec, events: list[Event], execution: Execution
) -> Verdict:
    """Return the verdict for one run, bound to its final execution record.

    The execution record only binds the verdict and decides whether every
    action must be resolved; a property failure never changes it.
    """
    return check(target, spec, events, execution=asdict(execution))


def preflight(target: Target) -> None:
    """Fail before anything runs when the target's verdict could not be bound.

    Raises when the combined registry is invalid or a module that can change
    a verdict has no readable source.
    """
    registry(target)
    semantic_digest(target)


def _write_durable(path: str, data: bytes) -> None:
    """Write a whole file and fsync it before returning."""
    with open(path, "wb") as handle:
        handle.write(data)
        handle.flush()
        os.fsync(handle.fileno())


def _write_execution_and_replay(
    artifact_dir: str,
    execution: Execution,
    spec: RunSpec,
    durable: list[Event],
) -> None:
    """Make execution.json and replay.json durable before anything is judged."""
    execution_text = json.dumps(asdict(execution), sort_keys=True, indent=2) + "\n"
    _write_durable(
        os.path.join(artifact_dir, EXECUTION_NAME), execution_text.encode("utf-8")
    )
    replay = {
        "spec": json.loads(to_json(spec)),
        "history": [event_to_json(event) for event in durable],
    }
    replay_text = json.dumps(replay, sort_keys=True, allow_nan=False) + "\n"
    _write_durable(os.path.join(artifact_dir, REPLAY_NAME), replay_text.encode("utf-8"))


def _write_manifests(
    artifact_dir: str,
    execution: Execution,
    spec: RunSpec,
    durable: list[Event],
    target: Target,
) -> Verdict:
    """Write execution.json and replay.json durably, then judge and write verdict.json.

    A judge failure propagates and leaves the execution and replay manifests
    in place.
    """
    _write_execution_and_replay(artifact_dir, execution, spec, durable)
    verdict = judge(target, spec, durable, execution)
    _write_durable(os.path.join(artifact_dir, VERDICT_NAME), encode_verdict(verdict))
    return verdict


def _manifest_only(
    artifact_dir: str,
    spec: RunSpec,
    execution: Execution,
    target: Target,
) -> ExplorationResult:
    """Write manifests for an empty history (pre-launch failure path)."""
    verdict = _write_manifests(artifact_dir, execution, spec, [], target)
    return ExplorationResult(execution, verdict, artifact_dir)


def _record_close(
    execution: Execution, stream: _Stream, closed: RunDirClose, history_path: str
) -> Execution:
    """Record the run-dir close in the execution and the durable history.

    `run-closed` is appended only to an intact history once no process of
    the run is left; a completed run whose close leaked fails as
    `CLEANUP_FAILED`.
    """
    durable = stream.durable
    if stream.intact and closed.killed:
        durable.append(_append_run_closed(history_path))
    status, reason = execution.status, execution.reason
    if closed.leaked and status == COMPLETED:
        status, reason = HARNESS_FAILURE, CLEANUP_FAILED + closed.text
    return replace(
        execution,
        status=status,
        reason=reason,
        events=len(durable),
        cleanup=closed.text,
    )


def run_exploration(
    spec: RunSpec,
    artifact_dir: str,
    *,
    target: Target,
    red_control: str | None = None,
    setup: Callable[[], None] | None = None,
    runner_argv: list[str] | None = None,
) -> ExplorationResult:
    """Run one spec under containment; always leave full manifests behind.

    target supplies the environment, backend, and ordered properties;
    setup is an optional pre-launch hook whose failure still manifests;
    runner_argv overrides the same-par ``--runner --spec`` vector in tests.
    A target whose verdict could not be bound raises before anything runs.
    Before the spawn, `sweep_orphaned_run_dirs` closes every dead run's dir;
    its texts are recorded under ``setup["swept"]``.
    """
    preflight(target)
    os.makedirs(artifact_dir, exist_ok=True)
    spec_path = os.path.join(artifact_dir, RUN_SPEC_NAME)
    history_path = os.path.join(artifact_dir, HISTORY_NAME)
    with open(spec_path, "w") as handle:
        handle.write(to_json(spec))
    open(history_path, "w").close()
    if setup is not None:
        try:
            setup()
        except EnvironmentDegraded as error:
            with open(os.path.join(artifact_dir, STDERR_NAME), "w") as stderr_file:
                stderr_file.write(error.reason + "\n")
            execution = Execution(
                ENVIRONMENT_DEGRADED, error.reason, 0, None, setup=dict(error.facts)
            )
            return _manifest_only(artifact_dir, spec, execution, target)
        except Exception as error:  # noqa: BLE001 - recorded in execution.json
            execution = Execution(
                HARNESS_FAILURE,
                f"setup-failed: {type(error).__name__}: {error}",
                0,
                None,
            )
            return _manifest_only(artifact_dir, spec, execution, target)
    argv = (
        list(runner_argv)
        if runner_argv is not None
        else runner_argv_for(spec_path, target.name, red_control)
    )
    opened: list[TextIO] = []
    try:
        opened.append(open(os.path.join(artifact_dir, STDERR_NAME), "w"))
        opened.append(
            open(history_path, "w", encoding="utf-8", errors="surrogateescape")
        )
        swept = sweep_orphaned_run_dirs()
        run_dir = tempfile.mkdtemp(prefix=RUN_DIR_PREFIX)
        lock_fd = hold_run_lock(run_dir)
    except OSError as error:
        for handle in opened:
            handle.close()
        return _manifest_only(
            artifact_dir,
            spec,
            Execution(HARNESS_FAILURE, f"setup-failed: {error}", 0, None),
            target,
        )
    stderr_file, history_file = opened
    runner_deadline = time.monotonic() + spec.budgets.runner_deadline_seconds
    try:
        proc = subprocess.Popen(
            argv,
            stdout=subprocess.PIPE,
            stderr=stderr_file,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
            preexec_fn=_die_with_spawning_thread(),
            env={
                **_runner_env(),
                RUN_DIR_ENV: run_dir,
                RUNNER_DEADLINE_ENV: str(runner_deadline),
            },
        )
    except (OSError, ValueError) as error:
        stderr_file.close()
        history_file.close()
        closed = close_run_dir(run_dir)
        os.close(lock_fd)
        return _manifest_only(
            artifact_dir,
            spec,
            Execution(
                HARNESS_FAILURE, f"spawn-failed: {error}", 0, None, cleanup=closed.text
            ),
            target,
        )
    try:
        stream = _supervise(spec, proc, history_file, runner_deadline)
    finally:
        stderr_file.close()
        # The runner carries RUN_DIR_ENV, so the close's kill scan kills it
        # and waits until it exits, as for every other process of the run;
        # its group is killed after for EDEN_TEST_CLEANUP=no.
        closed = close_run_dir(run_dir)
        kill_group(proc.pid)
        os.close(lock_fd)
        returncode = _reap(proc)
    execution = _execution(stream, returncode, os.path.join(artifact_dir, STDERR_NAME))
    execution = _record_close(execution, stream, closed, history_path)
    facts = {**(execution.setup or {}), "run_dir": run_dir}
    if swept:
        facts["swept"] = swept
    execution = replace(execution, setup=facts)
    verdict = _write_manifests(artifact_dir, execution, spec, stream.durable, target)
    return ExplorationResult(execution, verdict, artifact_dir)
