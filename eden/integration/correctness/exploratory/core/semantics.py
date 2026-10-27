# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Executable semantics of effects, observations, and constraints.

A `Problem` is every effect group, observation, and constraint of one run
plus the initial value of every cell. An `Assignment` is one candidate
explanation: the outcome variables that happened and one total order of
every node that happened. `explains(problem, assignment)` decides whether
the assignment explains the problem. It is the definition every engine
(`solver`, `exhaustive`) must agree with; it searches nothing.

Nodes. Every effect group and every observation is a node. An observation
always happens. A group happens exactly when its outcome variable
happened; an acked group's variable always happened, so every group
sharing it happens too, and groups sharing a variable happen together or
not at all.

Points and the application order. The model places every happened node at
a real point inside its closed window `[lo, hi]` (`hi` may be infinite)
and lets nodes at equal points apply in any order the explanation
chooses. That is the same thing as one total order of the happened nodes
in which a node `x` never precedes a node `y` with `y.hi < x.lo`:

- from points: sort by point and break ties in the chosen order; if `x`
  precedes `y` then `x.lo <= point(x) <= point(y) <= y.hi`;
- from an order: `point(n_i) = max(lo of n_1..n_i)` is non-decreasing and
  at most `hi(n_i)`, because every earlier `lo` is at most `hi(n_i)`.

So real time is "if `y.hi < x.lo` then `y` precedes `x`", ties in time are
orderable either way (conservative), and an infinite `hi` never forces
anything after the node.

Constraints. `Before(a, b)`, `Supersedes(a, b)`, `MustAppearIn(g, o)` (as
`Before(o, g)`), and `VisibleAfterAck(g, o)` (as `Before(g, o)`) require,
when both endpoints happened, that `a` precedes `b` in the application
order. Equal points are allowed, but the tie is broken once for every
constraint, so `Before(a, b)` and `Before(b, a)` with both endpoints
happening is a contradiction: e.g. a force checkout's group and an
overlapping write, with one property requiring the checkout first and
another the write first (`solver_test.WorkedExampleTest`).
`Exclusive(vars)` allows at most one of its outcome variables to happen.
`Agree` compares normalized observed payloads that a problem does not
carry; it never constrains an explanation, and engines leave its
obligation to a direct check.

State. Every cell starts at its initial value. Replaying the order, a
happened group reads the values visible just before it (its pre-group
values): every requirement must hold on them, every guard is evaluated
on them, and then every effect whose guard holds (or that has none)
applies at once: a produce sets its cell to its value; a transfer sets
its cell to the source's pre-group value and its source to ABSENT. The
effects of one group write pairwise-distinct cells, so their order inside
the group never matters. An observation's expectation must hold on the
values visible at its position. A cell's visible value is therefore the
result of every happened effect that precedes the observation in the
application order.

Inputs. The obligation-bearing inputs are observations, constraints
other than `Agree`, and group requirements; `Problem.inputs()` names each
by a stable ID. Removing any input never removes an explanation, which is
what makes deletion-based proof cores sound.

Deletion order. Engines delete inputs in `deletion_order`: latest
evidence first (descending window), then by input kind and an ID-free
canonical content document (rule ID, cells, expectation, endpoint and
group content); node, action, and outcome-variable IDs only break ties
between inputs of identical content. Renaming actions therefore never
changes which inputs a proof blames, even when proofs overlap.
"""

from __future__ import annotations

import json
import math
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass, field, replace
from typing import Any

from .effects import (
    ABSENT,
    Agree,
    Before,
    Cell,
    Constraint,
    constraint_id,
    CONSTRAINT_TYPES,
    Effect,
    EffectGroup,
    EffectKind,
    Exclusive,
    Expect,
    MustAppearIn,
    NodeType,
    ObligationRef,
    Observation,
    Status,
    Supersedes,
    UnknownInitial,
    Value,
    VisibleAfterAck,
    Window,
)
from .jsonio import require as _require

InitialValues = Callable[[Cell], Value]

OBSERVATION_INPUT = "observation"
CONSTRAINT_INPUT = "constraint"
REQUIREMENT_INPUT = "requirement"
_REQUIREMENT_PREFIX = "requires:"


def requirement_id(group_id: str, index: int) -> str:
    """Return the input ID of `group_id`'s requirement at position `index`."""
    return f"{_REQUIREMENT_PREFIX}{group_id}:{index}"


