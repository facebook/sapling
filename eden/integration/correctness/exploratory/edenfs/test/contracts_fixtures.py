# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Builders shared by the contracts tests: run specs over the generated fixture,
recorded outcomes, derived sinks checked against each action's footprint, read and
getattr observations, and the brute-force explanation check.
"""

from __future__ import annotations

import hashlib
import json
from collections.abc import Sequence
from typing import Any

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    Cell,
    Constraint,
    EffectGroup,
    eq,
    Expect,
    Guard,
    ObligationRef,
    Observation,
    Requirement,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.runspec import from_json, RunSpec
from eden.integration.correctness.exploratory.core.semantics import Problem
from eden.integration.correctness.exploratory.core.solver import satisfiable
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    WALK_DEADLOCK_TASK,
)
from eden.integration.correctness.exploratory.edenfs.contracts import record_contracts
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    CONTRACT_RULES,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    CLEAN,
    DESCRIPTOR,
    DESCRIPTOR_BINDING,
    DESCRIPTOR_OBJECT,
    EdenPinned,
    file_value,
    mode_cell,
    mode_value,
    path_cell,
    track_cell,
    within_footprint,
    YES,
)
from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_committed,
    encode_value,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import (
    RecordingSink,
    VOCAB_TARGET,
)


R0 = "tree/registers/r0.reg"


R1 = "tree/registers/r1.reg"


SEED = 5


TIP = 3


R0_PART = "p" + hashlib.sha256(R0.encode()).hexdigest()[:16]


def run_spec(
    actions: dict[str, dict[str, Any]],
    actors: list[dict[str, Any]],
    *,
    file_count: int = 1000,
    scatter_dirty: int = 0,
    registers: tuple[str, ...] = (R0, R1),
    type_changes: tuple[tuple[str, int], ...] = (),
    wide_dir: dict[str, Any] | None = None,
    metadata: dict[str, Any] | None = None,
) -> RunSpec:
    setup_profile: dict[str, Any] = {
        "materialize": False,
        "scatter_dirty": scatter_dirty,
    }
    if wide_dir is not None:
        setup_profile["wide_dir"] = wide_dir
    repository: dict[str, Any] = {
        "file_count": file_count,
        "commit_count": TIP + 1,
        "registers": list(registers),
    }
    if type_changes:
        repository["type_changes"] = [
            {"path": path, "at_revision": revision} for path, revision in type_changes
        ]
    text = json.dumps(
        {
            "schema": "edenfs-exploratory-run",
            "schema_version": 1,
            "seed": SEED,
            "target": "edenfs",
            "environment": {
                "target": {
                    "repository": repository,
                    "channel": "fuse",
                    "case_sensitive": True,
                    "fixture_source": "backing",
                    "setup_profile": setup_profile,
                }
            },
            "budgets": {
                "operation_deadline_seconds": 120,
                "cleanup_deadline_seconds": 30,
                "runner_deadline_seconds": 600,
            },
            "actors": actors,
            "actions": actions,
            "metadata": metadata or {},
        }
    )
    return from_json(text, VOCAB_TARGET)


# The declaration a spec overlapping a checkout walk with a remove or
# transfer carries (`validate_forced_checkout_walks`).
WALK_OVERLAP = {"task": WALK_DEADLOCK_TASK}


def action_payload(kind: str, **payload: Any) -> dict[str, Any]:
    return {"kind": kind, "deadline": "operation", **payload}


def checkout_op(
    revision: int, via: str = "thrift", mode: str = "normal"
) -> dict[str, Any]:
    return action_payload("checkout", revision=revision, via=via, mode=mode)


# Cells no footprint names: checkout sources and a held descriptor's own state.
OUTSIDE_FOOTPRINTS = frozenset(
    {"source", DESCRIPTOR, DESCRIPTOR_BINDING, DESCRIPTOR_OBJECT}
)


def contract_sink(
    spec: RunSpec,
    events: list[Event],
    pinned: EdenPinned | None = None,
    rules: frozenset[str] = CONTRACT_RULES,
) -> RecordingSink:
    facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
    sink = RecordingSink()
    record_contracts(spec, facts, pinned, sink, rules)
    for group in sink.groups:
        action = spec.action(group.action)
        footprint = ACTION_DEFS[action.kind].footprint(spec, action)
        for effect in group.effects:
            for cell in effect.writes():
                if cell.space not in OUTSIDE_FOOTPRINTS:
                    assert within_footprint(cell, footprint), (group.id, cell)
    return sink


def contract_one(
    kind_payload: dict[str, Any], *events: Event, actor: str = "a"
) -> tuple[RunSpec, RecordingSink]:
    spec = run_spec({"x": kind_payload}, [{"id": actor, "actions": ["x"]}])
    return spec, contract_sink(spec, list(events))


def guard_holds(guard: Guard | Requirement, values: dict[Cell, Value]) -> bool:
    return guard.expect.holds(tuple(values.get(cell, ABSENT) for cell in guard.cells))


def committed_value(path: str, revision: int) -> Value:
    return file_value(encode_committed(SEED, path, revision))


def guard_applies(
    group: EffectGroup,
    local: Value,
    source: int = TIP,
    *,
    mode: Value | None = None,
    track: Value = CLEAN,
    visited: bool = True,
) -> bool:
    """Return whether a group's first guard holds for one local state of its path."""
    guard = group.effects[0].guard
    assert guard is not None
    values = {
        cell: YES if cell.space in ("visit", "dryrun") and visited else ABSENT
        for cell in guard.cells
    }
    values.update(local_state(guard.cells[0].key, local, mode, track))
    mark = source_mark(guard, source)
    if mark is not None:
        values[mark] = YES
    return guard_holds(guard, values)


