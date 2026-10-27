# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Static upper bounds on the terminal events of complete observations.

A complete observation (sl or Thrift status, a directory listing, a
manifest presence probe, a journal window) records every path it saw in
one terminal event, and an event over `MAX_EVENT_BYTES` makes the whole
history invalid. RunSpec validation therefore refuses a configuration
whose observation could outgrow that limit, from arithmetic over the
pinned fixture and the spec's actions alone: nothing enumerates the
generated layout. Each bound counts the paths an observation can name,
times the longest encoded entry, plus the event's fixed envelope; a
differential run carries the reference side's copy too, so its bound is
doubled, plus room for the longest reason. Bounds are conservative: a refused spec may still have fit.
"""

from __future__ import annotations

import json
import math
from collections.abc import Iterable, Iterator
from dataclasses import dataclass
from pathlib import PurePosixPath
from typing import Any

from eden.integration.correctness.exploratory.core.history import (
    Event,
    event_line,
    MAX_EVENT_BYTES,
    OK,
)
from eden.integration.correctness.exploratory.core.jsonio import MAX_ID_BYTES
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    MAX_ACTIONS,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.edenfs.fixture import (
    CHURN_FILES,
    FILES_PER_DIRECTORY,
    REPLACEMENT_STRIDE,
)
from eden.integration.correctness.repo import wide_entry_path

COMPLETE_OBSERVATIONS = frozenset(
    {"status", "readdir", "readdir_resume", "manifest_presence", "journal_window"}
)
# The longest status code either channel records: Thrift's ScmFileStatus names.
_LONGEST_CODE = "MODIFIED"
_LONGEST_REASON = 1024
_MOVES = frozenset({"rename", "sl_mv"})
_TIME = float(2**52) + 0.5


def _json_bytes(value: Any) -> int:
    return len(
        json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    )


@dataclass(frozen=True)
class _Fixture:
    """The fixture dimensions a bound reads, without the layout's enumeration."""

    file_count: int
    commit_count: int
    registers: tuple[str, ...]
    type_changes: tuple[str, ...]
    scatter_dirty: int
    wide_name: str | None
    wide_entries: int
    wide_churn: bool
    wide_entry_bytes: int

    @classmethod
    def of(cls, spec: RunSpec) -> _Fixture:
        target = spec.environment.target
        repo = target["repository"]
        profile = target["setup_profile"]
        wide = profile.get("wide_dir")
        entry_bytes = 0
        if wide is not None:
            # The last entry has the widest index.
            last = wide_entry_path(wide["name"], wide["entries"] - 1)
            entry_bytes = len(last.encode("utf-8"))
        return cls(
            file_count=repo["file_count"],
            commit_count=repo["commit_count"],
            registers=tuple(repo["registers"]),
            type_changes=tuple(
                sorted({change["path"] for change in repo.get("type_changes", ())})
            ),
            scatter_dirty=profile["scatter_dirty"],
            wide_name=None if wide is None else wide["name"],
            wide_entries=0 if wide is None else wide["entries"],
            wide_churn=False if wide is None else wide["churn_all"],
            wide_entry_bytes=entry_bytes,
        )


def _generated_path_bytes(file_count: int) -> int:
    """Return the longest generated path any revision can hold, in bytes."""
    last = file_count - 1
    bucket = f"tree/a1/{last // FILES_PER_DIRECTORY:03d}/{last:05d}.dat"
    replaced = f"tree/a1/r{REPLACEMENT_STRIDE}/{last:05d}.dat"
    return max(len(bucket), len(replaced))


def _changed(file_count: int, low: int, high: int) -> int:
    """Return how many generated indexes change entering any revision in (low, high]."""
    return sum(
        0
        if revision - 1 >= file_count
        else (file_count - revision) // REPLACEMENT_STRIDE + 1
        for revision in range(low + 1, high + 1)
    )


def _named_paths(spec: RunSpec, fixture: _Fixture) -> set[str]:
    """Return every non-generated path the run can create or touch."""
    paths = {*fixture.registers, *CHURN_FILES}
    paths.update(f"{path}/entry.dat" for path in fixture.type_changes)
    for action in spec.actions:
        if action.kind == "mkdir":
            paths.add(action.payload["path"])
        elif action.kind in _MOVES:
            paths.add(action.payload["destination"])
    return paths


