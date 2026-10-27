# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""File-operation contracts: write, rename, unlink, mkdir, chmod, descriptors
for paths, `sl rm`, and `sl mv`.

`sl rm --force` and `sl mv` act as functions of pre-state:
guarded effects per matched path, a precondition for exit 0, and an
observation for the one failing exit status each has.
"""

from __future__ import annotations

import errno
from collections.abc import Callable, Iterator, Mapping, Sequence
from dataclasses import dataclass, replace
from pathlib import PurePosixPath
from typing import Any

from eden.integration.correctness.exploratory.core.checker import (
    failure_use,
    fault_excuses,
    targeting_faults,
)
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    ANY,
    Atom,
    AtomOp,
    Cell,
    Effect,
    EffectGroup,
    EffectKind,
    eq,
    Expect,
    Guard,
    none_of,
    normalize_path,
    ObligationRef,
    Observation,
    of_class,
    one_of,
    PRIMARY,
    Requirement,
    ROOT,
    Status,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import (
    ActionFacts,
    ErrnoUse,
    HistoryFacts,
    NO_ERRNO,
    Outcome,
    ResultSchema,
)
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.edenfs.actions import (
    GENERATED_DIRECTORIES,
    MOVE_KINDS,
)
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.fixture import DIRECTORY, FILE

from .model import (
    _ancestors,
    _Context,
    _descendants,
    _errno_of,
    _field,
    _group,
    _parent_path,
    _path_part,
    _produce,
    _rank_keys,
    _single,
    CLEAN,
    DESCRIPTOR,
    DESCRIPTOR_BINDING,
    DESCRIPTOR_OBJECT,
    DIR,
    file_value,
    INPUT_LEGALITY_RULE,
    mode_cell,
    mode_value,
    NEVER,
    parent_cell,
    PATH,
    path_cell,
    rev_value,
    SAPLING_PARENT,
    track_cell,
)


REMOVED = Value("removed")


OPENED = Value("open")


PRECONDITION_RULE = "namespace.precondition"


DESCRIPTOR_RULE = "fd.stale-read"


def added_value(source: str) -> Value:
    """Return the `track` value of a path Sapling records as added."""
    return Value("added", (source,))


def descriptor_keys(facts: HistoryFacts) -> dict[str, str]:
    """Return each invoked open's descriptor key: `d<rank>` (`_rank_keys`)."""
    return _rank_keys(facts, ("open",), "d")


def descriptor_cell(key: str, side: str = PRIMARY) -> Cell:
    """Return the cell an open marks when it takes effect; `key` is from `descriptor_keys`."""
    return Cell(side, DESCRIPTOR, key)


def binding_cell(key: str, path: str, side: str = PRIMARY) -> Cell:
    """Return the cell saying whether `path` binds the descriptor's object."""
    return Cell(side, DESCRIPTOR_BINDING, f"{key}/{normalize_path(path)}")


def object_cell(key: str, path: str, side: str = PRIMARY) -> Cell:
    """Return the descriptor's mirror of `path`; for its own path, its object's content after the open."""
    return Cell(side, DESCRIPTOR_OBJECT, f"{key}/{normalize_path(path)}")


def write_value(spec: RunSpec, action: Action) -> Value:
    """Return the `path` value one planned write produces."""
    data = encode_value(spec.seed, spec.owner(action.id), action.payload["value_id"])
    return file_value(data)


def _chmod_value(action: Action) -> Value:
    """Return the `mode` value one planned chmod produces."""
    return mode_value(bool(action.payload["mode"] & 0o111))


def _transfer(
    cell: Cell, source: Cell, provenance: str, guard: Guard | None = None
) -> Effect:
    return Effect(cell, EffectKind.TRANSFER, None, source, provenance, guard)


def _requirement(
    ctx: _Context, action: Action, cells: tuple[Cell, ...], expect: Expect
) -> tuple[Requirement, ...]:
    ref = ctx.obligation(PRECONDITION_RULE, action.id)
    return () if ref is None else (Requirement(cells, expect, ref),)


_PRESENT = of_class({FILE, DIRECTORY})