def precedence(constraint: Constraint) -> Before | None:
    """Return the precedence a constraint encodes, or None for Agree and Exclusive."""
    if isinstance(constraint, (Before, Supersedes, MustAppearIn, VisibleAfterAck)):
        return constraint.as_before()
    return None


@dataclass(frozen=True)
class Input:
    """One obligation-bearing solver input, named by a stable ID.

    `kind` is `observation` (ID = observation ID), `constraint` (ID =
    `constraint_id`), or `requirement` (ID = `requirement_id`).
    """

    id: str
    kind: str
    obligations: tuple[ObligationRef, ...]


def mapping_initial(values: Mapping[Cell, Value]) -> InitialValues:
    """Return an initial-value lookup over a finite mapping; unknown cells raise."""

    def lookup(cell: Cell) -> Value:
        try:
            return values[cell]
        except KeyError:
            raise UnknownInitial(f"no initial value for {cell.document()}") from None

    return lookup


@dataclass(frozen=True)
class Problem:
    """Every node and constraint one explanation must satisfy, and initial values.

    Node IDs are unique across groups and observations, and input IDs
    across observations, constraints, and requirements; every constraint
    endpoint names a node of the type its class requires; every Exclusive
    names known outcome variables; constraints are pairwise distinct.
    `initial(cell)` returns the cell's value before any node, or raises
    UnknownInitial.
    """

    groups: tuple[EffectGroup, ...]
    observations: tuple[Observation, ...]
    constraints: tuple[Constraint, ...]
    initial: InitialValues = field(compare=False, repr=False)

    def __post_init__(self) -> None:
        _require(
            type(self.groups) is tuple
            and all(isinstance(g, EffectGroup) for g in self.groups),
            "problem groups must be a tuple of EffectGroups",
        )
        _require(
            type(self.observations) is tuple
            and all(isinstance(o, Observation) for o in self.observations),
            "problem observations must be a tuple of Observations",
        )
        _require(
            type(self.constraints) is tuple
            and all(isinstance(c, CONSTRAINT_TYPES) for c in self.constraints),
            "problem constraints must be a tuple of Constraints",
        )
        ids = [g.id for g in self.groups] + [o.id for o in self.observations]
        _require(len(set(ids)) == len(ids), "problem repeats a node id")
        groups = {g.id for g in self.groups}
        observed = {o.id for o in self.observations}
        variables = {g.outcome_var for g in self.groups}
        names = [constraint_id(c) for c in self.constraints]
        _require(len(set(names)) == len(names), "problem repeats a constraint")
        for constraint in self.constraints:
            for node, required in constraint.signature():
                _require(
                    node in groups or node in observed,
                    f"constraint names unknown node {node!r:.200}",
                )
                _require(
                    required != NodeType.GROUP or node in groups,
                    f"constraint endpoint {node!r:.200} is not a group",
                )
                _require(
                    required != NodeType.OBSERVATION or node in observed,
                    f"constraint endpoint {node!r:.200} is not an observation",
                )
            if isinstance(constraint, Exclusive):
                unknown = sorted(set(constraint.outcome_vars) - variables)
                _require(not unknown, f"exclusive names unknown variables {unknown}")
        inputs = [item.id for item in self.inputs()]
        _require(len(set(inputs)) == len(inputs), "problem repeats an input id")

    def inputs(self) -> tuple[Input, ...]:
        """Return every obligation-bearing input, sorted by ID."""
        found = [
            Input(o.id, OBSERVATION_INPUT, o.obligations) for o in self.observations
        ]
        found += [
            Input(constraint_id(c), CONSTRAINT_INPUT, (c.obligation,))
            for c in self.constraints
            if not isinstance(c, Agree)
        ]
        found += [
            Input(requirement_id(g.id, i), REQUIREMENT_INPUT, (r.obligation,))
            for g in self.groups
            for i, r in enumerate(g.requires)
        ]
        return tuple(sorted(found, key=lambda item: item.id))

    def restrict(self, keep: Iterable[str]) -> Problem:
        """Return this problem with only the obligation-bearing inputs named in `keep`.

        Every group stays (requirements not kept are dropped), and so does
        every Agree constraint, which never constrains an explanation.
        Constraints naming a removed observation are dropped with it.
        """
        kept = frozenset(keep)
        observations = tuple(o for o in self.observations if o.id in kept)
        present = {o.id for o in observations} | {g.id for g in self.groups}
        constraints = tuple(
            c
            for c in self.constraints
            if all(node in present for node in c.nodes())
            and (isinstance(c, Agree) or constraint_id(c) in kept)
        )
        groups = tuple(
            replace(
                g,
                requires=tuple(
                    r
                    for i, r in enumerate(g.requires)
                    if requirement_id(g.id, i) in kept
                ),
            )
            for g in self.groups
        )
        return Problem(groups, observations, constraints, self.initial)


