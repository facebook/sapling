# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Spec builders, fake environments, and live-run helpers shared by the backend tests."""

import os
import stat
import subprocess
import tempfile
import threading
import unittest
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import replace
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.core.checker import (
    check as check_target,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    ExplorationResult,
    HARNESS_FAILURE,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    DEADLINE_RUN,
    Environment as SpecEnvironment,
    FINISHED,
    RunSpec,
    STARTED,
)
from eden.integration.correctness.exploratory.core.target import ProcessRegistry
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET
from eden.test_support.temporary_directory import TempFileManager


_SEED = 11
REGISTERS = ("tree/registers/r0.reg", "tree/registers/r1.reg")


def check(spec: RunSpec, events: list[Any]) -> Verdict:
    return check_target(RECORDING_TARGET, spec, events)


def facts_of(
    spec: RunSpec, events: list[Event], execution_status: str | None = None
) -> HistoryFacts:
    """Return the recording target's typed facts of one captured history."""
    return HistoryFacts.build(
        spec, events, RECORDING_TARGET, execution_status=execution_status
    )


def decode_problems(facts: HistoryFacts) -> dict[str, tuple[str, ...]]:
    """Return every action side whose recorded result did not decode, by side."""
    problems: dict[str, tuple[str, ...]] = {}
    for action in facts.actions.values():
        if action.problems:
            problems[action.id] = action.problems
        if action.reference is not None and action.reference.problems:
            problems[f"{action.id}/reference"] = action.reference.problems
    return problems


def _spec_env() -> SpecEnvironment:
    """Return the standard live-test environment section."""
    return SpecEnvironment(
        target={
            "repository": {
                "file_count": 32,
                "commit_count": 4,
                "registers": list(REGISTERS),
            },
            "channel": "fuse",
            "case_sensitive": True,
            "fixture_source": "backing",
            "setup_profile": {"materialize": False, "scatter_dirty": 0},
        }
    )


def _spec(
    register: str, actions: list[Action], actors: list[tuple[str, list[str]]]
) -> RunSpec:
    """Return a single-register spec owning every action exactly once."""
    return RunSpec(
        seed=_SEED,
        target=RECORDING_TARGET.name,
        environment=SpecEnvironment(
            target={
                **_spec_env().target,
                "repository": {
                    "file_count": 32,
                    "commit_count": 4,
                    "registers": [register],
                },
            }
        ),
        # Hang guards: these tests check outcomes, not how long a loaded host
        # takes to start a daemon and reach them.
        budgets=Budgets(120.0, 60.0, 540.0),
        actors=tuple(Actor(id=name, actions=tuple(ids)) for name, ids in actors),
        actions=tuple(actions),
    )


def _expected(writer: str, value_id: str) -> str:
    return encode_value(_SEED, writer, value_id).hex()


def _green_spec(register: str) -> RunSpec:
    """Two actors: one writes then reads, one thrift-reads (0 and 60) and stats."""
    w1 = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": register, "value_id": "green-w1"},
    )
    r1 = Action(
        id="r1",
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=((w1.id, FINISHED),),
        payload={"path": register, "channel": "fuse"},
    )
    t0 = Action(
        id="t0",
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=((w1.id, FINISHED),),
        payload={
            "path": register,
            "channel": "thrift",
            "sync_timeout_seconds": 0,
        },
    )
    t60 = Action(
        id="t60",
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=((w1.id, FINISHED),),
        payload={
            "path": register,
            "channel": "thrift",
            "sync_timeout_seconds": 60,
        },
    )
    g1 = Action(
        id="g1",
        kind="getattr",
        deadline=DEADLINE_OPERATION,
        payload={"path": register},
    )
    return _spec(
        register,
        [w1, r1, t0, t60, g1],
        [("a", ["w1", "r1"]), ("b", ["t0", "t60", "g1"])],
    )


_FUSE_DEVICE = "/dev/fuse"


def _session_bus_path() -> str:
    """The user DBus session bus socket that `systemd-run --user` connects to."""
    address = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
    if address.startswith("unix:path="):
        return address.removeprefix("unix:path=").split(",")[0]
    runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
    return os.path.join(runtime, "bus")