def source_mark(condition: Guard | Requirement, revision: int) -> Cell | None:
    """Return the source mark of `revision` a checkout condition reads.

    None for a settled checkout, whose conditions read no mark: its one
    source is a constant.
    """
    marks = [cell for cell in condition.cells if cell.space == "source"]
    if not marks:
        return None
    (cell,) = [cell for cell in marks if cell.key.endswith(f"/r{revision}")]
    return cell


DIRTY = Value("file", ("dirty",))


PLAIN = mode_value(False)


def local_state(
    path: str, value: Value, mode: Value | None = None, track: Value = CLEAN
) -> dict[Cell, Value]:
    """Return one path's local cells: a file defaults to the committed (non-exec) mode."""
    if mode is None:
        mode = PLAIN if value.cls == "file" else ABSENT
    return {path_cell(path): value, mode_cell(path): mode, track_cell(path): track}


def checkout_spec(
    *checkouts: tuple[str, dict[str, Any]],
    extra: dict[str, dict[str, Any]] | None = None,
    actors: list[dict[str, Any]] | None = None,
    metadata: dict[str, Any] | None = None,
) -> RunSpec:
    actions = dict(checkouts)
    actions.update(extra or {})
    return run_spec(
        actions, actors or [{"id": "a", "actions": list(actions)}], metadata=metadata
    )


def read_obs(oid: str, path: str, value: Value, lo: float, hi: float) -> Observation:
    """A read of `path` returning `value`, as a bytes collector would record it."""
    return Observation(
        id=oid,
        action=oid,
        cells=(path_cell(path),),
        expect=Expect(((eq(value),),)),
        window=Window(lo, hi),
        channel="fuse",
        obligations=(ObligationRef("bytes.read", oid),),
    )


def stat_obs(
    oid: str, path: str, executable: bool, lo: float, hi: float
) -> Observation:
    """A stat of `path` seeing its executable bit."""
    return Observation(
        id=oid,
        action=oid,
        cells=(mode_cell(path),),
        expect=Expect(((eq(mode_value(executable)),),)),
        window=Window(lo, hi),
        channel="fuse",
        obligations=(ObligationRef("metadata.mode", oid),),
    )


def is_explained(
    spec: RunSpec,
    events: list[Event],
    observations: tuple[Observation, ...] = (),
    rules: frozenset[str] = CONTRACT_RULES,
) -> bool:
    """Return whether the contract inputs plus `observations` have an explanation."""
    sink = contract_sink(spec, events, rules=rules)
    return engine_explains(
        EdenPinned.for_spec(spec),
        sink.groups,
        [*sink.observations, *observations],
        sink.constraints,
    )


def engine_explains(
    pinned: EdenPinned,
    groups: Sequence[EffectGroup],
    observations: Sequence[Observation],
    constraints: Sequence[Constraint] = (),
) -> bool:
    """Return the engine's decision on whether the inputs have one explanation."""
    problem = Problem(
        tuple(groups), tuple(observations), tuple(constraints), pinned.initial
    )
    verdict, _work = satisfiable(problem, work=10**9)
    assert verdict != "unknown", "the engine did not decide a contracts test input"
    return verdict == "sat"


def sl_abort_facts(revision: int, *paths: str, more: int = 0) -> dict[str, Any]:
    listed = [*paths, *([f"...and {more} more"] if more else [])]
    stderr = (
        f"abort: {len(paths) + more} conflicting file changes:\n "
        + "\n ".join(listed)
        + "\n(commit, shelve, goto --clean to discard all your changes, "
        "or goto --merge to merge them)\n"
    )
    return {"reason": "goto", "returncode": 255, "stdout": "", "stderr": stderr}


THRIFT_OK = {"node": "n", "via": "thrift", "conflicts": 0}


SL_OK = {"node": "n", "via": "goto"}


WRITTEN = file_value(encode_value(SEED, "a", "w"))


def digest_facts(data: bytes) -> dict[str, Any]:
    return {
        "bytes_prefix": data[:32].hex(),
        "length": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }
