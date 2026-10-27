# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Cells, values, and the per-run context every EdenFS contract reads.

Cells (all keyed canonically):

| space | key | values |
|---|---|---|
| `path` | normalized path | `absent`; `file` (sha256 hex,); `dir` |
| `mode` | path | `absent`; `mode` (executable,) |
| `track` | path | `clean`; `removed`; `added` (copy source,) |
| `parent` | `""` (EdenFS) or `sapling` | `rev` (revision,) |
| `source` | `<checkout key>/r<revision>` | `absent`; `yes` |
| `visit` | `<checkout key>/<path>` | `absent`; `yes` |
| `dryrun` | `<checkout key>/<path>` | `absent`; `yes` |
| `blocked` | `<checkout key>/r<revision>/<path>` | `absent`; `yes` |
| `planned` | `<checkout key>/r<revision>/<path>` | `absent`; `yes` |
| `updatestate` | `""` | `absent`; `rev` (revision,) |
| `interrupted` | `""` | `absent`; `checkout` (checkout key,) |
| `conflict` | `<checkout key>/<path>` | `absent`; `yes` |
| `fd` | `<descriptor key>` | `absent`; `open` |
| `fd-binding` | `<descriptor key>/<path>` | `absent`; `bound` |
| `fd-object` | `<descriptor key>/<path>` | as `path` |