def _write_groups(
    ctx: _Context, action: Action, status: Status, window: Window
) -> Iterator[EffectGroup]:
    """A write sets its path's bytes; it needs a file there (no O_CREAT)."""
    cell = path_cell(action.payload["path"])
    yield _group(
        action,
        status,
        window,
        (_produce(cell, write_value(ctx.spec, action), action.id),),
        requires=_requirement(ctx, action, (cell,), _single(of_class({FILE}))),
    )


def _moved_pairs(ctx: _Context, source: str, destination: str) -> list[tuple[str, str]]:
    """Return (source, destination) path pairs a move carries, the moved path first.

    A moved directory carries every relevant path under it; a relevant
    path under the destination with no counterpart is replaced by nothing.
    """
    source, destination = normalize_path(source), normalize_path(destination)
    pairs = [(source, destination)]
    pairs += [
        (path, destination + path[len(source) :]) for path in _descendants(ctx, source)
    ]
    return pairs


def _move_effects(ctx: _Context, action: Action) -> tuple[Effect, ...]:
    """Transfer the moved subtree's objects and modes, replacing what the destination held."""
    pairs = _moved_pairs(ctx, action.payload["source"], action.payload["destination"])
    effects: list[Effect] = []
    for source, destination in pairs:
        effects += [
            _transfer(path_cell(destination), path_cell(source), action.id),
            _transfer(mode_cell(destination), mode_cell(source), action.id),
        ]
    carried = {destination for _source, destination in pairs}
    for orphan in _descendants(ctx, pairs[0][1]):
        if orphan not in carried:
            effects += [
                _produce(path_cell(orphan), ABSENT, action.id),
                _produce(mode_cell(orphan), ABSENT, action.id),
            ]
    return tuple(effects)


def _rename_groups(
    ctx: _Context, action: Action, status: Status, window: Window
) -> Iterator[EffectGroup]:
    """A rename transfers the source's subtree, objects and modes, replacing the destination."""
    source = action.payload["source"]
    yield _group(
        action,
        status,
        window,
        _move_effects(ctx, action),
        requires=_requirement(ctx, action, (path_cell(source),), _single(_PRESENT)),
    )


def _unlink_groups(
    ctx: _Context, action: Action, status: Status, window: Window
) -> Iterator[EffectGroup]:
    """An unlink removes a file and its mode."""
    path = action.payload["path"]
    yield _group(
        action,
        status,
        window,
        (
            _produce(path_cell(path), ABSENT, action.id),
            _produce(mode_cell(path), ABSENT, action.id),
        ),
        requires=_requirement(
            ctx, action, (path_cell(path),), _single(of_class({FILE}))
        ),
    )


def _mkdir_groups(
    ctx: _Context, action: Action, status: Status, window: Window
) -> Iterator[EffectGroup]:
    """A mkdir binds a directory at an absent path."""
    path = action.payload["path"]
    yield _group(
        action,
        status,
        window,
        (
            _produce(path_cell(path), DIR, action.id),
            _produce(mode_cell(path), mode_value(True), action.id),
        ),
        requires=_requirement(ctx, action, (path_cell(path),), _single(eq(ABSENT))),
    )


def _chmod_groups(
    ctx: _Context, action: Action, status: Status, window: Window
) -> Iterator[EffectGroup]:
    """A chmod sets the executable bit of a present path."""
    path = action.payload["path"]
    yield _group(
        action,
        status,
        window,
        (_produce(mode_cell(path), _chmod_value(action), action.id),),
        requires=_requirement(ctx, action, (path_cell(path),), _single(_PRESENT)),
    )


def _tracked_rows(
    ctx: _Context, path: str, *, removed: bool
) -> list[tuple[Atom, Atom]]:
    """Return rows over `(EdenFS parent, track)` under which Sapling tracks `path`.

    A file is in the dirstate when it is in the parent's manifest and not
    marked removed, or added; with `removed`, a path marked removed counts
    too (`sl rm` of an already-removed file exits 0).
    """
    rows: list[tuple[Atom, Atom]] = [(ANY, of_class({"added"}))]
    if removed:
        rows.append((ANY, eq(REMOVED)))
    rows += [
        (eq(rev_value(revision)), eq(CLEAN))
        for revision in ctx.candidates
        if ctx.pinned.committed(path, revision).cls == FILE
    ]
    return rows


