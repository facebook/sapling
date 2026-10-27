# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Real-filesystem histories through `EDENFS_TARGET`: the kernel as the POSIX reference.

Each seed composes a concurrent RunSpec of POSIX actions (write, read,
getattr, chmod, rename, unlink, mkdir, open/read_fd/close, one-shot
readdir, a paused readdir stream) over 2-4 actors, runs it with the same
`SharedExecutor` both differential sides use on a plain temporary
directory, records it through the real scheduler, and judges it. A kernel
filesystem produces only legal histories, so every reject is a contract
that disagrees with POSIX or with the filesystem: fix the contract and add
the recorded history as a legal corpus row (the failure prints both).

The same histories, with one certainly-illegal change planted into a
recorded result, must be rejected: that is the detection floor of the
POSIX layer on real interleavings.

Recorded orders differ between runs (real threads); the specs do not.
"""

from __future__ import annotations

import hashlib
import json
import os
import random
import shutil
import subprocess
import tempfile
import threading
import time
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core.checker import check, REJECT
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    Event,
    event_to_json,
    InMemorySink,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    from_json,
    RunSpec,
    SpecError,
    to_json,
)
from eden.integration.correctness.exploratory.core.scheduler import (
    BackendResult,
    run_actions,
)
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.executor import SharedExecutor
from eden.integration.correctness.exploratory.edenfs.fixture import (
    CHURN_FILES,
    DIRECTORY,
    GeneratedFixture,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET

SEEDS = range(300)
PLANT_SEEDS = range(0, 300, 3)
JITTER_SECONDS = 0.002
# The lane runs on tmpfs. POSIX leaves unspecified whether readdir returns an
# entry added or removed after opendir, and a rename onto a listed name is a
# remove plus an add, so btrfs may omit that name (seed 217). The lane's
# one-shot listing contract is stronger than POSIX.
LANE_PARENT = "/dev/shm" if os.path.isdir("/dev/shm") else None
REGISTERS = ["tree/registers/r0.reg", "tree/registers/r1.reg", "tree/registers/r2.reg"]
ENVIRONMENT = {
    "target": {
        "case_sensitive": True,
        "channel": "fuse",
        "fixture_source": "backing",
        "repository": {"commit_count": 4, "file_count": 32, "registers": REGISTERS},
        "setup_profile": {
            "materialize": False,
            "scatter_dirty": 0,
            "wide_dir": {"churn_all": False, "entries": 3, "name": "tree/wide"},
        },
    }
}


def _draft(seed: int) -> str:  # noqa: C901 - one generated spec, step by step
    """Return one candidate RunSpec text; the validator may refuse it."""
    rng = random.Random(seed)
    actions: dict[str, dict[str, Any]] = {}
    actors: list[dict[str, Any]] = []
    made: list[str] = []
    stream: tuple[int, str] | None = None
    n = 0
    for index in range(rng.randrange(2, 5)):
        ids, opens = [], []
        for _ in range(rng.randrange(3, 9)):
            n += 1
            aid = f"x{n}"
            names = REGISTERS + made
            parent = rng.choice(["tree/registers", "tree/registers", "tree/wide"])
            roll = rng.random()
            if roll < 0.18:
                act = {"kind": "write", "path": rng.choice(names), "value_id": aid}
            elif roll < 0.32:
                act = {"kind": "read", "path": rng.choice(names), "channel": "fuse"}
            elif roll < 0.42:
                act = {"kind": "getattr", "path": rng.choice(names)}
            elif roll < 0.52:
                source = rng.choice(names)
                others = [p for p in names if p != source]
                destination = (
                    rng.choice(others)
                    if others and rng.random() < 0.4
                    else f"{parent}/moved-{aid}"
                )
                if destination not in names:
                    made.append(destination)
                act = {"kind": "rename", "source": source, "destination": destination}
            elif roll < 0.58:
                act = {"kind": "unlink", "path": rng.choice(names)}
            elif roll < 0.65:
                path = f"{parent}/dir-{aid}"
                made.append(path)
                act = {"kind": "mkdir", "path": path}
            elif roll < 0.70:
                act = {
                    "kind": "chmod",
                    "path": rng.choice(names),
                    "mode": rng.choice([0o755, 0o644]),
                }
            elif roll < 0.78:
                act = {
                    "kind": "readdir",
                    "path": rng.choice(["tree/registers", "tree/wide"]),
                    "channel": "fuse",
                }
            elif roll < 0.84 and stream is None:
                act = {"kind": "readdir_stream", "path": "tree/wide", "pause_after": 1}
                stream = (index, aid)
            else:
                act = {"kind": "open", "path": rng.choice(names)}
                opens.append(aid)
            act["deadline"] = "operation"
            actions[aid] = act
            ids.append(aid)
        for fd in opens:
            for kind in ("read_fd", "close"):
                n += 1
                actions[f"x{n}"] = {"kind": kind, "fd": fd, "deadline": "operation"}
                ids.append(f"x{n}")
        actors.append({"id": f"a{index}", "actions": ids})
    if stream is not None:
        n += 1
        actions[f"x{n}"] = {
            "kind": "readdir_resume",
            "stream": stream[1],
            "deadline": "operation",
        }
        actors[stream[0]]["actions"].append(f"x{n}")
    for actor in actors[1:]:
        if rng.random() < 0.4:
            gate = rng.choice(actors[0]["actions"])
            actions[actor["actions"][0]]["requires"] = [
                [gate, rng.choice(["started", "finished"])]
            ]
    return json.dumps(
        {
            "schema": "edenfs-exploratory-run",
            "schema_version": 1,
            "seed": seed,
            "target": "edenfs",
            "environment": ENVIRONMENT,
            "budgets": {
                "cleanup_deadline_seconds": 30.0,
                "operation_deadline_seconds": 120.0,
                "runner_deadline_seconds": 600,
            },
            "metadata": {"producer_stratum": "posix-lane"},
            "actors": actors,
            "actions": actions,
        }
    )


def compose(seed: int) -> RunSpec:
    """Return the first admitted draft for `seed` (deterministic)."""
    for attempt in range(300):
        try:
            return from_json(_draft(seed * 1000 + attempt), EDENFS_TARGET)
        except SpecError:
            continue
    raise AssertionError(f"seed {seed}: no admitted draft")


def _materialize(root: str, spec: RunSpec) -> None:
    fixture = GeneratedFixture.for_spec(spec)
    revision = fixture.initial_revision
    layout = fixture.layout
    paths = [f.path for f in layout.files(revision)]
    paths += [*fixture.registers, *CHURN_FILES, *(w.path for w in layout.wide_files())]
    for path in paths:
        entry = fixture.committed(path, revision)
        if entry is None:
            continue
        full = os.path.join(root, path)
        if entry.kind == DIRECTORY:
            os.makedirs(full, exist_ok=True)
            continue
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "wb") as handle:
            handle.write(entry.data)
        os.chmod(full, 0o755 if entry.executable else 0o644)


class _Streams:
    """`readdir_stream`/`readdir_resume` in process, with the live backend's result fields."""

    def __init__(self, root: str) -> None:
        self._root = root
        self._live: dict[str, Any] = {}

    @staticmethod
    def _pos(fd: int) -> int:
        with open(f"/proc/self/fdinfo/{fd}") as info:
            return int(
                next(line for line in info if line.startswith("pos:")).split()[1]
            )

    def start(self, action: Any) -> BackendResult:
        fd = os.open(
            os.path.join(self._root, action.payload["path"]),
            os.O_RDONLY | os.O_DIRECTORY,
        )
        stream = os.scandir(fd)
        names = []
        for entry in stream:
            names.append(entry.name)
            if len(names) == action.payload["pause_after"]:
                break
        pos = self._pos(fd)
        self._live[action.id] = (fd, stream, names, pos)
        return BackendResult(
            "ok", {"pgid": os.getpid(), "paused_at": len(names), "pos_at_pause": pos}
        )

    def resume(self, action: Any) -> BackendResult:
        fd, stream, names, pos = self._live.pop(action.payload["stream"])
        end = pos
        for entry in stream:
            names.append(entry.name)
            end = self._pos(fd)
        stream.close()
        os.close(fd)
        return BackendResult(
            "ok",
            {
                "stream": action.payload["stream"],
                "entries": names,
                "pos_at_pause": pos,
                "pos_at_end": end,
            },
        )