A checkout key (`checkout_keys`) is the checkout's rank by invocation
time, a descriptor key (`descriptor_keys`) is the open's rank, and an op
key (`_Op.key`) is a write's or chmod's rank, so cell keys carry no
action ID and solver input content stays independent of how actions are
named. A checkout's supersede mark for an op (`_supersede_groups`) is a
`visit` cell keyed `<checkout key>:<op key>/<path>`.
"""

from __future__ import annotations

import bisect
import errno
import hashlib
import math
from collections.abc import Callable, Iterator, Mapping, Sequence
from dataclasses import dataclass
from enum import StrEnum
from pathlib import PurePosixPath
from typing import Any

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
    Footprint,
    Guard,
    none_of,
    normalize_path,
    ObligationRef,
    of_class,
    one_of,
    PinnedFacts,
    PRIMARY,
    Requirement,
    ROOT,
    Status,
    UnknownInitial,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import (
    ActionFacts,
    ErrnoUse,
    HistoryFacts,
    Outcome,
)
from eden.integration.correctness.exploratory.core.history import INFO, RUN_CLOSED
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.core.sites import POST_EFFECT
from eden.integration.correctness.exploratory.core.target import (
    Binding,
    MUTATING_ROLES,
    Role,
)
from eden.integration.correctness.exploratory.edenfs.actions import ACTION_DEFS
from eden.integration.correctness.exploratory.edenfs.encoding import content_token
from eden.integration.correctness.exploratory.edenfs.fixture import (
    DIRECTORY,
    Entry,
    FILE,
    GeneratedFixture,
)
from eden.integration.correctness.exploratory.edenfs.sites import SITES


PATH = "path"


MODE = "mode"


TRACK = "track"


PARENT = "parent"


SOURCE = "source"


VISIT = "visit"


DRYRUN = "dryrun"


BLOCKED = "blocked"


PLANNED = "planned"


UPDATESTATE = "updatestate"


INTERRUPTED = "interrupted"


CONFLICT = "conflict"


DESCRIPTOR = "fd"


DESCRIPTOR_BINDING = "fd-binding"


DESCRIPTOR_OBJECT = "fd-object"


EDEN_PARENT = ROOT


SAPLING_PARENT = "sapling"


CLEAN = Value("clean")


DIR = Value(DIRECTORY)


YES = Value("yes")


BOUND = Value("bound")


# No value has this class: an atom that no state satisfies.
NEVER = of_class({"impossible"})


INPUT_LEGALITY_RULE = "core.input-legality"


# The existence rule that owns a getattr's ENOENT (`state_observation`); no
# contract input supports it, so it is not in CONTRACT_RULES.
GETATTR_ABSENCE_RULE = "existence.missing-after-create"


# Starting dirt over more generated paths than this is not enumerated for
# the conflict count's closed-domain test.
MAX_DIRT_SCAN = 256


DAEMON_EXIT = "daemon-exit"


KILLED = -9


# Kinds whose effects one FUSE syscall applies: they mutate state, bind a
# path, and run no process.
FUSE_MUTATIONS = frozenset(
    kind
    for kind, definition in ACTION_DEFS.items()
    if definition.roles & MUTATING_ROLES
    and definition.binding == Binding.PATH
    and not definition.spawns_process
)


_FUSE_INDETERMINATE_ERRNOS = frozenset(
    {errno.ETIMEDOUT, errno.ECONNABORTED, errno.ENOTCONN}
)


# Kinds that observe an aggregate (a listing, a status, a window).
_AGGREGATE_KINDS = frozenset(
    kind
    for kind, definition in ACTION_DEFS.items()
    if Role.OBSERVE in definition.roles and definition.binding == Binding.AGGREGATE
)


# The non-aggregate kinds that read the EdenFS parent: a checkout (its
# source), and `sl rm` / `sl mv`, whose tracking is judged at the parent
# (`_tracked_rows`).
_PARENT_READERS = frozenset({"checkout", "sl_rm", "sl_mv"})


_PATH_FIELDS = ("path", "source", "destination")


class Resolution(StrEnum):
    """What a recorded result says about an action's effects.

    `acked`: they happened inside the action's interval. `indeterminate`:
    they may have happened, before the run's resolution boundary. `none`:
    they definitely did not.
    """

    ACKED = "acked"
    INDETERMINATE = "indeterminate"
    NONE = "none"


def file_value(data: bytes) -> Value:
    """Return the `path` value of a file holding `data`."""
    return Value(FILE, (content_token(data),))


def mode_value(executable: bool) -> Value:
    """Return the `mode` value of a present path."""
    return Value(MODE, (executable,))


def rev_value(revision: int) -> Value:
    """Return the `parent` value of one fixture revision."""
    return Value("rev", (revision,))


def path_cell(path: str, side: str = PRIMARY) -> Cell:
    """Return the `path` cell of a checkout-relative path."""
    return Cell(side, PATH, normalize_path(path))


def mode_cell(path: str, side: str = PRIMARY) -> Cell:
    """Return the `mode` cell of a checkout-relative path."""
    return Cell(side, MODE, normalize_path(path))


def track_cell(path: str, side: str = PRIMARY) -> Cell:
    """Return the `track` cell of a checkout-relative path."""
    return Cell(side, TRACK, normalize_path(path))


def parent_cell(key: str = EDEN_PARENT, side: str = PRIMARY) -> Cell:
    """Return the EdenFS (`""`) or Sapling (`sapling`) parent cell."""
    return Cell(side, PARENT, key)


def source_cell(checkout: str, revision: int, side: str = PRIMARY) -> Cell:
    """Return the cell a checkout's parent group marks when it starts from `revision`.

    `checkout` is the checkout's key from `checkout_keys`, not its action ID.
    """
    return Cell(side, SOURCE, f"{checkout}/r{revision}")


def visit_cell(checkout: str, path: str, side: str = PRIMARY) -> Cell:
    """Return the cell a checkout's per-path group marks when its walk reaches `path`.

    `checkout` is the checkout's key from `checkout_keys`, not its action ID.
    """
    return Cell(side, VISIT, f"{checkout}/{normalize_path(path)}")


def blocked_cell(checkout: str, revision: int, path: str, side: str = PRIMARY) -> Cell:
    """Return the cell a DRY_RUN visit marks when `path` blocks from source `revision`."""
    return Cell(side, BLOCKED, f"{checkout}/r{revision}/{normalize_path(path)}")


def _rank_keys(
    facts: HistoryFacts, kinds: tuple[str, ...], prefix: str
) -> dict[str, str]:
    """Return each invoked action's key: `<prefix><rank>` by invocation, then completion.

    Equal times fall back to the action ID, the only ID material, as the
    last tie-break.
    """
    invoked = sorted(
        (
            (
                action.invoked,
                math.inf if action.completed is None else action.completed,
            ),
            action.id,
        )
        for kind in kinds
        for action in facts.actions_of(kind)
        if action.invoked is not None
    )
    return {
        action_id: f"{prefix}{rank}" for rank, (_times, action_id) in enumerate(invoked)
    }


def checkout_keys(facts: HistoryFacts) -> dict[str, str]:
    """Return each invoked checkout's cell key: `k<rank>` (`_rank_keys`)."""
    return _rank_keys(facts, ("checkout",), "k")