def _untracked_atom(
    ctx: _Context, path: str, revision: int, *, removed: bool
) -> Atom | None:
    """Return the `track` atom under which Sapling does not track `path` at `revision`, or None.

    `removed` says a path marked removed counts as untracked (a directory
    walk skips it; an explicitly named file does not).
    """
    if ctx.pinned.committed(path, revision).cls == FILE:
        return eq(REMOVED) if removed else None
    return one_of({CLEAN, REMOVED}) if removed else eq(CLEAN)


def _committed_dir(ctx: _Context, path: str) -> bool:
    """Return whether some candidate revision holds `path` as a directory."""
    return any(ctx.pinned.committed(path, rev) == DIR for rev in ctx.candidates)


def _maybe_dir(ctx: _Context, path: str) -> bool:
    """Return whether `path` may be a directory: committed as one, made, or moved to."""
    path = normalize_path(path)
    if _committed_dir(ctx, path):
        return True
    return any(
        (action.kind == "mkdir" and normalize_path(action.payload["path"]) == path)
        or (
            action.kind in MOVE_KINDS
            and normalize_path(action.payload["destination"]) == path
        )
        for action in ctx.spec.actions
    )


def _prune_groups(
    ctx: _Context, action: Action, status: Status, window: Window, path: str
) -> Iterator[EffectGroup]:
    """Sapling may prune a directory its command emptied.

    Each relevant directory above `path`, and `path` itself, may vanish
    once every relevant path under it is absent: an optional group inside
    the command's window. Unmodeled children are not known, so it stays
    optional.
    """
    for directory in (path, *_ancestors(path)):
        if directory not in ctx.relevant_set:
            continue
        children = _descendants(ctx, directory)
        if not children:
            continue
        cells = (path_cell(directory), *(path_cell(child) for child in children))
        guard = Guard(cells, Expect(((eq(DIR), *(eq(ABSENT) for _ in children)),)))
        yield _group(
            action,
            Status.INDETERMINATE,
            window,
            (
                _produce(path_cell(directory), ABSENT, action.id, guard),
                _produce(mode_cell(directory), ABSENT, action.id, guard),
            ),
            part=f"{_path_part(directory)}:prune",
        )


def _sl_rm_groups(
    ctx: _Context, action: Action, status: Status, window: Window
) -> Iterator[EffectGroup]:
    """`sl rm --force` as a function of each matched file's pre-state.

    For the path and every relevant path under it: a file in the parent's
    manifest and not marked removed is unlinked and marked removed; an
    added file is forgotten (`track` back to clean) and kept on disk; any
    other path is untouched. Emptied directories may vanish
    (`_prune_groups`). Exit 0 needs some matched path tracked (the
    precondition), including an explicitly named already-removed file.
    """
    path = normalize_path(action.payload["path"])
    removes: list[Effect] = []
    forgets: list[Effect] = []
    for matched in (path, *_descendants(ctx, path)):
        rows = [
            (eq(rev_value(revision)), eq(CLEAN))
            for revision in ctx.candidates
            if ctx.pinned.committed(matched, revision).cls == FILE
        ]
        if rows:
            guard = Guard((parent_cell(), track_cell(matched)), Expect(tuple(rows)))
            removes += [
                _produce(path_cell(matched), ABSENT, action.id, guard),
                _produce(mode_cell(matched), ABSENT, action.id, guard),
                _produce(track_cell(matched), REMOVED, action.id, guard),
            ]
        added = Guard((track_cell(matched),), _single(of_class({"added"})))
        forgets.append(_produce(track_cell(matched), CLEAN, action.id, added))
    requires = _requirement(ctx, action, *_sl_rm_precondition(ctx, path))
    main = _group(action, status, window, tuple(removes or forgets), requires=requires)
    yield main
    if removes:
        yield replace(
            _group(action, status, window, tuple(forgets), part="forget"),
            outcome_var=main.outcome_var,
        )
    yield from _prune_groups(ctx, action, status, window, path)