def live_environment_problem() -> str | None:
    """Return why this host cannot start a live daemon, or None when it can.

    This is the only check that may skip a live test, and it runs before
    any setup: FUSE, then the user DBus session bus.
    """
    if not os.access(_FUSE_DEVICE, os.R_OK | os.W_OK):
        return f"no FUSE: {_FUSE_DEVICE} is not readable and writable"
    bus = _session_bus_path()
    try:
        if stat.S_ISSOCK(os.stat(bus).st_mode):
            return None
    except OSError:
        pass
    return f"no user DBus session bus at {bus}"


def _require_live(test: unittest.TestCase) -> None:
    """Skip a live test before any setup when the host cannot run a daemon."""
    problem = live_environment_problem()
    if problem is not None:
        test.skipTest(f"live EdenFS unavailable: {problem}")


def _stderr_log(artifact_dir: str) -> str:
    """Return the live runner's stderr log, or empty when it was never written."""
    try:
        with open(os.path.join(artifact_dir, "stderr.log")) as handle:
            return handle.read()
    except OSError:
        return ""


def _fail_on_setup_failure(result: ExplorationResult, artifact_dir: str) -> None:
    """Fail when the live runner's setup failed after the environment check passed."""
    if result.execution.status != HARNESS_FAILURE:
        return
    if result.execution.reason.startswith("setup-failed") or (
        "setup-failed" in _stderr_log(artifact_dir)
    ):
        raise AssertionError(f"live setup failed: {result.execution.reason}")


@contextmanager
def _artifact_directory(prefix: str = "process-kill.") -> Iterator[str]:
    """Create an artifact directory retained by TPX when requested."""
    result_root = os.environ.get("TEST_RESULT_ARTIFACTS_DIR")
    if result_root:
        os.makedirs(result_root, exist_ok=True)
        yield tempfile.mkdtemp(prefix=prefix, dir=result_root)
        return
    with TempFileManager() as manager:
        yield str(manager.make_temp_dir(prefix=prefix))


def _status_spec(register: str) -> RunSpec:
    """One register write followed by an sl status probe of the mount."""
    w1 = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": register, "value_id": "status-w1"},
    )
    s1 = Action(
        id="s1",
        kind="status",
        deadline=DEADLINE_OPERATION,
        requires=((w1.id, FINISHED),),
        payload={"channel": "sl"},
    )
    return _spec(register, [w1, s1], [("a", ["w1", "s1"])])


def _status_via_thrift_spec(register: str) -> RunSpec:
    """Write one register, then query status at an explicit and current parent."""
    actions = [
        Action(
            id="w1",
            kind="write",
            deadline=DEADLINE_OPERATION,
            payload={"path": register, "value_id": "status-w1"},
        ),
        Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_OPERATION,
            payload={"channel": "thrift", "revision": 3},
        ),
        Action(
            id="s2",
            kind="status",
            deadline=DEADLINE_OPERATION,
            payload={"channel": "thrift"},
        ),
    ]
    return _spec(register, actions, [("a", [action.id for action in actions])])


def _manifest_presence_spec(register: str) -> RunSpec:
    """Checkout through Thrift, observe changed paths, then read a register."""
    checkout = Action(
        id="c1",
        kind="checkout",
        deadline=DEADLINE_OPERATION,
        payload={"revision": 0, "via": "thrift", "mode": "normal"},
    )
    manifest = Action(
        id="m1",
        kind="manifest_presence",
        deadline=DEADLINE_OPERATION,
        requires=((checkout.id, FINISHED),),
        payload={"from_revision": 3, "to_revision": 0},
    )
    read = Action(
        id="r1",
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=((manifest.id, FINISHED),),
        payload={"path": register, "channel": "fuse"},
    )
    return _spec(register, [checkout, manifest, read], [("a", ["c1", "m1", "r1"])])