def _entry_value(entry: Entry | None) -> Value:
    if entry is None:
        return ABSENT
    if entry.kind == DIRECTORY:
        return DIR
    return file_value(entry.data)


def _entry_mode(entry: Entry | None) -> Value:
    return ABSENT if entry is None else mode_value(entry.executable)


class EdenPinned:
    """The pinned facts of one run: initial and committed values, by indexed query.

    Every answer costs one fixture point query; nothing enumerates the
    repository. Both sides start from the same fixture and dirt.
    """

    def __init__(self, fixture: GeneratedFixture) -> None:
        self.fixture = fixture

    @classmethod
    def for_spec(cls, spec: RunSpec) -> EdenPinned:
        """Return the pinned facts a RunSpec's environment describes."""
        return cls(GeneratedFixture.for_spec(spec))

    @property
    def initial_revision(self) -> int:
        """Return the revision the run starts at."""
        return self.fixture.initial_revision

    def committed(self, path: str, revision: int) -> Value:
        """Return the `path` value a fixture revision holds."""
        return _entry_value(self.fixture.committed(path, revision))

    def committed_mode(self, path: str, revision: int) -> Value:
        """Return the `mode` value a fixture revision holds."""
        return _entry_mode(self.fixture.committed(path, revision))

    def initial(self, cell: Cell) -> Value:
        """Return one cell's value before any action ran."""
        if cell.space == PATH:
            return _entry_value(self.fixture.initial(cell.key))
        if cell.space == MODE:
            return _entry_mode(self.fixture.initial(cell.key))
        if cell.space == TRACK:
            return CLEAN
        if cell.space == PARENT and cell.key in (EDEN_PARENT, SAPLING_PARENT):
            return rev_value(self.initial_revision)
        if cell.space in (
            SOURCE,
            VISIT,
            CONFLICT,
            DRYRUN,
            BLOCKED,
            PLANNED,
            UPDATESTATE,
            INTERRUPTED,
            DESCRIPTOR,
            DESCRIPTOR_BINDING,
        ):
            return ABSENT
        if cell.space == DESCRIPTOR_OBJECT and "/" in cell.key:
            return _entry_value(self.fixture.initial(cell.key.split("/", 1)[1]))
        raise UnknownInitial(f"no pinned value for {cell.document()}")


def pinned_facts(spec: RunSpec) -> EdenPinned:
    """Return the target's `pinned` hook value for one RunSpec."""
    return EdenPinned.for_spec(spec)


def _errno_of(facts: ActionFacts) -> int | None:
    return None if facts.result is None else facts.result.errno


def _field(facts: ActionFacts, name: str) -> Any:
    return None if facts.result is None else facts.result.get(name)


def _stderr_first_line(facts: ActionFacts) -> str:
    stderr = _field(facts, "stderr")
    return (
        stderr.strip().splitlines()[0]
        if isinstance(stderr, str) and stderr.strip()
        else ""
    )


