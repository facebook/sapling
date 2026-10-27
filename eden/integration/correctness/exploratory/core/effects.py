# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Typed effects, observations, constraints, and footprints.

A target's effect contracts hook turns validated history and pinned fixture facts
into concrete effect groups; collectors turn observed results into
observations and named constraints. Every type here validates itself on
construction and has a canonical JSON document for proofs and digests.
This module defines the vocabulary only: it decides nothing.
"""

from __future__ import annotations

import functools
import json
import math
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any, Protocol, TYPE_CHECKING, TypeAlias

from .jsonio import (
    check_name,
    digest,
    MAX_ID_BYTES,
    MAX_SCALAR_BYTES,
    require as _require,
)

if TYPE_CHECKING:
    from .facts import HistoryFacts
    from .spec_types import RunSpec

PRIMARY = "primary"
REFERENCE = "reference"
SIDES = frozenset({PRIMARY, REFERENCE})
ROOT = ""

_MAX_VALUE_DEPTH = 8


def _check_id(value: Any, what: str) -> None:
    _require(
        type(value) is str
        and bool(value)
        and len(value.encode("utf-8", errors="surrogatepass")) <= MAX_ID_BYTES,
        f"bad {what} {value!r:.200}",
    )


def _check_name(value: Any, what: str) -> None:
    check_name(value, what, MAX_ID_BYTES)


def _check_text(value: Any, what: str) -> None:
    _require(
        type(value) is str
        and len(value.encode("utf-8", errors="surrogatepass")) <= MAX_SCALAR_BYTES,
        f"bad {what} {value!r:.200}",
    )


def normalize_path(path: str) -> str:
    """Return the canonical checkout-relative form of a modeled path.

    Separators collapse, ``.`` components drop, and ``""`` or ``.`` is
    ``ROOT``. An absolute path or a ``..`` component raises ValueError.
    """
    _require(type(path) is str, f"footprint path must be a string, got {path!r:.200}")
    _require(not path.startswith("/"), f"footprint path {path!r:.200} is absolute")
    parts = [part for part in path.split("/") if part not in ("", ".")]
    _require(".." not in parts, f"footprint path {path!r:.200} escapes the checkout")
    return "/".join(parts)


def is_canonical_key(key: Any) -> bool:
    """Return whether `key` is a canonical cell key.

    A canonical key is ``ROOT`` or "/"-separated segments none of which is
    empty, ``.``, or ``..``: no leading, trailing, or doubled separator.
    `normalize_path` returns the canonical key of a checkout path.
    """
    if type(key) is not str:
        return False
    if key == ROOT:
        return True
    return all(part not in ("", ".", "..") for part in key.split("/"))


def _check_key(key: Any, what: str) -> None:
    _check_text(key, what)
    _require(is_canonical_key(key), f"{what} {key!r:.200} is not canonical")


def _paths(raw: Iterable[str]) -> frozenset[str]:
    _require(
        not isinstance(raw, str), "footprint paths must be a collection, not a string"
    )
    return frozenset(normalize_path(path) for path in raw)


@dataclass(frozen=True)
class Footprint:
    """The modeled paths and channels one action may read or write.

    `reads` and `writes` name exact paths; `subtrees_read` and
    `subtrees_written` cover a path and everything beneath it, and
    ``ROOT`` names the whole checkout. Every path is normalized on
    construction. `channels` name the observation channels the action
    uses. Footprints find possible interactions conservatively; they never
    couple solver variables.
    """

    reads: frozenset[str] = frozenset()
    writes: frozenset[str] = frozenset()
    subtrees_read: frozenset[str] = frozenset()
    subtrees_written: frozenset[str] = frozenset()
    channels: frozenset[str] = frozenset()

    def __post_init__(self) -> None:
        for name in ("reads", "writes", "subtrees_read", "subtrees_written"):
            object.__setattr__(self, name, _paths(getattr(self, name)))
        _require(
            not isinstance(self.channels, str),
            "footprint channels must be a collection, not a string",
        )
        channels = frozenset(self.channels)
        for channel in channels:
            _check_name(channel, "footprint channel")
        object.__setattr__(self, "channels", channels)

    def paths(self) -> frozenset[str]:
        """Return every path the footprint names, exact or as a subtree root."""
        return self.reads | self.writes | self.subtrees_read | self.subtrees_written

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this footprint."""
        return {
            "reads": sorted(self.reads),
            "writes": sorted(self.writes),
            "subtrees_read": sorted(self.subtrees_read),
            "subtrees_written": sorted(self.subtrees_written),
            "channels": sorted(self.channels),
        }


EMPTY_FOOTPRINT = Footprint()


def _under(path: str, root: str) -> bool:
    return root == ROOT or path == root or path.startswith(root + "/")