def _revisions(spec: RunSpec, fixture: _Fixture) -> tuple[int, int]:
    """Return the lowest and highest revision the run can check out or compare."""
    revisions = {fixture.commit_count - 1}
    for action in spec.actions:
        if action.kind in ("checkout", "status") and "revision" in action.payload:
            revisions.add(action.payload["revision"])
        if action.kind == "manifest_presence":
            revisions.update(
                (action.payload["from_revision"], action.payload["to_revision"])
            )
    return min(revisions), max(revisions)


def _compares_revisions(spec: RunSpec) -> bool:
    """Return whether a status can compare the tree with another revision.

    Only an explicit status revision does. A Thrift checkout does not:
    Sapling's `.` stays at the old revision and Sapling passes it as the
    status commit (`edenfs.rs:379-386`), which EdenFS refuses with
    OUT_OF_DATE_PARENT under `hg:enforce-parents` (the environment pins it
    true; `_THRIFT_STATUS_ERRORS`). A later `sl status --rev R` compares R
    against that stale parent rather than against the tree: it returned
    clean for `R` = the old `.` while the tree held another revision.
    Status collectors must not read such a status as tree-vs-R evidence.
    """
    return any(
        action.kind == "status" and "revision" in action.payload
        for action in spec.actions
    )


def _envelope(action: Action, fields: dict[str, Any]) -> int:
    """Return the bytes of the action's terminal event with every list empty."""
    data = {
        "action": "x" * MAX_ID_BYTES if len(action.id) < MAX_ID_BYTES else action.id,
        "actor": "x" * MAX_ID_BYTES,
        **fields,
    }
    return len(event_line(Event(OK, _TIME, data)))


def _status_bound(spec: RunSpec, action: Action, fixture: _Fixture) -> int:
    """Bound a status: each listed path appears once in `status` and in `diff_paths`.

    Paths are counted per class, each class at its longest encoded path:
    generated paths (starting dirt, and every path two compared revisions
    differ in), named paths, and wide entries.
    """
    # Per path: one status entry plus one diff_paths string, each with a comma.
    per_path = _json_bytes({"code": _LONGEST_CODE, "path": ""}) + 1 + 2 + 1
    generated = fixture.scatter_dirty
    wide = 0
    if _compares_revisions(spec):
        low, high = _revisions(spec, fixture)
        generated += 2 * _changed(fixture.file_count, low, high)
        if fixture.wide_churn:
            wide = fixture.wide_entries
    named = _named_paths(spec, fixture)
    path_bytes = (
        generated * (_generated_path_bytes(fixture.file_count) * 2 + per_path)
        + wide * (fixture.wide_entry_bytes * 2 + per_path)
        + sum(2 * (_json_bytes(path) - 2) + per_path for path in named)
    )
    fields = {"status": [], "diff_paths": [], "parent": "0" * 40}
    return _envelope(action, fields) + path_bytes


def _children(spec: RunSpec, directory: str, fixture: _Fixture) -> Iterator[str]:
    """Yield the names a non-generated path contributes to `directory`."""
    for path in _named_paths(spec, fixture):
        pure = PurePosixPath(path)
        for ancestor in (pure, *pure.parents):
            if str(ancestor.parent) == directory:
                yield ancestor.name


def _generated_children(directory: str, file_count: int) -> tuple[int, int]:
    """Return how many generated names `directory` can hold and the longest one."""
    last = file_count - 1
    file_name = len(f"{last:05d}.dat")
    if directory == "tree":
        return 2, 2
    if directory in ("tree/a0", "tree/a1"):
        buckets = math.ceil(file_count / FILES_PER_DIRECTORY)
        return buckets + REPLACEMENT_STRIDE, max(3, len(f"{last // 100:03d}"))
    parts = directory.split("/")
    if len(parts) == 3 and parts[1] in ("a0", "a1"):
        width = max(FILES_PER_DIRECTORY, math.ceil(file_count / REPLACEMENT_STRIDE))
        return width, file_name
    return 0, 0