def _post_effect_fault(facts: ActionFacts, history: HistoryFacts | None) -> bool:
    """Return whether an injected error at a `post` site may have failed this action.

    The site's declared position says its target's effect already took
    place when the error fired (`Site.effect`); only a failure whose errno
    the kind declares a fault errno can be that error.
    """
    if history is None or facts.result is None:
        return False
    name = _errno_name(facts) if _errno_of(facts) is not None else None
    definition = ACTION_DEFS.get(facts.kind)
    if (
        name is None
        or definition is None
        or definition.result.errnos.get(name) != ErrnoUse.FAULT
    ):
        return False
    return any(
        install.outcome == Outcome.OK
        and install.payload.get("behavior") == "error"
        and install.payload.get("target_action") == facts.id
        and SITES[install.payload["site"]].effect == POST_EFFECT
        for install in history.actions_of("fault_install")
        if install.payload.get("site") in SITES
    )


def resolution_boundary(facts: HistoryFacts) -> float:
    """Return when every indeterminate effect has resolved.

    That is the supervisor's `run-closed` record (every process of the run
    is dead), else an unowned daemon exit, else never.
    """
    closed = [event.time for event in facts.run_events if event.type == RUN_CLOSED]
    if closed:
        return min(closed)
    exits = [
        event.time
        for event in facts.run_events
        if event.type == INFO and event.data.get("reason") == DAEMON_EXIT
    ]
    return min(exits, default=math.inf)


def _group_id(action_id: str, part: str = "") -> str:
    return f"g:{action_id}" if not part else f"g:{action_id}:{part}"


def _path_part(path: str) -> str:
    return "p" + hashlib.sha256(path.encode("utf-8")).hexdigest()[:16]


@dataclass(frozen=True)
class _Op:
    """One write or chmod that may have produced its value inside a window.

    `cell` is the one cell it sets: a write's `path`, a chmod's `mode`.
    `key` is its `o<rank>` among writes and chmods (`_rank_keys`): the
    supersede marks a checkout keeps for it are keyed by `key`, never by
    `action`, so cell keys carry no action ID.
    A checkout may supersede it (`_supersede_groups`).
    """

    action: str
    key: str
    cell: Cell
    value: Value
    window: Window


@dataclass(frozen=True)
class _Context:
    """Everything the contracts read, computed once per run."""

    spec: RunSpec
    facts: HistoryFacts
    pinned: EdenPinned
    boundary: float
    rules: frozenset[str]
    relevant: tuple[str, ...]
    relevant_set: frozenset[str]
    candidates: tuple[int, ...]
    ops: Mapping[str, tuple[_Op, ...]]
    checkout_keys: Mapping[str, str]
    plans: Mapping[str, _Plan]
    weights: Mapping[str, int]
    named: frozenset[str]
    # Whether some checkout may leave EdenFS interrupted (`_interruption_groups`).
    interruptible: bool

    def obligation(self, rule: str, scope: str) -> ObligationRef | None:
        """Return the obligation a contract input supports, if its rule is registered."""
        return ObligationRef(rule, scope) if rule in self.rules else None


def _produce(
    cell: Cell, value: Value, provenance: str, guard: Guard | None = None
) -> Effect:
    return Effect(cell, EffectKind.PRODUCE, value, None, provenance, guard)


def _single(atom: Atom) -> Expect:
    return Expect(((atom,),))


def _group(
    action: Action,
    status: Status,
    window: Window,
    effects: tuple[Effect, ...],
    *,
    part: str = "",
    requires: tuple[Requirement, ...] = (),
) -> EffectGroup:
    return EffectGroup(
        id=_group_id(action.id, part),
        action=action.id,
        status=status,
        window=window,
        effects=effects,
        requires=requires,
    )