def _forced_checkout_spec(register: str) -> RunSpec:
    """Dirty one register, force a Thrift checkout over it, then read it back."""
    write = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": register, "value_id": "force-w1"},
    )
    checkout = Action(
        id="c0",
        kind="checkout",
        deadline=DEADLINE_OPERATION,
        payload={"revision": 0, "via": "thrift", "mode": "force"},
    )
    read = Action(
        id="r1",
        kind="read",
        deadline=DEADLINE_OPERATION,
        payload={"path": register, "channel": "fuse"},
    )
    return _spec(register, [write, checkout, read], [("a", ["w1", "c0", "r1"])])


def _fd_spec(register: str) -> RunSpec:
    """Open a register, let another actor write it, then read through the held fd."""
    o0 = Action(
        id="o0", kind="open", deadline=DEADLINE_OPERATION, payload={"path": register}
    )
    w1 = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        requires=((o0.id, FINISHED),),
        payload={"path": register, "value_id": "fd-w1"},
    )
    f1 = Action(
        id="f1",
        kind="read_fd",
        deadline=DEADLINE_OPERATION,
        requires=((w1.id, FINISHED),),
        payload={"fd": o0.id},
    )
    x1 = Action(
        id="x1", kind="close", deadline=DEADLINE_OPERATION, payload={"fd": o0.id}
    )
    return _spec(register, [o0, w1, f1, x1], [("a", ["o0", "f1", "x1"]), ("b", ["w1"])])


def _journal_window_spec(register: str) -> RunSpec:
    """Write one register before observing its raw-journal window."""
    write = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": register, "value_id": "journal-w1"},
    )
    journal = Action(
        id="j1",
        kind="journal_window",
        deadline=DEADLINE_OPERATION,
        requires=((write.id, FINISHED),),
        payload={"since_sequence": 0},
    )
    return _spec(register, [write, journal], [("a", ["w1", "j1"])])


def _kill_status_spec(register: str) -> RunSpec:
    """Block one status until its owned process group is killed."""
    install = Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        payload={
            "fault_id": "F",
            "site": "status-cache-fill",
            "target_action": "s1",
            "behavior": "block",
        },
    )
    status = Action(
        id="s1",
        kind="status",
        deadline=DEADLINE_RUN,
        requires=((install.id, FINISHED),),
        payload={"channel": "sl"},
    )
    wait_action = Action(
        id="h1",
        kind="fault_await",
        deadline=DEADLINE_OPERATION,
        requires=((status.id, STARTED),),
        payload={"fault_id": "F"},
    )
    kill = Action(
        id="k1",
        kind="kill_process",
        deadline=DEADLINE_OPERATION,
        requires=((status.id, STARTED), (wait_action.id, FINISHED)),
        payload={"target_action": status.id, "after_ms": 500},
    )
    remove = Action(
        id="x1",
        kind="fault_remove",
        deadline=DEADLINE_OPERATION,
        requires=((wait_action.id, FINISHED), (kill.id, FINISHED)),
        payload={"fault_id": "F"},
    )
    return _spec(
        register,
        [install, status, wait_action, kill, remove],
        [
            ("installer", [install.id]),
            ("worker", [status.id]),
            ("nemesis", [wait_action.id, kill.id, remove.id]),
        ],
    )