def _writes_into(writer: Footprint, other: Footprint) -> bool:
    """Return whether `writer` may write a path `other` names, exactly or by subtree."""
    for path in writer.writes:
        if path in other.reads or path in other.writes:
            return True
        if any(_under(path, root) for root in other.subtrees_read):
            return True
        if any(_under(path, root) for root in other.subtrees_written):
            return True
    return any(
        _under(path, root) or _under(root, path)
        for root in writer.subtrees_written
        for path in other.paths()
    )


def footprints_interact(first: Footprint, second: Footprint) -> bool:
    """Return whether one footprint may write a path the other reads or writes.

    Two pure readers never interact. A subtree covers its root and every
    path beneath it, and ``ROOT`` covers the whole checkout. Channels are
    not compared: the same path through two channels is the same state.
    """
    return _writes_into(first, second) or _writes_into(second, first)


def no_footprint(_spec: Any, _action: Any) -> Footprint:
    """Return the empty footprint of an action that touches no modeled state."""
    return EMPTY_FOOTPRINT


@dataclass(frozen=True)
class ObligationRef:
    """One obligation: one rule applied to one scope, e.g. (`bytes.torn-read`, `r3`).

    A run has at most one obligation per `(rule_id, scope)`.
    """

    rule_id: str
    scope: str

    def __post_init__(self) -> None:
        _check_id(self.rule_id, "obligation rule id")
        _check_id(self.scope, "obligation scope")

    def document(self) -> list[str]:
        """Return the canonical JSON document of this reference."""
        return [self.rule_id, self.scope]


@dataclass(frozen=True)
class Cell:
    """One modeled state cell: a canonical key in a named space on one side.

    The key is canonical (`is_canonical_key`) in every space; targets
    normalize keys, for example with `normalize_path`, before constructing.
    """

    side: str
    space: str
    key: str

    def __post_init__(self) -> None:
        _require(self.side in SIDES, f"bad cell side {self.side!r:.200}")
        _check_name(self.space, "cell space")
        _check_key(self.key, "cell key")

    def document(self) -> list[str]:
        """Return the canonical JSON document of this cell."""
        return [self.side, self.space, self.key]


def _check_datum(value: Any, depth: int = 0) -> None:
    _require(depth <= _MAX_VALUE_DEPTH, "value data nests too deeply")
    if type(value) is tuple:
        for item in value:
            _check_datum(item, depth + 1)
        return
    _require(
        value is None or type(value) in (str, int, bool),
        f"value data must be text, int, bool, None, or tuples, got {type(value).__name__}",
    )
    if type(value) is str:
        _check_text(value, "value datum")


def _datum_document(value: Any) -> Any:
    if type(value) is tuple:
        return [_datum_document(item) for item in value]
    return value


@dataclass(frozen=True)
class Value:
    """One concrete state value: a class name and class-specific data."""

    cls: str
    data: tuple[Any, ...] = ()

    def __post_init__(self) -> None:
        _check_name(self.cls, "value class")
        _require(type(self.data) is tuple, "value data must be a tuple")
        _check_datum(self.data)

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this value."""
        return {"cls": self.cls, "data": _datum_document(self.data)}


ABSENT = Value("absent")


@dataclass(frozen=True)
class Window:
    """A closed interval of monotonic event time; `hi` may be infinite."""

    lo: float
    hi: float

    def __post_init__(self) -> None:
        _require(
            type(self.lo) in (int, float) and math.isfinite(self.lo),
            f"window lo must be finite, got {self.lo!r}",
        )
        _require(
            type(self.hi) in (int, float) and not math.isnan(self.hi),
            f"window hi must be a number, got {self.hi!r}",
        )
        _require(self.lo <= self.hi, f"window [{self.lo}, {self.hi}] is empty")

    def document(self) -> list[float | None]:
        """Return `[lo, hi]`, with an infinite `hi` as null."""
        return [self.lo, None if math.isinf(self.hi) else self.hi]

    def meets(self, other: Window) -> bool:
        """Return whether the two closed intervals share an instant."""
        return self.lo <= other.hi and other.lo <= self.hi

    def contains(self, other: Window) -> bool:
        """Return whether `other` lies inside this interval."""
        return self.lo <= other.lo and other.hi <= self.hi

    def before(self, other: Window) -> bool:
        """Return whether this interval ends strictly before `other` begins."""
        return self.hi < other.lo


class Status(StrEnum):
    """Whether an effect group's action is known to have taken effect."""

    ACKED = "acked"
    INDETERMINATE = "indeterminate"


class EffectKind(StrEnum):
    """`produce` sets a value (ABSENT removes); `transfer` moves a value."""

    PRODUCE = "produce"
    TRANSFER = "transfer"