@dataclass(frozen=True)
class _Resume:
    """A checkout to the same target that may have left the mount interrupted.

    While the `interrupted` cell names it, a checkout to that target
    resumes it from its source (`EdenMount::beginCheckout`
    accepts the same target as a resume); the engine decides whether it
    does. `source` is that checkout's settled source, or None when its
    marks name it.
    """

    key: str
    source: int | None


@dataclass(frozen=True)
class _Plan:
    """What the run's other actions fix about one checkout's source and order.

    `settled` is the one revision the EdenFS parent holds when the
    checkout starts, when every parent move that may precede it is
    acked and ordered and nothing that may read the parent or another
    parent move overlaps its window, for an indeterminate checkout up to
    the resolution boundary (`_plans`); its groups then read no
    mark and need no order requirement. `serialized` says another
    checkout's parent move may land inside its window, so its paths also
    require the parent at its destination.
    """

    settled: int | None
    serialized: bool
    resumes: tuple[_Resume, ...]


def _interval(facts: ActionFacts, boundary: float) -> Window | None:
    if facts.invoked is None:
        return None
    return Window(
        facts.invoked, facts.completed if facts.completed is not None else boundary
    )


# One local state of a path: an atom each for its `path`, `mode`, and
# `track` cells, in that order.
_Local = tuple[Atom, Atom, Atom]


_ANY_LOCAL: _Local = (ANY, ANY, ANY)


@dataclass(frozen=True)
class _Entry:
    """A path's committed object and mode at one revision."""

    value: Value
    mode: Value

    def local(self, track: Atom = ANY) -> _Local:
        """Return the local state equal to this entry.

        EdenFS compares a file's bytes and executable bit with the
        committed blob and entry type (`FileInode::isSameAsFast`), so a
        mode-only change is a modification; a directory is never compared
        by mode.
        """
        if self.value in (ABSENT, DIR):
            return (eq(self.value), ANY, track)
        return (eq(self.value), eq(self.mode), track)


def _entry(pinned: EdenPinned, path: str, revision: int) -> _Entry:
    return _Entry(
        pinned.committed(path, revision), pinned.committed_mode(path, revision)
    )


def _changes(source: _Entry, target: _Entry) -> bool:
    """Return whether the destination changes the path (EdenFS compares SCM entries)."""
    if source.value != target.value:
        return True
    return source.value not in (ABSENT, DIR) and source.mode != target.mode


def _replaces(force: bool, source: _Entry, target: _Entry) -> bool:
    """Return whether a checkout that applies the path swaps in a new inode.

    FORCE replaces every loaded path the source or destination holds,
    changed or kept; NORMAL only a path the destination changes.
    """
    if force:
        return (source.value, target.value) != (ABSENT, ABSENT)
    return _changes(source, target)


_ABSENT_LOCAL: _Local = (eq(ABSENT), ANY, ANY)


def _applies(source: _Entry, target: _Entry) -> list[_Local]:
    """Return the local states a NORMAL checkout replaces with the destination.

    Empty when the destination does not change the path (no effect in any
    state). EdenFS applies a clean path or one already equal to the
    destination; a locally present path the source lacks is left alone
    even when it equals the destination (UNTRACKED_ADDED), and so is any
    other dirty or absent path (in `sl goto`'s NORMAL phase too).
    """
    if not _changes(source, target):
        return []
    if source.value == ABSENT:
        return [_ABSENT_LOCAL]
    rows = [source.local()]
    if target.value != ABSENT:
        rows.append(target.local())
    return rows