_INPUT_RANK = {OBSERVATION_INPUT: 0, REQUIREMENT_INPUT: 1, "precedence": 2}
_EXCLUSIVE_RANK = 3


def _effect_content(effect: Effect) -> dict[str, Any]:
    """Return an effect's document without its provenance, which names actions."""
    document = effect.document()
    del document["provenance"]
    return document


def _group_content(group: EffectGroup) -> dict[str, Any]:
    return {
        "status": str(group.status),
        "window": group.window.document(),
        "effects": [_effect_content(effect) for effect in group.effects],
        "requires": [
            [
                r.obligation.rule_id,
                [c.document() for c in r.cells],
                r.expect.document(),
            ]
            for r in group.requires
        ],
    }


def _observation_content(observation: Observation) -> dict[str, Any]:
    return {
        "cells": [c.document() for c in observation.cells],
        "expect": observation.expect.document(),
        "window": observation.window.document(),
        "channel": observation.channel,
    }


def node_contents(problem: Problem) -> dict[str, str]:
    """Return every node ID's ID-free canonical content document (JSON text).

    Engines order components by it, so renaming nodes or actions never
    changes which component is decided first.
    """
    found = {
        o.id: {"observation": _observation_content(o)} for o in problem.observations
    }
    found.update((g.id, {"group": _group_content(g)}) for g in problem.groups)
    return {
        name: json.dumps(content, sort_keys=True, separators=(",", ":"))
        for name, content in found.items()
    }


def deletion_order(problem: Problem) -> dict[str, tuple[Any, ...]]:
    """Return every input ID's deletion sort key (see the module docstring).

    Sorting inputs by key puts the latest evidence first; the key ends
    with the input ID, the only ID material, as the final tie-break.
    """
    groups = {g.id: g for g in problem.groups}
    observations = {o.id: o for o in problem.observations}
    var_groups: dict[str, list[EffectGroup]] = {}
    for g in problem.groups:
        var_groups.setdefault(g.outcome_var, []).append(g)

    def node(name: str) -> tuple[Window, dict[str, Any]]:
        if name in observations:
            o = observations[name]
            return o.window, {"observation": _observation_content(o)}
        return groups[name].window, {"group": _group_content(groups[name])}

    keys: dict[str, tuple[Any, ...]] = {}

    def add(
        name: str, rank: int, windows: list[Window], content: dict[str, Any]
    ) -> None:
        lo = max(w.lo for w in windows)
        hi = max(w.hi for w in windows)
        canonical = json.dumps(content, sort_keys=True, separators=(",", ":"))
        keys[name] = (-lo, -hi, rank, canonical, name)

    for o in problem.observations:
        content = {
            "rules": sorted({ref.rule_id for ref in o.obligations}),
            **_observation_content(o),
        }
        add(o.id, _INPUT_RANK[OBSERVATION_INPUT], [o.window], content)
    for g in problem.groups:
        for index, r in enumerate(g.requires):
            content = {
                "rule": r.obligation.rule_id,
                "cells": [c.document() for c in r.cells],
                "expect": r.expect.document(),
                "group": _group_content(g),
            }
            add(
                requirement_id(g.id, index),
                _INPUT_RANK[REQUIREMENT_INPUT],
                [g.window],
                content,
            )
    for constraint in problem.constraints:
        if isinstance(constraint, Agree):
            continue
        name = constraint_id(constraint)
        before = precedence(constraint)
        if before is not None:
            (wa, a), (wb, b) = node(before.a), node(before.b)
            content = {
                "kind": type(constraint).__name__,
                "rule": constraint.obligation.rule_id,
                "a": a,
                "b": b,
            }
            add(name, _INPUT_RANK["precedence"], [wa, wb], content)
            continue
        assert isinstance(constraint, Exclusive)
        members = [var_groups[v] for v in constraint.outcome_vars]
        content = {
            "kind": "Exclusive",
            "rule": constraint.obligation.rule_id,
            "vars": sorted(
                json.dumps(
                    sorted(
                        json.dumps(_group_content(g), sort_keys=True) for g in listed
                    )
                )
                for listed in members
            ),
        }
        windows = [g.window for listed in members for g in listed]
        add(name, _EXCLUSIVE_RANK, windows, content)
    return keys