class AtomOp(StrEnum):
    """How one cell's visible value is constrained."""

    ANY = "any"
    EQ = "eq"
    IN = "in"
    NOT_IN = "not-in"
    CLASS = "class"


def _sorted_values(values: Iterable[Value]) -> list[dict[str, Any]]:
    documents = [value.document() for value in values]
    return sorted(documents, key=lambda doc: json.dumps(doc, sort_keys=True))


@dataclass(frozen=True)
class Atom:
    """One per-cell constraint: any value, a value, a set, or value classes."""

    op: AtomOp
    values: frozenset[Value] = frozenset()
    classes: frozenset[str] = frozenset()

    def __post_init__(self) -> None:
        _require(self.op in frozenset(AtomOp), f"bad atom op {self.op!r}")
        _require(
            type(self.values) is frozenset and type(self.classes) is frozenset,
            "atom operands must be frozensets",
        )
        for value in self.values:
            _require(isinstance(value, Value), "atom values must be Values")
        for cls in self.classes:
            _check_name(cls, "atom class")
        expected = {
            AtomOp.ANY: (False, False),
            AtomOp.EQ: (True, False),
            AtomOp.IN: (True, False),
            AtomOp.NOT_IN: (True, False),
            AtomOp.CLASS: (False, True),
        }[self.op]
        _require(
            (bool(self.values), bool(self.classes)) == expected,
            f"{self.op} atom has the wrong operands",
        )
        _require(
            self.op != AtomOp.EQ or len(self.values) == 1,
            "an eq atom names exactly one value",
        )

    def matches(self, value: Value) -> bool:
        """Return whether a visible value satisfies this atom."""
        if self.op == AtomOp.ANY:
            return True
        if self.op == AtomOp.CLASS:
            return value.cls in self.classes
        if self.op == AtomOp.NOT_IN:
            return value not in self.values
        return value in self.values

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this atom."""
        return {
            "op": str(self.op),
            "values": _sorted_values(self.values),
            "classes": sorted(self.classes),
        }


ANY = Atom(AtomOp.ANY)


def eq(value: Value) -> Atom:
    """Return the atom requiring exactly `value`."""
    return Atom(AtomOp.EQ, frozenset({value}))


def one_of(values: Iterable[Value]) -> Atom:
    """Return the atom requiring one of `values`."""
    return Atom(AtomOp.IN, frozenset(values))


def none_of(values: Iterable[Value]) -> Atom:
    """Return the atom forbidding every one of `values`."""
    return Atom(AtomOp.NOT_IN, frozenset(values))


def of_class(classes: Iterable[str]) -> Atom:
    """Return the atom requiring a value of one of `classes`."""
    return Atom(AtomOp.CLASS, classes=frozenset(classes))


@dataclass(frozen=True)
class Expect:
    """A disjunction of rows; each row conjoins one atom per cell."""

    rows: tuple[tuple[Atom, ...], ...]

    def __post_init__(self) -> None:
        _require(
            type(self.rows) is tuple and bool(self.rows),
            "an expectation needs at least one row",
        )
        widths = set()
        for row in self.rows:
            _require(
                type(row) is tuple and all(isinstance(atom, Atom) for atom in row),
                "an expectation row is a tuple of atoms",
            )
            widths.add(len(row))
        _require(len(widths) == 1, "every expectation row has one width")

    @property
    def width(self) -> int:
        """Return how many cells each row constrains."""
        return len(self.rows[0])

    def holds(self, values: tuple[Value, ...]) -> bool:
        """Return whether some row accepts the visible values, in cell order."""
        _require(len(values) == self.width, "expectation width mismatch")
        return any(
            all(atom.matches(value) for atom, value in zip(row, values))
            for row in self.rows
        )

    def document(self) -> list[list[dict[str, Any]]]:
        """Return the canonical JSON document of this expectation."""
        return [[atom.document() for atom in row] for row in self.rows]


def _check_cells(cells: Any, expect: Any, what: str) -> str:
    """Validate a non-empty, repeat-free, one-sided cell tuple and its expectation.

    Returns the cells' side.
    """
    _require(
        type(cells) is tuple
        and bool(cells)
        and all(isinstance(cell, Cell) for cell in cells),
        f"{what} needs a non-empty tuple of cells",
    )
    _require(len(set(cells)) == len(cells), f"{what} repeats a cell")
    sides = sorted({cell.side for cell in cells})
    _require(len(sides) == 1, f"{what} spans sides {sides}")
    _require(isinstance(expect, Expect), f"{what}: expect is not an Expect")
    _require(
        expect.width == len(cells),
        f"{what}: rows constrain {expect.width} cells, not {len(cells)}",
    )
    return sides[0]


@dataclass(frozen=True)
class Guard:
    """A condition on cells' values just before an effect's group takes effect.

    When it does not hold, the guarded effect is a no-op; the rest of the
    group still applies.
    """

    cells: tuple[Cell, ...]
    expect: Expect

    def __post_init__(self) -> None:
        _check_cells(self.cells, self.expect, "guard")

    @property
    def side(self) -> str:
        """Return the side every guarded cell is on."""
        return self.cells[0].side

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this guard."""
        return {
            "cells": [cell.document() for cell in self.cells],
            "expect": self.expect.document(),
        }