def _fetch_tree_status_spec(
    register: str, *, warm: bool, revision: int | None
) -> RunSpec:
    """Park a current-parent Thrift status and compare its result after drain."""
    write = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": register, "value_id": "fetch-tree-status"},
    )
    warmup = Action(
        id="p0",
        kind="status",
        deadline=DEADLINE_OPERATION,
        requires=((write.id, FINISHED),),
        payload={
            "channel": "thrift",
            **({} if revision is None else {"revision": revision}),
        },
    )
    install = Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        requires=(((warmup if warm else write).id, FINISHED),),
        payload={
            "fault_id": "F",
            "site": "fetch-tree",
            "target_action": "s1",
            "behavior": "block",
        },
    )
    status = Action(
        id="s1",
        kind="status",
        deadline=DEADLINE_RUN,
        requires=((install.id, FINISHED),),
        payload={
            "channel": "thrift",
            **({} if revision is None else {"revision": revision}),
        },
    )
    wait_action = Action(
        id="h1",
        kind="fault_await",
        deadline=DEADLINE_OPERATION,
        requires=((status.id, STARTED),),
        payload={"fault_id": "F"},
    )
    remove = Action(
        id="x1",
        kind="fault_remove",
        deadline=DEADLINE_OPERATION,
        requires=((wait_action.id, FINISHED),),
        payload={"fault_id": "F"},
    )
    control = Action(
        id="s2",
        kind="status",
        deadline=DEADLINE_OPERATION,
        requires=((status.id, FINISHED), (remove.id, FINISHED)),
        payload={
            "channel": "thrift",
            **({} if revision is None else {"revision": revision}),
        },
    )
    actions = [write]
    if warm:
        actions.append(warmup)
    actions.extend([install, status, wait_action, remove, control])
    return _spec(
        register,
        actions,
        [
            ("setup", [write.id] + ([warmup.id] if warm else [])),
            ("installer", [install.id]),
            ("worker", [status.id, control.id]),
            ("nemesis", [wait_action.id, remove.id]),
        ],
    )


def _fetch_tree_cancel_spec(register: str) -> RunSpec:
    """Park native status, kill its group, drain, then probe current status."""
    write = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": register, "value_id": "fetch-tree-cancel"},
    )
    install = Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        requires=((write.id, FINISHED),),
        payload={
            "fault_id": "F",
            "site": "fetch-tree",
            "target_action": "s1",
            "behavior": "block",
        },
    )
    status = Action(
        id="s1",
        kind="status",
        deadline=DEADLINE_RUN,
        requires=((install.id, FINISHED),),
        payload={"channel": "sl", "revision": 2},
    )
    wait_action = Action(
        id="h1",
        kind="fault_await",
        deadline=DEADLINE_OPERATION,
        requires=((status.id, STARTED),),
        payload={"fault_id": "F"},
    )
    kill = Action(
        id="k1",
        kind="kill_process",
        deadline=DEADLINE_OPERATION,
        requires=((status.id, STARTED), (wait_action.id, FINISHED)),
        payload={"target_action": status.id, "after_ms": 500},
    )
    remove = Action(
        id="x1",
        kind="fault_remove",
        deadline=DEADLINE_OPERATION,
        requires=((wait_action.id, FINISHED), (kill.id, FINISHED)),
        payload={"fault_id": "F"},
    )
    probe = Action(
        id="s2",
        kind="status",
        deadline=DEADLINE_OPERATION,
        requires=((status.id, FINISHED), (remove.id, FINISHED)),
        payload={"channel": "sl"},
    )
    return _spec(
        register,
        [write, install, status, wait_action, kill, remove, probe],
        [
            ("setup", [write.id]),
            ("installer", [install.id]),
            ("worker", [status.id, probe.id]),
            ("nemesis", [wait_action.id, kill.id, remove.id]),
        ],
    )


class _FakeMountRepo:
    """Stub mount_repo serving canned sl status and diff --stat output."""

    def __init__(
        self,
        status: dict[str, str],
        parent: str | None = "abc123",
        raise_status: bool = False,
        diff: str = "",
        raise_diff: bool = False,
    ) -> None:
        self.hg_bin = "/bin/sl"
        self.hg_environment: dict[str, str] = {}
        self.path = "/mount"
        self._status = status
        self._parent = parent
        self._raise_status = raise_status
        self._diff = diff
        self._raise_diff = raise_diff

    def status(self) -> dict[str, str]:
        if self._raise_status:
            raise RuntimeError("sl exploded")
        return dict(self._status)

    def hg(self, *args: str) -> str:
        if args and args[0] == "diff":
            if self._raise_diff:
                raise RuntimeError("diff exploded")
            return self._diff
        if self._parent is None:
            raise RuntimeError("no parent")
        return self._parent