def _no_sl(
    action: Action, args: list[str], *, primary: bool, started: float
) -> subprocess.CompletedProcess[str]:
    raise AssertionError(f"the POSIX lane runs no Sapling command ({action.id})")


def record(spec: RunSpec) -> list[Event]:
    """Run `spec` on a fresh temporary directory and return its history."""
    root = tempfile.mkdtemp(prefix="posix-lane-", dir=LANE_PARENT)
    try:
        _materialize(root, spec)
        owners = {a: actor.id for actor in spec.actors for a in actor.actions}
        executor = SharedExecutor(
            lambda path: os.path.join(root, path),
            lambda action: encode_value(
                spec.seed, owners[action.id], action.payload["value_id"]
            ),
            lambda _revision: "",
            _no_sl,
            lambda kind, detail: RuntimeError(f"{kind}: {detail}"),
        )
        streams = _Streams(root)
        lock = threading.Lock()
        rng = random.Random(spec.seed)

        def jitter() -> None:
            with lock:
                delay = rng.random() * JITTER_SECONDS
            time.sleep(delay)

        def backend(action: Any) -> BackendResult:
            jitter()
            if action.kind == "readdir_stream":
                result = streams.start(action)
            elif action.kind == "readdir_resume":
                result = streams.resume(action)
            else:
                result = executor(action)
            if action.kind == "getattr" and result.outcome == "ok":
                # No Thrift channel here: the source-control type is the FUSE type.
                result = BackendResult(
                    "ok", {**result.facts, "thrift_type": result.facts["fuse_type"]}
                )
            jitter()
            return result

        sink = InMemorySink()
        status = run_actions(spec, backend, sink, time.monotonic)
        executor.close()
        return [
            *sink.events,
            Event(
                CLEANUP,
                time.monotonic(),
                {"scheduler": {"status": status.status, "reason": status.reason}},
            ),
        ]
    finally:
        # A chmod 0644 on a created directory would make rmtree skip its children.
        for directory, _subdirs, _files in os.walk(root):
            os.chmod(directory, 0o755)
        shutil.rmtree(root)