@dataclass(frozen=True)
class Requirement:
    """A condition that must hold on pre-group values whenever its group happens.

    It is solver input supporting `obligation`.
    """

    cells: tuple[Cell, ...]
    expect: Expect
    obligation: ObligationRef

    def __post_init__(self) -> None:
        _check_cells(self.cells, self.expect, "requirement")
        _require(
            isinstance(self.obligation, ObligationRef),
            "requirement obligation is not an ObligationRef",
        )

    @property
    def side(self) -> str:
        """Return the side every required cell is on."""
        return self.cells[0].side

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this requirement."""
        return {
            "cells": [cell.document() for cell in self.cells],
            "expect": self.expect.document(),
            "obligation": self.obligation.document(),
        }


@dataclass(frozen=True)
class Effect:
    """One state change inside an effect group.

    A produce sets `cell` to `value` (removal produces ABSENT). A transfer
    moves `source`'s current value to `cell` and sets `source` ABSENT;
    both are on one side. A `guard` on the effect's side makes it a no-op
    when the guard does not hold on the values visible just before the
    group's point.
    """

    cell: Cell
    kind: EffectKind
    value: Value | None
    source: Cell | None
    provenance: str
    guard: Guard | None = None

    def __post_init__(self) -> None:
        _require(isinstance(self.cell, Cell), "effect cell is not a Cell")
        _require(self.kind in frozenset(EffectKind), f"bad effect kind {self.kind!r}")
        _check_text(self.provenance, "effect provenance")
        if self.guard is not None:
            _require(isinstance(self.guard, Guard), "effect guard is not a Guard")
            _require(
                self.guard.side == self.cell.side,
                "an effect guard is on the effect's side",
            )
        if self.kind == EffectKind.PRODUCE:
            _require(
                isinstance(self.value, Value) and self.source is None,
                "a produce effect has a value and no source",
            )
            return
        _require(
            self.value is None and isinstance(self.source, Cell),
            "a transfer effect has a source and no value",
        )
        assert self.source is not None
        _require(
            self.source.side == self.cell.side and self.source != self.cell,
            "a transfer moves between distinct cells on one side",
        )

    def writes(self) -> tuple[Cell, ...]:
        """Return the cells this effect writes: its cell, and a transfer's source."""
        return (self.cell,) if self.source is None else (self.cell, self.source)

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this effect."""
        return {
            "cell": self.cell.document(),
            "kind": str(self.kind),
            "value": None if self.value is None else self.value.document(),
            "source": None if self.source is None else self.source.document(),
            "provenance": self.provenance,
            "guard": None if self.guard is None else self.guard.document(),
        }


@dataclass(frozen=True)
class EffectGroup:
    """Effects of one action that take effect at one shared point.

    Groups sharing an `outcome_var` happen together or not at all; the
    empty default names the group itself. An acked group happened; an
    indeterminate one may or may not have. A definitely-failed action has
    no group at all. The effects write pairwise-distinct cells, so their
    order inside the group never matters. `requires` must hold on the
    values visible just before the group's point whenever it happens.
    """

    id: str
    action: str
    status: Status
    window: Window
    effects: tuple[Effect, ...]
    outcome_var: str = ""
    requires: tuple[Requirement, ...] = ()

    def __post_init__(self) -> None:
        _check_id(self.id, "effect group id")
        _check_id(self.action, "effect group action")
        _require(self.status in frozenset(Status), f"bad group status {self.status!r}")
        _require(isinstance(self.window, Window), "group window is not a Window")
        _require(
            type(self.effects) is tuple and bool(self.effects),
            f"group {self.id!r} needs a non-empty tuple of effects",
        )
        for effect in self.effects:
            _require(isinstance(effect, Effect), f"group {self.id!r}: bad effect")
        sides = {effect.cell.side for effect in self.effects}
        _require(len(sides) == 1, f"group {self.id!r} spans sides {sorted(sides)}")
        written = [cell for effect in self.effects for cell in effect.writes()]
        _require(
            len(set(written)) == len(written),
            f"group {self.id!r} writes one cell twice",
        )
        _require(
            type(self.requires) is tuple
            and all(isinstance(item, Requirement) for item in self.requires),
            f"group {self.id!r}: requires is a tuple of Requirements",
        )
        _require(
            all(item.side in sides for item in self.requires),
            f"group {self.id!r}: a requirement is on another side",
        )
        if not self.outcome_var:
            object.__setattr__(self, "outcome_var", self.id)
        _check_id(self.outcome_var, "outcome variable")

    @property
    def side(self) -> str:
        """Return the side every effect of the group is on."""
        return self.effects[0].cell.side

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this group."""
        return {
            "id": self.id,
            "action": self.action,
            "status": str(self.status),
            "window": self.window.document(),
            "outcome_var": self.outcome_var,
            "effects": [effect.document() for effect in self.effects],
            "requires": [item.document() for item in self.requires],
        }