@dataclass(frozen=True)
class Assignment:
    """One candidate explanation: happened outcome variables and the application order."""

    happened: frozenset[str]
    order: tuple[str, ...]


def holds(expect: Expect, cells: tuple[Cell, ...], state: Mapping[Cell, Value]) -> bool:
    """Return whether `expect` accepts the visible values of `cells` in `state`."""
    return expect.holds(tuple(state[cell] for cell in cells))


class _State(dict[Cell, Value]):
    """Visible values; a cell no node wrote yet reads its initial value."""

    def __init__(self, initial: InitialValues) -> None:
        super().__init__()
        self._initial = initial

    def __missing__(self, cell: Cell) -> Value:
        value = self._initial(cell)
        self[cell] = value
        return value


def _apply(group: EffectGroup, state: _State) -> bool:
    """Apply one happened group to `state`; return False if a requirement fails."""
    for requirement in group.requires:
        if not holds(requirement.expect, requirement.cells, state):
            return False
    writes: dict[Cell, Value] = {}
    for effect in group.effects:
        if effect.guard is not None and not holds(
            effect.guard.expect, effect.guard.cells, state
        ):
            continue
        if effect.kind == EffectKind.PRODUCE:
            assert effect.value is not None
            writes[effect.cell] = effect.value
        else:
            assert effect.source is not None
            writes[effect.cell] = state[effect.source]
            writes[effect.source] = ABSENT
    state.update(writes)
    return True


def _order_respects_windows(nodes: list[Any]) -> bool:
    """Return whether no node precedes one whose window ends before it starts."""
    # A suffix minimum of `hi` makes this linear: node i is preceded only by
    # nodes that may precede it iff every later node's hi >= its lo.
    later_min = math.inf
    for node in reversed(nodes):
        if later_min < node.window.lo:
            return False
        later_min = min(later_min, node.window.hi)
    return True


def explains(  # noqa: C901 - the definition reads best as one checklist
    problem: Problem, assignment: Assignment
) -> bool:
    """Return whether `assignment` explains every input of `problem`.

    The assignment explains the problem when: its happened variables
    include every acked group's variable and name only known variables;
    every Exclusive has at most one happened variable; its order lists
    exactly the happened groups and every observation, once each; no node
    precedes one whose window ends before the node's starts; every
    precedence constraint whose endpoints both happened holds; and
    replaying the order satisfies every requirement and every observation.
    Raises UnknownInitial when replay reads a cell whose initial value is
    unknown.
    """
    variables = {g.outcome_var for g in problem.groups}
    happened = assignment.happened
    acked = {g.outcome_var for g in problem.groups if g.status == Status.ACKED}
    if not acked <= happened or not happened <= variables:
        return False
    for constraint in problem.constraints:
        if isinstance(constraint, Exclusive):
            if len(happened.intersection(constraint.outcome_vars)) > 1:
                return False
    nodes: dict[str, Any] = {
        g.id: g for g in problem.groups if g.outcome_var in happened
    }
    nodes.update((o.id, o) for o in problem.observations)
    order = assignment.order
    if len(order) != len(nodes) or set(order) != set(nodes):
        return False
    sequence = [nodes[node] for node in order]
    if not _order_respects_windows(sequence):
        return False
    position = {node: index for index, node in enumerate(order)}
    for constraint in problem.constraints:
        before = precedence(constraint)
        if before is None:
            continue
        if before.a in position and before.b in position:
            if position[before.a] > position[before.b]:
                return False
    state = _State(problem.initial)
    for node in sequence:
        if isinstance(node, EffectGroup):
            if not _apply(node, state):
                return False
        elif not holds(node.expect, node.cells, state):
            return False
    return True