class _FakeEnv:
    """Stub Environment exposing only what the sl status backend touches."""

    def __init__(
        self,
        status: dict[str, str],
        parent: str | None = "abc123",
        raise_status: bool = False,
        diff: str = "",
        raise_diff: bool = False,
    ) -> None:
        self.mount_repo = _FakeMountRepo(status, parent, raise_status, diff, raise_diff)
        self.processes = ProcessRegistry()
        self.commits = (mock.Mock(node="node0"), mock.Mock(node="node1"))


class _BlockingProcess:
    """Popen stub whose communicate call exposes a controllable lifetime."""

    def __init__(self, output: str, returncode: int = 0, pid: int = 4101) -> None:
        self.pid = pid
        self.returncode = returncode
        self.output = output
        self.entered = threading.Event()
        self.release = threading.Event()
        self.waited = False

    def communicate(self) -> tuple[str, str]:
        self.entered.set()
        self.release.wait(timeout=60)
        return self.output, ""

    def wait(self, timeout: float | None = None) -> int:
        self.waited = True
        return self.returncode

    def poll(self) -> int | None:
        return None


_REAL_POPEN = subprocess.Popen


def _session_leader(test: unittest.TestCase) -> int:
    """Start a real session leader that ``test`` kills and reaps at cleanup."""
    leader = _REAL_POPEN(["sleep", "60"], start_new_session=True)
    test.addCleanup(leader.wait)
    test.addCleanup(leader.kill)
    return leader.pid


# Wait while the gate file exists, then exit with $2 (a negative $2 kills itself).
_CANNED_LEADER = (
    'while [ -e "$1" ]; do sleep 0.01; done; '
    '[ "$2" -lt 0 ] && kill -s KILL $$; exit "$2"'
)


class _CannedSl:
    """``Popen`` stand-in for live sl runs: canned output, a real leader process.

    Each call records its argv and keyword arguments, writes the next canned
    stdout (default empty) into the output file the runner passed, and starts
    a real ``sh`` leader with the runner's own arguments (except ``cwd``), so
    the shared Sapling process rule pins and reaps a real group.
    The leader exits with the next of ``returncodes``, else ``returncode``;
    with ``blocked`` it waits until ``release()``.
    """

    def __init__(
        self,
        *stdouts: str,
        returncode: int = 0,
        returncodes: tuple[int, ...] = (),
        blocked: bool = False,
    ) -> None:
        self._stdouts = iter(stdouts)
        self._returncodes = iter(returncodes)
        self.returncode = returncode
        self.calls: list[tuple[list[str], dict[str, Any]]] = []
        self.processes: list[subprocess.Popen[Any]] = []
        self.entered = threading.Event()
        gate, self._gate = tempfile.mkstemp(prefix="canned-sl.")
        os.close(gate)
        if not blocked:
            self.release()

    def release(self) -> None:
        if os.path.exists(self._gate):
            os.unlink(self._gate)

    def __call__(self, argv: list[str], **kwargs: Any) -> subprocess.Popen[Any]:
        self.calls.append((list(argv), kwargs))
        kwargs["stdout"].write(next(self._stdouts, "").encode())
        kwargs["stdout"].flush()
        popen_kwargs: dict[str, Any] = {**kwargs, "cwd": "/"}
        process = _REAL_POPEN(
            [
                "sh",
                "-c",
                _CANNED_LEADER,
                "sh",
                self._gate,
                str(next(self._returncodes, self.returncode)),
            ],
            **popen_kwargs,
        )
        self.processes.append(process)
        self.entered.set()
        return process


def _native_spec(action: Action, planned_kill: bool = False) -> RunSpec:
    """Return a spec containing one native target and optional process kill."""
    actions = [action]
    actors = [("a", [action.id])]
    if planned_kill:
        kill = Action(
            id="k1",
            kind="kill_process",
            deadline=DEADLINE_OPERATION,
            requires=((action.id, STARTED),),
            payload={"target_action": action.id, "after_ms": 0},
        )
        actions.append(kill)
        actors.append(("nemesis", [kill.id]))
    return _spec(REGISTERS[0], actions, actors)