@dataclass(frozen=True)
class Domain:
    """The cells of one space on one side whose keys lie under `prefix`.

    The prefix is a canonical key; ``ROOT`` covers the whole space. An
    aggregate observation names its authoritative domain; its members are
    expanded lazily from pinned facts, never enumerated whole.
    """

    side: str
    space: str
    prefix: str = ROOT

    def __post_init__(self) -> None:
        _require(self.side in SIDES, f"bad domain side {self.side!r:.200}")
        _check_name(self.space, "domain space")
        _check_key(self.prefix, "domain prefix")

    def contains(self, cell: Cell) -> bool:
        """Return whether a cell is a member of this domain."""
        if (cell.side, cell.space) != (self.side, self.space):
            return False
        return (
            self.prefix == ROOT
            or cell.key == self.prefix
            or cell.key.startswith(self.prefix + "/")
        )

    def document(self) -> list[str]:
        """Return the canonical JSON document of this domain."""
        return [self.side, self.space, self.prefix]


@dataclass(frozen=True)
class Observation:
    """What one result says about cells' visible values inside a window.

    One observation is one solver node at one point, whatever obligations
    it supports; `obligations` is stored sorted and without repeats. Its
    cells are all on one side. `domain`, when set, names the authoritative
    domain of an aggregate observation; every cell is a member of it, and
    the collector still emits one cell per relevant member or
    equivalence-class representative.
    """

    id: str
    action: str
    cells: tuple[Cell, ...]
    expect: Expect
    window: Window
    channel: str
    obligations: tuple[ObligationRef, ...]
    domain: Domain | None = None

    def __post_init__(self) -> None:
        _check_id(self.id, "observation id")
        _check_id(self.action, "observation action")
        side = _check_cells(self.cells, self.expect, f"observation {self.id!r}")
        _require(isinstance(self.window, Window), "observation window is not Window")
        _check_name(self.channel, "observation channel")
        _require(
            type(self.obligations) is tuple
            and bool(self.obligations)
            and all(isinstance(ref, ObligationRef) for ref in self.obligations),
            f"observation {self.id!r} needs a non-empty tuple of ObligationRefs",
        )
        object.__setattr__(
            self,
            "obligations",
            tuple(sorted(set(self.obligations), key=ObligationRef.document)),
        )
        if self.domain is not None:
            _require(
                isinstance(self.domain, Domain),
                f"observation {self.id!r}: domain is not a Domain",
            )
            _require(
                self.domain.side == side
                and all(self.domain.contains(cell) for cell in self.cells),
                f"observation {self.id!r}: every cell is in its domain",
            )

    @property
    def side(self) -> str:
        """Return the side every observed cell is on."""
        return self.cells[0].side

    def content(self) -> dict[str, Any]:
        """Return the canonical document of everything except the obligations."""
        return {
            "id": self.id,
            "action": self.action,
            "cells": [cell.document() for cell in self.cells],
            "expect": self.expect.document(),
            "window": self.window.document(),
            "channel": self.channel,
            "domain": None if self.domain is None else self.domain.document(),
        }

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this observation."""
        return {
            **self.content(),
            "obligations": [ref.document() for ref in self.obligations],
        }


class NodeType(StrEnum):
    """What a constraint endpoint must be: an observation, a group, or either."""

    OBSERVATION = "observation"
    GROUP = "group"
    ANY_NODE = "node"


@dataclass(frozen=True)
class Before:
    """If both nodes happen, node `a`'s point is at or before node `b`'s."""

    a: str
    b: str
    obligation: ObligationRef

    def __post_init__(self) -> None:
        _check_pair(self.a, self.b, self.obligation)

    def nodes(self) -> tuple[str, ...]:
        """Return the node IDs this constraint names."""
        return (self.a, self.b)

    def signature(self) -> tuple[tuple[str, NodeType], ...]:
        """Return each endpoint with the node type it must name."""
        return ((self.a, NodeType.ANY_NODE), (self.b, NodeType.ANY_NODE))

    def as_before(self) -> Before:
        """Return the precedence this constraint encodes."""
        return self

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this constraint."""
        return _pair_document("before", self.a, self.b, self.obligation)