def _sl_rm_precondition(ctx: _Context, path: str) -> tuple[tuple[Cell, ...], Expect]:
    """Return the cells and rows of "some matched path is tracked" (`sl rm` exit 0)."""
    matched = (path, *_descendants(ctx, path))
    cells = (parent_cell(), *(track_cell(item) for item in matched))
    rows: list[tuple[Atom, ...]] = []
    if _committed_dir(ctx, path):
        rows += [
            (eq(rev_value(rev)), *(ANY for _ in matched))
            for rev in ctx.candidates
            if ctx.pinned.committed(path, rev) == DIR
        ]
    for index, item in enumerate(matched):
        for parent, track in _tracked_rows(ctx, item, removed=index == 0):
            row = [ANY for _ in matched]
            row[index] = track
            rows.append((parent, *row))
    return cells, Expect(tuple(dict.fromkeys(rows)))


def _sl_mv_groups(
    ctx: _Context, action: Action, status: Status, window: Window
) -> Iterator[EffectGroup]:
    """`sl mv S D` as a function of the pre-state of S and D.

    A file S Sapling tracks moves when it is present and D is absent: D
    takes its object and mode and is recorded as a copy, S is marked
    removed, and missing directories above D are made. A tracked S absent
    from disk is only marked removed ("half move"). Validation keeps D from
    ever being a directory (`_validate_move`). When D exists as a file
    nothing changes. A directory S moves its
    subtree when D is absent. Exit 0 needs S to have a tracked file (the
    precondition); exit 0 never by itself implies the move.
    """
    source = normalize_path(action.payload["source"])
    destination = normalize_path(action.payload["destination"])
    tracked = _tracked_rows(ctx, source, removed=False)
    if _maybe_dir(ctx, source):
        tracked = [(ANY, ANY)]
    cells = (
        parent_cell(),
        track_cell(source),
        path_cell(source),
        path_cell(destination),
    )
    moving = Guard(
        cells, Expect(tuple((*row, _PRESENT, eq(ABSENT)) for row in tracked))
    )
    removed = Guard(
        cells,
        Expect(
            tuple(
                (*row, *pair)
                for row in tracked
                for pair in (
                    (_PRESENT, eq(ABSENT)),
                    (eq(ABSENT), ANY),
                )
            )
        ),
    )
    effects: list[Effect] = []
    pairs = _moved_pairs(ctx, source, destination)
    for index, (moved, landing) in enumerate(pairs):
        guard = moving
        if index:
            guard = Guard(
                (*cells, path_cell(moved)),
                Expect(tuple((*row, _PRESENT) for row in moving.expect.rows)),
            )
        effects += [
            _transfer(path_cell(landing), path_cell(moved), action.id, guard),
            _transfer(mode_cell(landing), mode_cell(moved), action.id, guard),
            _produce(track_cell(landing), added_value(moved), action.id, guard),
        ]
        if index:
            effects.append(_produce(track_cell(moved), REMOVED, action.id, guard))
    effects.append(_produce(track_cell(source), REMOVED, action.id, removed))
    for directory in _ancestors(destination):
        if directory not in ctx.relevant_set:
            continue
        made = Guard(
            (*cells, path_cell(directory)),
            Expect(tuple((*row, eq(ABSENT)) for row in moving.expect.rows)),
        )
        effects += [
            _produce(path_cell(directory), DIR, action.id, made),
            _produce(mode_cell(directory), mode_value(True), action.id, made),
        ]
    requires = _requirement(
        ctx,
        action,
        (parent_cell(), track_cell(source)),
        Expect(tuple(dict.fromkeys(tracked))),
    )
    yield _group(action, status, window, tuple(effects), requires=requires)
    yield from _prune_groups(ctx, action, status, window, _parent_path(source))