def _listing_directory(spec: RunSpec, action: Action) -> str:
    if action.kind == "readdir_resume":
        return spec.action(action.payload["stream"]).payload["path"]
    return action.payload["path"]


def _listing_bound(spec: RunSpec, action: Action, fixture: _Fixture) -> int:
    directory = _listing_directory(spec, action)
    named = set(_children(spec, directory, fixture))
    count, longest = _generated_children(directory, fixture.file_count)
    count += len(named)
    longest = max([longest, *(_json_bytes(name) - 2 for name in named)])
    if directory == fixture.wide_name:
        count += fixture.wide_entries
        longest = max(longest, fixture.wide_entry_bytes - len(directory) - 1)
    fields = {
        "channel": "thrift",
        "entries": [],
        "stream": "x" * MAX_ID_BYTES,
        "pos_at_pause": 2**63,
        "pos_at_end": 2**63,
    }
    return _envelope(action, fields) + count * (longest + 3)


def _manifest_bound(spec: RunSpec, action: Action, fixture: _Fixture) -> int:
    low, high = sorted((action.payload["from_revision"], action.payload["to_revision"]))
    count = 2 * _changed(fixture.file_count, low, high) + 2 * len(fixture.type_changes)
    longest = max(
        [
            _generated_path_bytes(fixture.file_count),
            *(_json_bytes(f"{path}/entry.dat") - 2 for path in fixture.type_changes),
        ]
    )
    entry = _json_bytes({"path": "", "present": False}) + longest + 1
    fields = {"deleted_presence": [], "added_presence": []}
    return _envelope(action, fields) + count * entry


def _journal_bound(spec: RunSpec, action: Action, fixture: _Fixture) -> int:
    """Bound a journal window: FUSE-level changes only; checkouts are commit transitions."""
    named = _named_paths(spec, fixture)
    count = fixture.scatter_dirty + len(named)
    longest = max(
        _generated_path_bytes(fixture.file_count),
        max(_json_bytes(path) - 2 for path in named),
    )
    entry = (
        _json_bytes({"existed_after": False, "existed_before": False, "path": ""})
        + longest
        + 1
    )
    fields = {
        "mount_generation": 2**63,
        "sequence_number": 2**63,
        "since_sequence": 2**63,
        "changed_paths": [],
        "truncated": False,
        "lost_changes": ["JOURNAL_TRUNCATED", "MOUNT_GENERATION_CHANGED"],
    }
    return _envelope(action, fields) + count * entry


_BOUNDS = {
    "status": _status_bound,
    "readdir": _listing_bound,
    "readdir_resume": _listing_bound,
    "manifest_presence": _manifest_bound,
    "journal_window": _journal_bound,
}


def observation_bound(spec: RunSpec, action: Action) -> int | None:
    """Return an upper bound on one complete observation's terminal event, in bytes.

    Returns None for an action that records no complete observation. A
    differential run's bound includes the reference side's copy.
    """
    bound = _BOUNDS.get(action.kind)
    if bound is None:
        return None
    single = bound(spec, action, _Fixture.of(spec))
    if spec.environment.target["setup_profile"].get("reference_clone", False):
        return 2 * single + _LONGEST_REASON
    return single


def oversized(spec: RunSpec) -> Iterable[tuple[Action, int]]:
    """Yield each complete observation whose bound exceeds `MAX_EVENT_BYTES`."""
    for action in spec.actions[:MAX_ACTIONS]:
        bound = observation_bound(spec, action)
        if bound is not None and bound > MAX_EVENT_BYTES:
            yield action, bound


def validate_observation_sizes(spec: RunSpec) -> None:
    """Refuse a spec whose complete observation could exceed `MAX_EVENT_BYTES`.

    Such an event would make the whole history invalid; the spec is
    refused before it runs instead. Larger observations need a chunked
    format, which the harness does not have.
    """
    for action, bound in oversized(spec):
        raise SpecError(
            f"{action.kind} {action.id!r} can record up to {bound} bytes in one "
            f"event, over the {MAX_EVENT_BYTES}-byte event limit"
        )