@dataclass(frozen=True)
class Supersedes:
    """Effect group `a` precedes group `b`; kept apart from Before for proofs."""

    a: str
    b: str
    obligation: ObligationRef

    def __post_init__(self) -> None:
        _check_pair(self.a, self.b, self.obligation)

    def nodes(self) -> tuple[str, ...]:
        """Return the node IDs this constraint names."""
        return (self.a, self.b)

    def signature(self) -> tuple[tuple[str, NodeType], ...]:
        """Return each endpoint with the node type it must name: two groups."""
        return ((self.a, NodeType.GROUP), (self.b, NodeType.GROUP))

    def as_before(self) -> Before:
        """Return the precedence this constraint encodes."""
        return Before(self.a, self.b, self.obligation)

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this constraint."""
        return _pair_document("supersedes", self.a, self.b, self.obligation)


@dataclass(frozen=True)
class MustAppearIn:
    """An acked group a complete observation omits: the observation came first."""

    group: str
    observation: str
    obligation: ObligationRef

    def __post_init__(self) -> None:
        _check_pair(self.group, self.observation, self.obligation)

    def nodes(self) -> tuple[str, ...]:
        """Return the node IDs this constraint names."""
        return (self.group, self.observation)

    def signature(self) -> tuple[tuple[str, NodeType], ...]:
        """Return each endpoint with the node type it must name."""
        return ((self.group, NodeType.GROUP), (self.observation, NodeType.OBSERVATION))

    def as_before(self) -> Before:
        """Return the precedence this constraint encodes: the observation first."""
        return Before(self.observation, self.group, self.obligation)

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this constraint."""
        return _pair_document(
            "must-appear-in", self.group, self.observation, self.obligation
        )


@dataclass(frozen=True)
class VisibleAfterAck:
    """A documented ordering: the group precedes the observation."""

    group: str
    observation: str
    obligation: ObligationRef

    def __post_init__(self) -> None:
        _check_pair(self.group, self.observation, self.obligation)

    def nodes(self) -> tuple[str, ...]:
        """Return the node IDs this constraint names."""
        return (self.group, self.observation)

    def signature(self) -> tuple[tuple[str, NodeType], ...]:
        """Return each endpoint with the node type it must name."""
        return ((self.group, NodeType.GROUP), (self.observation, NodeType.OBSERVATION))

    def as_before(self) -> Before:
        """Return the precedence this constraint encodes: the group first."""
        return Before(self.group, self.observation, self.obligation)

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this constraint."""
        return _pair_document(
            "visible-after-ack", self.group, self.observation, self.obligation
        )


@dataclass(frozen=True)
class Agree:
    """Two observations' payloads agree after the named normalization."""

    o1: str
    o2: str
    normalize_id: str
    obligation: ObligationRef

    def __post_init__(self) -> None:
        _check_pair(self.o1, self.o2, self.obligation)
        _check_name(self.normalize_id, "normalization id")

    def nodes(self) -> tuple[str, ...]:
        """Return the node IDs this constraint names."""
        return (self.o1, self.o2)

    def signature(self) -> tuple[tuple[str, NodeType], ...]:
        """Return each endpoint with the node type it must name: two observations."""
        return ((self.o1, NodeType.OBSERVATION), (self.o2, NodeType.OBSERVATION))

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this constraint."""
        document = _pair_document("agree", self.o1, self.o2, self.obligation)
        document["normalize"] = self.normalize_id
        return document


@dataclass(frozen=True)
class Exclusive:
    """At most one of the named outcome variables happens.

    `outcome_vars` names at least two distinct variables and is stored
    sorted.
    """

    outcome_vars: tuple[str, ...]
    obligation: ObligationRef

    def __post_init__(self) -> None:
        _require(
            type(self.outcome_vars) is tuple and len(self.outcome_vars) >= 2,
            "an exclusive constraint names at least two outcome variables",
        )
        for var in self.outcome_vars:
            _check_id(var, "outcome variable")
        _require(
            len(set(self.outcome_vars)) == len(self.outcome_vars),
            "an exclusive constraint repeats an outcome variable",
        )
        object.__setattr__(self, "outcome_vars", tuple(sorted(self.outcome_vars)))
        _require(
            isinstance(self.obligation, ObligationRef),
            "constraint obligation is not an ObligationRef",
        )

    def nodes(self) -> tuple[str, ...]:
        """Return the node IDs this constraint names: none, it names outcome variables."""
        return ()

    def signature(self) -> tuple[tuple[str, NodeType], ...]:
        """Return each node endpoint with its type: none."""
        return ()

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this constraint."""
        return {
            "kind": "exclusive",
            "outcome_vars": list(self.outcome_vars),
            "obligation": self.obligation.document(),
        }