def process_failure_observation(ctx: _Context, action: Action) -> Observation | None:
    """Return what a failed `sl rm`/`sl mv` says about state, for `core.input-legality`.

    Each has one failing exit status with no effect: `sl rm
    --force` exits 1 iff no matched path is tracked, `sl mv` exits 255 iff
    the source has no tracked file. Both are read at one point inside the
    call against the EdenFS parent's manifest. A path that may be a
    directory with unmodeled files, and any other exit status, is not
    observed (not evaluated).
    """
    recorded = ctx.facts.action(action.id)
    ref = ctx.obligation(INPUT_LEGALITY_RULE, action.id)
    if (
        ref is None
        or recorded.outcome != Outcome.FAIL
        or recorded.completed is None
        or recorded.result is None
    ):
        return None
    assert recorded.invoked is not None
    code = _field(recorded, "returncode")
    if action.kind == "sl_rm" and code == 1:
        path = normalize_path(action.payload["path"])
        matched = (path, *_descendants(ctx, path))
    elif action.kind == "sl_mv" and code == 255:
        path = normalize_path(action.payload["source"])
        if _maybe_dir(ctx, path):
            return None
        matched = (path,)
    else:
        return None
    rows: list[tuple[Atom, ...]] = []
    for revision in ctx.candidates:
        if ctx.pinned.committed(path, revision) == DIR:
            rows.append((eq(rev_value(revision)), *(ANY for _ in matched)))
            continue
        atoms = [
            atom
            for index, item in enumerate(matched)
            if (
                atom := _untracked_atom(
                    ctx, item, revision, removed=index > 0 or action.kind == "sl_mv"
                )
            )
            is not None
        ]
        if len(atoms) == len(matched):
            rows.append((eq(rev_value(revision)), *atoms))
    return Observation(
        id=f"o:{action.id}:exit",
        action=action.id,
        cells=(parent_cell(), *(track_cell(item) for item in matched)),
        expect=Expect(tuple(rows) or ((NEVER, *(ANY for _ in matched)),)),
        window=Window(recorded.invoked, recorded.completed),
        channel="sl",
        obligations=(ref,),
    )


# The `error_code` values a Thrift read's EdenError reports with POSIX
# meaning: each says something about the path's state (ENOTDIR: the path
# walk met a file, `TreeInode::getOrLoadChildTree`).
_THRIFT_READ_STATE = frozenset({"ENOENT", "EISDIR", "ENOTDIR"})


# The (errorType, errno) pairs a Thrift readdir reports for a path's state
# on a live mount, named by the errno they carry.
_THRIFT_READDIR_STATE: Mapping[tuple[str, int], str] = {
    ("POSIX_ERROR", errno.ENOENT): "ENOENT",
    ("POSIX_ERROR", errno.ENOTDIR): "ENOTDIR",
    ("ARGUMENT_ERROR", errno.EINVAL): "EINVAL",
}


# The meaning of a failure no state allows.
_FORBIDDEN = "FORBIDDEN"


# One alternative a state errno allows: the class each named cell holds.
_Row = tuple[tuple[str, str, Atom], ...]


def _absent_parent(path: str) -> tuple[_Row, ...]:
    parent = _parent_path(path)
    return () if parent == ROOT else (((PATH, parent, eq(ABSENT)),),)


def _file_on_the_way(path: str, *, itself: bool) -> tuple[_Row, ...]:
    """Return the rows saying a proper ancestor of `path` (or, with `itself`, it) is a file."""
    above = tuple(
        ((PATH, ancestor, of_class({FILE})),) for ancestor in _ancestors(path)
    )
    return (((PATH, path, of_class({FILE})),), *above) if itself else above


def _readdir_notdir(payload: Mapping[str, Any]) -> tuple[_Row, ...]:
    """FUSE: the path or a directory above it is a file. Thrift: one above it is."""
    return _file_on_the_way(payload["path"], itself=payload["channel"] != "thrift")


def _readdir_einval(payload: Mapping[str, Any]) -> tuple[_Row, ...]:
    """Thrift: the path is a regular file. FUSE readdir never fails EINVAL."""
    if payload["channel"] != "thrift":
        return (((PATH, payload["path"], NEVER),),)
    return (((PATH, payload["path"], of_class({FILE})),),)