@dataclass(frozen=True)
class _Values:
    """A per-cell constraint: the values inside `values`, or every value outside."""

    inside: bool
    values: frozenset[Value]

    @classmethod
    def of(cls, atom: Atom) -> _Values:
        if atom == ANY:
            return cls(False, frozenset())
        if atom.op in (AtomOp.EQ, AtomOp.IN):
            return cls(True, atom.values)
        if atom.op == AtomOp.NOT_IN:
            return cls(False, atom.values)
        raise ValueError(f"no value-set form for {atom.op}")

    def negated(self) -> _Values:
        return _Values(not self.inside, self.values)

    def meet(self, other: _Values) -> _Values:
        if self.inside and other.inside:
            return _Values(True, self.values & other.values)
        if self.inside or other.inside:
            inside, outside = (self, other) if self.inside else (other, self)
            return _Values(True, inside.values - outside.values)
        return _Values(False, self.values | other.values)

    @property
    def empty(self) -> bool:
        return self.inside and not self.values

    def atom(self) -> Atom:
        if not self.inside:
            return none_of(self.values) if self.values else ANY
        return (
            eq(next(iter(self.values)))
            if len(self.values) == 1
            else one_of(self.values)
        )


def _complement(rows: list[_Local]) -> list[_Local]:
    """Return local states, as rows, matching none of `rows`."""
    result: list[tuple[_Values, ...]] = [tuple(_Values.of(ANY) for _ in _ANY_LOCAL)]
    for row in rows:
        grown: list[tuple[_Values, ...]] = []
        for partial in result:
            for index, atom in enumerate(row):
                if atom == ANY:
                    continue
                narrowed = partial[index].meet(_Values.of(atom).negated())
                if not narrowed.empty:
                    grown.append((*partial[:index], narrowed, *partial[index + 1 :]))
        result = list(dict.fromkeys(grown))
    return [(row[0].atom(), row[1].atom(), row[2].atom()) for row in result]


def _local_cells(path: str) -> tuple[Cell, Cell, Cell]:
    return (path_cell(path), mode_cell(path), track_cell(path))


def _marks(ctx: _Context, revision: int) -> tuple[Atom, ...]:
    return tuple(eq(YES) if other == revision else ANY for other in ctx.candidates)


def _unmarked(ctx: _Context) -> tuple[Atom, ...]:
    return tuple(eq(ABSENT) for _ in ctx.candidates)


def _descendants(ctx: _Context, path: str) -> tuple[str, ...]:
    """Return the relevant paths strictly under `path`, by bisection."""
    prefix = path + "/"
    start = bisect.bisect_left(ctx.relevant, prefix)
    found = []
    for candidate in ctx.relevant[start:]:
        if not candidate.startswith(prefix):
            break
        found.append(candidate)
    return tuple(found)


def _ancestors(path: str) -> Iterator[str]:
    parent = _parent_path(path)
    while parent != ROOT:
        yield parent
        parent = _parent_path(parent)


def _retypes(source: _Entry, target: _Entry) -> bool:
    return (source.value == DIR) != (target.value == DIR)


@dataclass(frozen=True)
class _Structure:
    """The cells a per-path checkout condition reads besides the path's own.

    A checkout replaces a directory with a file only when nothing it
    keeps is left under it: each relevant descendant is clean or already
    absent (a dirty or untracked child keeps its directory under NORMAL,
    DIRECTORY_NOT_EMPTY; FORCE removing local-only children is not encoded,
    since validation refuses a path below a type-changing register).
    `visited` names a cell that must already be marked.
    """

    descendants: tuple[str, ...] = ()
    visited: Cell | None = None

    def cells(self) -> tuple[Cell, ...]:
        nested = tuple(
            cell
            for path in self.descendants
            for cell in (path_cell(path), mode_cell(path))
        )
        return (*nested, *(() if self.visited is None else (self.visited,)))

    def tails(
        self, ctx: _Context, revision: int, source: _Entry, target: _Entry
    ) -> list[tuple[Atom, ...]]:
        """Return the alternative atom tuples over `cells()` for one source revision."""
        tails: list[tuple[Atom, ...]] = [()]
        emptied = source.value == DIR and target.value != DIR
        for path in self.descendants:
            entry = _entry(ctx.pinned, path, revision)
            options = [(ANY, ANY)]
            if emptied:
                options = [(eq(ABSENT), ANY)]
                if entry.value != ABSENT:
                    options.append(entry.local()[:2])
            tails = [(*tail, *option) for tail in tails for option in options]
        visited = () if self.visited is None else (eq(YES),)
        return [(*tail, *visited) for tail in tails]