Constraint = Before | Supersedes | MustAppearIn | VisibleAfterAck | Agree | Exclusive
CONSTRAINT_TYPES = (Before, Supersedes, MustAppearIn, VisibleAfterAck, Agree, Exclusive)


def _check_pair(a: str, b: str, obligation: ObligationRef) -> None:
    _check_id(a, "constraint node")
    _check_id(b, "constraint node")
    _require(a != b, f"constraint relates {a!r} to itself")
    _require(
        isinstance(obligation, ObligationRef),
        "constraint obligation is not an ObligationRef",
    )


def _pair_document(
    kind: str, a: str, b: str, obligation: ObligationRef
) -> dict[str, Any]:
    return {"kind": kind, "nodes": [a, b], "obligation": obligation.document()}


def constraint_id(constraint: Constraint) -> str:
    """Return the ID a proof uses to name one constraint.

    It is the sha256 of the constraint's canonical document, which
    includes its kind, endpoints, normalization, and obligation, so two
    constraints share an ID exactly when they are equal.
    """
    return digest(constraint.document())


class UnknownInitial(LookupError):
    """No pinned fact gives a cell's value before any action ran."""


class PinnedFacts(Protocol):
    """Indexed, read-only facts about the fixture a run started from."""

    def initial(self, cell: Cell) -> Value:
        """Return one cell's value before any action ran; raise UnknownInitial if unknown."""
        ...  # pragma: no cover


class ContractSink(Protocol):
    """Where a target's effect contracts hook records groups and observations."""

    def group(self, group: EffectGroup) -> None:
        """Record one effect group."""
        ...  # pragma: no cover

    def observe(self, observation: Observation) -> None:
        """Record one observation supporting obligations some property declares."""
        ...  # pragma: no cover

    def constrain(self, constraint: Constraint) -> None:
        """Record one constraint over contract nodes supporting a declared obligation."""
        ...  # pragma: no cover


_UNKNOWN = object()