# Each state errno's meaning: the alternative rows the cells it names must meet.
_STATE_MEANINGS: Mapping[
    tuple[str, str], Callable[[Mapping[str, Any]], tuple[_Row, ...]]
] = {
    ("write", "ENOENT"): lambda p: (((PATH, p["path"], eq(ABSENT)),),),
    ("write", "EISDIR"): lambda p: (((PATH, p["path"], eq(DIR)),),),
    ("read", "ENOENT"): lambda p: (((PATH, p["path"], eq(ABSENT)),),),
    ("read", "EISDIR"): lambda p: (((PATH, p["path"], eq(DIR)),),),
    ("read", _FORBIDDEN): lambda p: (((PATH, p["path"], NEVER),),),
    ("getattr", "ENOENT"): lambda p: (((PATH, p["path"], eq(ABSENT)),),),
    ("open", "ENOENT"): lambda p: (((PATH, p["path"], eq(ABSENT)),),),
    ("chmod", "ENOENT"): lambda p: (((PATH, p["path"], eq(ABSENT)),),),
    ("unlink", "ENOENT"): lambda p: (((PATH, p["path"], eq(ABSENT)),),),
    ("unlink", "EISDIR"): lambda p: (((PATH, p["path"], eq(DIR)),),),
    ("mkdir", "EEXIST"): lambda p: (((PATH, p["path"], _PRESENT),),),
    ("mkdir", "ENOENT"): lambda p: _absent_parent(p["path"]),
    ("readdir", "ENOENT"): lambda p: (((PATH, p["path"], eq(ABSENT)),),),
    ("readdir", "ENOTDIR"): _readdir_notdir,
    ("readdir", "EINVAL"): _readdir_einval,
    ("readdir", _FORBIDDEN): lambda p: (((PATH, p["path"], NEVER),),),
    # A missing source, or a missing directory the destination is named in
    # (the kernel fails that lookup before any FUSE rename).
    ("rename", "ENOENT"): lambda p: (
        ((PATH, p["source"], eq(ABSENT)),),
        *_absent_parent(p["destination"]),
    ),
    ("rename", "EISDIR"): lambda p: (
        ((PATH, p["source"], of_class({FILE})), (PATH, p["destination"], eq(DIR))),
    ),
    ("rename", "ENOTDIR"): lambda p: (
        ((PATH, p["source"], eq(DIR)), (PATH, p["destination"], of_class({FILE}))),
        *_file_on_the_way(p["source"], itself=False),
        *_file_on_the_way(p["destination"], itself=False),
    ),
    # The kernel's path walk finds a file above the path.
    **{
        (kind, "ENOTDIR"): lambda p: _file_on_the_way(p["path"], itself=False)
        for kind in ("write", "read", "getattr", "open", "chmod", "unlink", "mkdir")
    },
}


# A rename onto a non-empty directory fails with either errno; their rows
# read the run's relevant paths (`nonempty_destination_rows`).
NONEMPTY_DESTINATION_ERRNOS = frozenset({("rename", "ENOTEMPTY"), ("rename", "EEXIST")})


def nonempty_destination_rows(
    ctx: _Context, payload: Mapping[str, Any]
) -> tuple[_Row, ...] | None:
    """Return the rows of a rename failing `ENOTEMPTY`/`EEXIST`, or None when undecided.

    The source and the destination are directories and some path under
    the destination is present: one row per relevant path under it. Away
    from the generated trees and the wide directory every child is a
    reserved path, a type-change path or its child, a path some action
    names, or where a move carries one of those (all relevant), and a
    non-root `.eden` is resolved on lookup, never a directory entry; no
    row means the destination is empty. A destination that is, holds, or
    lies in a tree with untouched generated entries is None.
    """
    destination = normalize_path(payload["destination"])
    roots: list[tuple[str, ...]] = list(GENERATED_DIRECTORIES)
    wide = ctx.pinned.fixture.layout.wide_dir
    if wide is not None:
        roots.append(PurePosixPath(wide.name).parts)
    parts = PurePosixPath(destination).parts
    if any(parts[: len(root)] == root[: len(parts)] for root in roots):
        return None
    return tuple(
        (
            (PATH, payload["source"], eq(DIR)),
            (PATH, destination, eq(DIR)),
            (PATH, child, _PRESENT),
        )
        for child in _descendants(ctx, destination)
    )


# The STATE errnos of modeled inputs that no row above judges: the
# observation comes from elsewhere (an `sl rm`/`sl mv` exit status,
# `process_failure_observation`; `read_fd` EISDIR, the descriptor's object,
# `_directory_object_observation`; a failed status, collectors) or the
# obligation stays not evaluated.
UNJUDGED_STATE_ERRNOS = frozenset(
    {
        ("read_fd", "EISDIR"),
        ("read_fd", "EBADF"),
        ("sl_rm", NO_ERRNO),
        ("sl_mv", NO_ERRNO),
        ("readdir", NO_ERRNO),
        ("status", NO_ERRNO),
    }
)