def _red_spec(register: str) -> RunSpec:
    """One torn write followed by a read that must observe the torn value."""
    w1 = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": register, "value_id": "red-write-value"},
    )
    r1 = Action(
        id="r1",
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=((w1.id, FINISHED),),
        payload={"path": register, "channel": "fuse"},
    )
    return _spec(register, [w1, r1], [("a", ["w1", "r1"])])


_WIDE_ENTRIES = 2000


def _readdir_stream_spec() -> RunSpec:
    """Stream the wide directory, pause halfway, then resume with no checkout."""
    spec = _spec(
        REGISTERS[0],
        [
            Action(
                id="d0",
                kind="readdir_stream",
                deadline=DEADLINE_OPERATION,
                payload={"path": "tree/wide", "pause_after": _WIDE_ENTRIES // 2},
            ),
            Action(
                id="d1",
                kind="readdir_resume",
                deadline=DEADLINE_OPERATION,
                payload={"stream": "d0"},
            ),
        ],
        [("a", ["d0", "d1"])],
    )
    target = dict(spec.environment.target)
    target["setup_profile"] = {
        **target["setup_profile"],
        "wide_dir": {"name": "tree/wide", "entries": _WIDE_ENTRIES, "churn_all": True},
    }
    return replace(spec, environment=SpecEnvironment(target=target))


def _rename_spec(register: str) -> RunSpec:
    """Write a register, rename it aside, then read both paths."""
    destination = "tree/registers/moved.reg"
    actions = [
        Action(
            id="w1",
            kind="write",
            deadline=DEADLINE_OPERATION,
            payload={"path": register, "value_id": "rename-w1"},
        ),
        Action(
            id="n0",
            kind="rename",
            deadline=DEADLINE_OPERATION,
            payload={"source": register, "destination": destination},
        ),
        Action(
            id="ra",
            kind="read",
            deadline=DEADLINE_OPERATION,
            payload={"path": register, "channel": "fuse"},
        ),
        Action(
            id="rb",
            kind="read",
            deadline=DEADLINE_OPERATION,
            payload={"path": destination, "channel": "fuse"},
        ),
    ]
    return _spec(register, actions, [("a", ["w1", "n0", "ra", "rb"])])


def _mkdir_spec(register: str) -> RunSpec:
    """Make a directory beside a register, then stat it over both channels."""
    made = f"{register.rpartition('/')[0]}/made"
    actions = [
        Action(
            id="m0", kind="mkdir", deadline=DEADLINE_OPERATION, payload={"path": made}
        ),
        Action(
            id="g0", kind="getattr", deadline=DEADLINE_OPERATION, payload={"path": made}
        ),
    ]
    return _spec(register, actions, [("a", ["m0", "g0"])])


def _unlink_spec(register: str) -> RunSpec:
    """Unlink a committed register, then probe sl status."""
    actions = [
        Action(
            id="u0",
            kind="unlink",
            deadline=DEADLINE_OPERATION,
            payload={"path": register},
        ),
        Action(
            id="s1",
            kind="status",
            deadline=DEADLINE_OPERATION,
            payload={"channel": "sl"},
        ),
    ]
    return _spec(register, actions, [("a", ["u0", "s1"])])


def _dirstate_spec(register: str, kind: str, payload: dict[str, str]) -> RunSpec:
    """Change a register's dirstate through sl, then probe sl status."""
    change = Action("h0", kind, DEADLINE_OPERATION, payload=payload)
    probe = Action("s1", "status", DEADLINE_OPERATION, payload={"channel": "sl"})
    return _spec(register, [change, probe], [("a", ["h0", "s1"])])


def _mkdir_listing_spec() -> RunSpec:
    """Make a directory under the wide dir, stat it, then list the wide dir."""
    made = "tree/wide/made"
    spec = _readdir_stream_spec()
    actions = [
        Action(
            id="m0", kind="mkdir", deadline=DEADLINE_OPERATION, payload={"path": made}
        ),
        Action(
            id="g0", kind="getattr", deadline=DEADLINE_OPERATION, payload={"path": made}
        ),
        *spec.actions,
    ]
    return replace(
        spec, actions=tuple(actions), actors=(Actor("a", ("m0", "g0", "d0", "d1")),)
    )