def _violations(verdict: Any) -> list[str]:
    assert verdict.report is not None
    return sorted(
        f"{o.rule_id}[{o.scope}]"
        for r in verdict.report.results
        for o in r.obligations
        if str(o.status) == "violated"
    )


def _content(data: bytes) -> dict[str, Any]:
    return {
        "bytes_prefix": data[:128].hex(),
        "length": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def plants(spec: RunSpec, events: list[Event]) -> list[tuple[str, list[Event]]]:
    """Return copies of `events` with one change planted that no explanation allows."""
    completions = {
        e.data["action"]: i
        for i, e in enumerate(events)
        if e.type == "ok" and "action" in e.data
    }
    planted = []

    def changed(index: int, **fields: Any) -> list[Event]:
        copy = list(events)
        copy[index] = Event(
            events[index].type, events[index].time, {**events[index].data, **fields}
        )
        return copy

    for action in spec.actions:
        index = completions.get(action.id)
        if index is None:
            continue
        if action.kind in ("read", "read_fd"):
            planted.append(
                (
                    "fabricated-read",
                    changed(
                        index, **_content(encode_value(spec.seed, "nobody", "never"))
                    ),
                )
            )
            break
    for action in spec.actions:
        index = completions.get(action.id)
        if (
            action.kind == "getattr"
            and index is not None
            and events[index].data.get("size") == 64
        ):
            # Every file the lane can produce is 64 bytes.
            planted.append(("impossible-size", changed(index, size=65)))
            break
    return planted


class PosixLaneTest(unittest.TestCase):
    def test_real_filesystem_histories_are_never_rejected(self) -> None:
        for seed in SEEDS:
            spec = compose(seed)
            events = record(spec)
            verdict = check(
                EDENFS_TARGET, spec, events, execution={"status": "completed"}
            )
            with self.subTest(seed=seed):
                self.assertNotEqual(
                    REJECT,
                    verdict.kind,
                    f"seed {seed}: {_violations(verdict)}; add it as a legal corpus row:\n"
                    + json.dumps(
                        {
                            "spec": json.loads(to_json(spec)),
                            "history": [event_to_json(e) for e in events],
                        }
                    ),
                )

    def test_planted_results_are_rejected(self) -> None:
        for seed in PLANT_SEEDS:
            spec = compose(seed)
            events = record(spec)
            for name, planted in plants(spec, events):
                verdict = check(
                    EDENFS_TARGET, spec, planted, execution={"status": "completed"}
                )
                with self.subTest(seed=seed, plant=name):
                    self.assertEqual(
                        REJECT,
                        verdict.kind,
                        f"seed {seed}: planted {name} was {verdict.kind}",
                    )