def failure_errno_name(facts: ActionFacts) -> str | None:
    """Return the state-meaningful errno name of a failure, or None.

    A syscall failure names its `errno`. A Thrift read's EdenError carries
    the same POSIX code in `error_code` and means the same state. Any
    other errno-less read failure (another `error_code`, a transport
    error, an empty read) is `_FORBIDDEN`: no state allows it, so only a
    nemesis excuses it. A Thrift readdir is matched on its (errorType,
    errno) pair (`_THRIFT_READDIR_STATE`); any other pair, a typed error
    without an errno included (readdir is never refused for checkout
    reasons, so `CHECKOUT_IN_PROGRESS` is one), is `_FORBIDDEN`, and a
    failure without a recorded type stays unobserved. A Sapling exit
    status carries none.
    """
    code = _errno_of(facts)
    if code is None and facts.kind == "read":
        error_code = _field(facts, "error_code")
        name = errno.errorcode.get(error_code) if type(error_code) is int else None
        return name if name in _THRIFT_READ_STATE else _FORBIDDEN
    if facts.kind == "readdir" and facts.payload.get("channel") == "thrift":
        error_type = _field(facts, "error_type")
        if not isinstance(error_type, str):
            return None
        if code is None:
            return _FORBIDDEN
        return _THRIFT_READDIR_STATE.get((error_type, code), _FORBIDDEN)
    return None if code is None else errno.errorcode.get(code)


def _fault_caused(
    history: HistoryFacts, recorded: ActionFacts, schema: ResultSchema
) -> bool:
    """Return whether a fault targeting the action can cause its failure.

    `core.input-legality` alone judges such a failure: a state observation
    would judge it as if the product made it. The core's `fault_excuses`
    table decides, on the failure as `failure_errno_name` names it (a
    Thrift read's `error_code`, a readdir's typed pair): `_FORBIDDEN` is
    a failure only a nemesis explains (FAULT), and a named state errno is
    still the product's. A failure it does not name keeps the core's
    `failure_use` classification.
    """
    name = failure_errno_name(recorded)
    if name is None:
        assert recorded.result is not None
        name, use = failure_use(schema, recorded.result)
    else:
        use = ErrnoUse.FAULT if name == _FORBIDDEN else schema.errnos.get(name)
    return any(
        fault_excuses(install.payload.get("behavior"), use, name)
        for install in targeting_faults(history, recorded.id)
    )


def _meaning_rows(rows: tuple[_Row, ...]) -> tuple[tuple[Cell, ...], Expect]:
    """Return the cells a meaning names and its rows over them, ANY where a row is silent."""
    cells = tuple(
        dict.fromkeys(
            Cell(PRIMARY, space, normalize_path(key))
            for row in rows
            for space, key, _atom in row
        )
    )
    expect = []
    for row in rows:
        atoms = {
            Cell(PRIMARY, space, normalize_path(key)): atom for space, key, atom in row
        }
        expect.append(tuple(atoms.get(cell, ANY) for cell in cells))
    return cells, Expect(tuple(dict.fromkeys(expect)))


# `close` is not a modeled input, so only `read_fd` owns an obligation.
_UNOPENED = frozenset({("read_fd", "EBADF")})


def _unopened_observation(
    ctx: _Context, action: Action, recorded: ActionFacts, ref: ObligationRef
) -> Observation | None:
    """Return what an EBADF `read_fd` says: nothing about state.

    A backend fails a descriptor operation EBADF only when it holds no
    descriptor, because the open failed or never completed. That is
    consistent with an open that was not acked in every state, so the
    observation holds anywhere (`ANY` on the opened path). After an acked
    open no descriptor operation fails EBADF; the obligation then stays
    not evaluated.
    """
    if recorded.completed is None:
        return None
    assert recorded.invoked is not None
    if ctx.facts.action(action.payload["fd"]).outcome == Outcome.OK:
        return None
    path = ctx.spec.action(action.payload["fd"]).payload["path"]
    return Observation(
        id=f"o:{action.id}:ebadf",
        action=action.id,
        cells=(Cell(PRIMARY, PATH, normalize_path(path)),),
        expect=Expect(((ANY,),)),
        window=Window(recorded.invoked, recorded.completed),
        channel="fuse",
        obligations=(ref,),
    )