# A per-path condition: the rows over the path's local cells plus any
# extra cells that satisfy it when the checkout starts from `revision`, or
# None when every state does.
_Select = Callable[[int, _Entry, _Entry], Sequence[tuple[Atom, ...]] | None]


def _local_select(
    select: Callable[[_Entry, _Entry], Sequence[tuple[Atom, ...]] | None],
) -> _Select:
    return lambda _revision, source, target: select(source, target)


def _sapling_moves(facts: ActionFacts) -> bool:
    """Return whether an `sl goto` may have set Sapling's parent.

    It does on success, and may when its outcome is unknown or it was
    killed; an `sl` that exited with any failure never did.
    """
    if facts.outcome in (None, Outcome.OK, Outcome.INFO) or facts.result is None:
        return True
    return _field(facts, "returncode") == KILLED


def _moved(ctx: _Context, path: str) -> bool:
    """Return whether a rename or `sl mv` may move `path` or a directory above it."""
    held = {path, *_ancestors(path)}
    return any(
        action.kind in ("rename", "sl_mv")
        and normalize_path(action.payload["source"]) in held
        for action in ctx.spec.actions
    )


def _dirt_outside(ctx: _Context) -> bool:
    """Return whether starting dirt may lie on a path the contracts does not track."""
    fixture = ctx.pinned.fixture
    limit = min(fixture.scatter_dirty, fixture.layout.file_count)
    if limit == 0 or ctx.weights:
        return False
    if limit > MAX_DIRT_SCAN:
        return True
    revision = fixture.initial_revision
    return any(
        normalize_path(fixture.layout.file_at(index, revision).path)
        not in ctx.relevant_set
        for index in range(limit)
    )


def _subsets(weights: list[int], total: int) -> Iterator[frozenset[int]]:
    """Yield the index sets of `weights` that sum to `total`."""

    rest = [sum(weights[index:]) for index in range(len(weights) + 1)]

    def grow(
        start: int, reached: int, picked: tuple[int, ...]
    ) -> Iterator[frozenset[int]]:
        if reached == total:
            yield frozenset(picked)
            return
        for index in range(start, len(weights)):
            if reached + rest[index] < total:
                return
            if reached + weights[index] <= total:
                yield from grow(index + 1, reached + weights[index], (*picked, index))

    yield from grow(0, 0, ())


def _parent_path(path: str) -> str:
    parent = str(PurePosixPath(normalize_path(path) or ".").parent)
    return normalize_path(parent)


_OUT_OF_DATE_PARENT = "OUT_OF_DATE_PARENT"


def _errno_name(facts: ActionFacts) -> str:
    code = _errno_of(facts)
    assert code is not None
    return errno.errorcode.get(code, str(code))


def _eden_pinned(spec: RunSpec, pinned: PinnedFacts | None) -> EdenPinned:
    if pinned is None:
        return EdenPinned.for_spec(spec)
    if not isinstance(pinned, EdenPinned):
        raise TypeError(
            f"EdenFS the contracts needs EdenPinned, got {type(pinned).__name__}"
        )
    return pinned


def within_footprint(cell: Cell, footprint: Footprint) -> bool:
    """Return whether an effect cell lies inside an action's declared written footprint.

    Path-keyed cells must be written exactly or under a written subtree;
    parent and source cells belong to whole-checkout writers.
    """
    if cell.space in (PATH, MODE, TRACK):
        key = cell.key
        if key in footprint.writes:
            return True
        return any(
            tree == ROOT or key == tree or key.startswith(tree + "/")
            for tree in footprint.subtrees_written
        )
    return ROOT in footprint.subtrees_written