@dataclass(frozen=True)
class EffectSet:
    """Every contract group, observation, and constraint, the initial-value query, and domains.

    Node IDs are unique across groups and observations; derived
    constraints name contract nodes only. `initial(cell)`
    asks the pinned facts at most once per cell and caches the answer; it
    returns a Value or raises UnknownInitial, never None. The cache is the
    only mutable state and is not an input: pinned facts are a function of
    the cell.
    """

    groups: tuple[EffectGroup, ...] = ()
    pinned: PinnedFacts | None = None
    domains: tuple[Domain, ...] = ()
    observations: tuple[Observation, ...] = ()
    constraints: tuple[Constraint, ...] = ()
    _initial: dict[Cell, Any] = field(
        default_factory=dict, init=False, compare=False, repr=False
    )

    def __post_init__(self) -> None:
        _require(type(self.groups) is tuple, "effect groups must be a tuple")
        for group in self.groups:
            _require(isinstance(group, EffectGroup), "bad effect group")
        ids = [group.id for group in self.groups]
        _require(len(set(ids)) == len(ids), "repeated effect group id")
        _require(
            type(self.observations) is tuple
            and all(isinstance(item, Observation) for item in self.observations),
            "contract observations must be a tuple of Observations",
        )
        nodes = ids + [item.id for item in self.observations]
        _require(len(set(nodes)) == len(nodes), "repeated contract node id")
        _require(
            type(self.constraints) is tuple
            and all(isinstance(item, CONSTRAINT_TYPES) for item in self.constraints),
            "contract constraints must be a tuple of Constraints",
        )
        _require(
            type(self.domains) is tuple
            and all(isinstance(domain, Domain) for domain in self.domains),
            "effect domains must be a tuple of Domains",
        )

    @property
    def group_ids(self) -> frozenset[str]:
        """Return every group ID."""
        return frozenset(group.id for group in self.groups)

    @property
    def outcome_vars(self) -> frozenset[str]:
        """Return every group's outcome variable."""
        return frozenset(group.outcome_var for group in self.groups)

    @functools.cached_property
    def _support(self) -> Mapping[ObligationRef, int]:
        counts: dict[ObligationRef, int] = {}
        refs = [ref for item in self.observations for ref in item.obligations]
        refs += [item.obligation for group in self.groups for item in group.requires]
        refs += [item.obligation for item in self.constraints]
        for ref in refs:
            counts[ref] = counts.get(ref, 0) + 1
        return counts

    @functools.cached_property
    def _writer_positions(self) -> Mapping[Cell, tuple[int, ...]]:
        found: dict[Cell, list[int]] = {}
        for position, group in enumerate(self.groups):
            written = {cell for effect in group.effects for cell in effect.writes()}
            for cell in written:
                found.setdefault(cell, []).append(position)
        return {cell: tuple(positions) for cell, positions in found.items()}

    def writers(self, cells: Iterable[Cell]) -> tuple[EffectGroup, ...]:
        """Return every group writing one of `cells`, once each, in effect-set order.

        The cell index is built once per effect set.
        """
        index = self._writer_positions
        positions = {p for cell in cells for p in index.get(cell, ())}
        return tuple(self.groups[p] for p in sorted(positions))

    def supports(self, ref: ObligationRef) -> int:
        """Return how many contract observations, requirements, and constraints support `ref`."""
        return self._support.get(ref, 0)

    def agree_supports(self, ref: ObligationRef) -> int:
        """Return how many of `ref`'s contract supports are Agree constraints."""
        return sum(
            1
            for item in self.constraints
            if isinstance(item, Agree) and item.obligation == ref
        )

    def contract_obligations(self) -> frozenset[ObligationRef]:
        """Return every obligation a contract observation, requirement, or constraint supports."""
        return frozenset(self._support)

    def initial(self, cell: Cell) -> Value:
        """Return a cell's pinned initial value, querying it at most once.

        Raises UnknownInitial without pinned facts or when they do not know
        the cell, and ValueError when they return anything but a Value.
        """
        if cell not in self._initial:
            if self.pinned is None:
                self._initial[cell] = _UNKNOWN
            else:
                try:
                    value = self.pinned.initial(cell)
                except UnknownInitial:
                    value = _UNKNOWN
                _require(
                    value is _UNKNOWN or isinstance(value, Value),
                    f"pinned facts returned {type(value).__name__} for {cell}",
                )
                self._initial[cell] = value
        value = self._initial[cell]
        if value is _UNKNOWN:
            raise UnknownInitial(f"no initial value for {cell.document()}")
        return value

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of the groups, observations, constraints, and domains."""
        return {
            "groups": [group.document() for group in self.groups],
            "observations": [item.document() for item in self.observations],
            "constraints": sorted(
                (item.document() for item in self.constraints),
                key=lambda doc: json.dumps(doc, sort_keys=True),
            ),
            "domains": sorted(domain.document() for domain in self.domains),
        }


if TYPE_CHECKING:
    EffectContracts: TypeAlias = Callable[
        [RunSpec, HistoryFacts, PinnedFacts | None, ContractSink], None
    ]
else:
    EffectContracts: TypeAlias = Callable[..., None]


@dataclass(frozen=True)
class ProofCertificate:
    """The inputs that jointly admit no explanation, and the obligations they violate.

    `observations` and `constraints` are the inputs proven necessary (the
    rest of the unsatisfiable set is satisfiable without each); `obligations`
    are exactly theirs.
    """

    id: str
    obligations: tuple[ObligationRef, ...]
    observations: tuple[str, ...]
    groups: tuple[str, ...]
    constraints: tuple[str, ...]

    def __post_init__(self) -> None:
        _check_id(self.id, "proof id")
        _require(
            type(self.obligations) is tuple
            and bool(self.obligations)
            and all(isinstance(ref, ObligationRef) for ref in self.obligations),
            f"proof {self.id!r} names no obligation",
        )
        _require(
            list(self.obligations)
            == sorted(set(self.obligations), key=ObligationRef.document),
            f"proof {self.id!r}: obligations must be sorted and unique",
        )
        for name in ("observations", "groups", "constraints"):
            items = getattr(self, name)
            _require(type(items) is tuple, f"proof {self.id!r}: {name} is not a tuple")
            for item in items:
                _check_id(item, f"proof {name} entry")
            _require(
                list(items) == sorted(set(items)),
                f"proof {self.id!r}: {name} must be sorted and unique",
            )

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this certificate."""
        return {
            "id": self.id,
            "obligations": [ref.document() for ref in self.obligations],
            "observations": list(self.observations),
            "groups": list(self.groups),
            "constraints": list(self.constraints),
        }


def read_proof(raw: Mapping[str, Any]) -> ProofCertificate:
    """Parse one canonical proof document; raise ValueError on any other shape."""
    keys = {
        "id",
        "obligations",
        "observations",
        "groups",
        "constraints",
    }
    _require(
        type(raw) is dict and set(raw) == keys,
        f"proof must have exactly {sorted(keys)}",
    )
    obligations = raw["obligations"]
    _require(type(obligations) is list, "proof obligations must be a list")
    refs = []
    for item in obligations:
        _require(
            type(item) is list and len(item) == 2, "proof obligation is [rule, scope]"
        )
        refs.append(ObligationRef(item[0], item[1]))
    lists = {}
    for name in ("observations", "groups", "constraints"):
        _require(type(raw[name]) is list, f"proof {name} must be a list")
        lists[name] = tuple(raw[name])
    return ProofCertificate(
        id=raw["id"],
        obligations=tuple(refs),
        **lists,
    )