def _out_of_date_rows(ctx: _Context, action: Action) -> tuple[tuple[Cell, ...], Expect]:
    """Return the cells and rows of an `OUT_OF_DATE_PARENT` refusal.

    getScmStatusV2 refuses a commit other than EdenFS's parent. The commit
    is the payload revision, or Sapling's `.` when it names none
    (`backend_scm.status_thrift`), so the refusal says the two differ.
    """
    revision = action.payload.get("revision")
    if revision is not None:
        return (parent_cell(),), _single(none_of({rev_value(revision)}))
    rows = tuple(
        (eq(rev_value(eden)), eq(rev_value(sapling)))
        for eden in ctx.candidates
        for sapling in ctx.candidates
        if eden != sapling
    )
    cells = (parent_cell(), parent_cell(SAPLING_PARENT))
    return cells, Expect(rows or ((NEVER, ANY),))


@dataclass(frozen=True)
class DescriptorGap:
    """Where a held descriptor's object is not modeled, and why.

    A write through a path the object can reach overlaps a checkout that
    changes that path, inside `window` (the two windows' overlap): the
    write may land in the inode the checkout replaces.
    """

    window: Window
    reason: str


def _move_pairs(ctx: _Context) -> list[tuple[str, str]]:
    pairs: list[tuple[str, str]] = []
    for action in ctx.spec.actions:
        if action.kind in MOVE_KINDS:
            payload = action.payload
            pairs += _moved_pairs(ctx, payload["source"], payload["destination"])
    return pairs


def _reachable(path: str, pairs: list[tuple[str, str]]) -> frozenset[str]:
    found = {path}
    grown = True
    while grown:
        grown = False
        for source, destination in pairs:
            if (source in found) != (destination in found):
                found |= {source, destination}
                grown = True
    return frozenset(found)


def _conjoined(
    cells: tuple[Cell, ...], rows: Sequence[tuple[Atom, ...]], guard: Guard | None
) -> Guard:
    """Return a guard over `cells` + the guard's cells: each row of ours with each of its."""
    if guard is None:
        return Guard(cells, Expect(tuple(rows)))
    return Guard(
        (*cells, *guard.cells),
        Expect(tuple((*row, *other) for row in rows for other in guard.expect.rows)),
    )


def _excluding(atom: Atom, value: Value) -> Atom | None:
    """Return `atom` without `value`, or None when nothing else is left.

    Raises ValueError for a class atom admitting `value`'s class: the
    difference has no atom form.
    """
    if not atom.matches(value):
        return atom
    if atom.op == AtomOp.ANY:
        return none_of({value})
    if atom.op == AtomOp.NOT_IN:
        return none_of(atom.values | {value})
    if atom.op in (AtomOp.EQ, AtomOp.IN):
        rest = atom.values - {value}
        return one_of(rest) if rest else None
    raise ValueError(f"no atom form for {atom.op} without one value")


def _split_on_value(
    guard: Guard | None, cell: Cell, value: Value
) -> tuple[Guard | None, Guard | None]:
    """Split `guard` by whether `cell` holds `value` just before its group.

    Returns the guard of the states where `cell` differs from `value`
    and the guard of the states where it may equal it, either None when
    no state is left. A row whose difference has no atom form goes whole
    to the second guard, which only ever widens what may happen there.
    """
    if guard is None:
        cells: tuple[Cell, ...] = (cell,)
        rows: list[tuple[Atom, ...]] = [(ANY,)]
    elif cell in guard.cells:
        cells, rows = guard.cells, list(guard.expect.rows)
    else:
        cells = (cell, *guard.cells)
        rows = [(ANY, *row) for row in guard.expect.rows]
    index = cells.index(cell)
    differs: list[tuple[Atom, ...]] = []
    equal: list[tuple[Atom, ...]] = []
    for row in rows:
        atom = row[index]
        try:
            narrowed = _excluding(atom, value)
        except ValueError:
            equal.append(row)
            continue
        if narrowed is not None:
            differs.append((*row[:index], narrowed, *row[index + 1 :]))
        if atom.matches(value):
            equal.append((*row[:index], eq(value), *row[index + 1 :]))

    def guard_of(found: list[tuple[Atom, ...]]) -> Guard | None:
        return Guard(cells, Expect(tuple(dict.fromkeys(found)))) if found else None

    return guard_of(differs), guard_of(equal)
