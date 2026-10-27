# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""An independent brute-force enumerator of `semantics` for tiny problems.

It shares no code with `solver`; from `semantics` it takes the frozen
problem types, `Problem.inputs` and `Problem.restrict`, and
`deletion_order`, and it replays nodes with its own `_step`. For every
subset of the indeterminate outcome variables it enumerates every order of
the happened nodes that respects windows, precedence constraints, and
Exclusive, and replays each order directly. It exists to cross-check the
optimized engine and is bounded to `MAX_NODES` nodes.

`deletion_results` reproduces the engine's result contract from first
principles with this enumerator as the satisfiability check: while the
remaining inputs are unsatisfiable, delete inputs one at a time in
`semantics.deletion_order` (an input stays only when the rest becomes
satisfiable), report the core, and remove it; then, for each reported
core `P` in order (growing as it goes) and each member `k` of `P`, while
`P - {k}` plus every input not yet in a core is unsatisfiable, the
deletion core of that set is one more core. The order is
part of the result contract's definition, not of any search.
"""

from __future__ import annotations

import itertools
from collections.abc import Iterator
from dataclasses import dataclass
from typing import Any

from .effects import (
    ABSENT,
    Agree,
    Before,
    Cell,
    EffectKind,
    Exclusive,
    MustAppearIn,
    ObligationRef,
    Observation,
    Status,
    Supersedes,
    Value,
    VisibleAfterAck,
)
from .semantics import Assignment, deletion_order, Input, Problem

MAX_NODES = 8


@dataclass(frozen=True)
class ReferenceCore:
    """One reported unsatisfiable core: its input IDs and their obligations."""

    inputs: tuple[str, ...]
    obligations: tuple[ObligationRef, ...]


@dataclass(frozen=True)
class ReferenceResults:
    """The per-obligation results the engine must reproduce.

    `cores` are in the order they were found (deletion cores first, then
    the cores the remaining inputs complete); `violated` maps every
    obligation some core names to the indices of every such core;
    `satisfied` is every other obligation an input supports.
    """

    cores: tuple[ReferenceCore, ...]
    violated: dict[ObligationRef, tuple[int, ...]]
    satisfied: frozenset[ObligationRef]


def _edges(problem: Problem) -> list[tuple[str, str]]:
    edges = []
    for constraint in problem.constraints:
        if isinstance(constraint, (Before, Supersedes)):
            edges.append((constraint.a, constraint.b))
        elif isinstance(constraint, MustAppearIn):
            edges.append((constraint.observation, constraint.group))
        elif isinstance(constraint, VisibleAfterAck):
            edges.append((constraint.group, constraint.observation))
    return edges


def _read(state: dict[Cell, Value], problem: Problem, cell: Cell) -> Value:
    if cell not in state:
        state[cell] = problem.initial(cell)
    return state[cell]


def _accepts(
    expect: Any, cells: Any, state: dict[Cell, Value], problem: Problem
) -> bool:
    values = [_read(state, problem, cell) for cell in cells]
    for row in expect.rows:
        if all(atom.matches(value) for atom, value in zip(row, values)):
            return True
    return False


def _step(
    node: Any, state: dict[Cell, Value], problem: Problem
) -> dict[Cell, Value] | None:
    """Return the state after `node`, or None when the node is not satisfied."""
    if isinstance(node, Observation):
        return state if _accepts(node.expect, node.cells, state, problem) else None
    after = dict(state)
    for requirement in node.requires:
        if not _accepts(requirement.expect, requirement.cells, state, problem):
            return None
    for effect in node.effects:
        guard = effect.guard
        if guard is not None and not _accepts(
            guard.expect, guard.cells, state, problem
        ):
            continue
        if effect.kind == EffectKind.PRODUCE:
            after[effect.cell] = effect.value
        else:
            after[effect.cell] = _read(state, problem, effect.source)
            after[effect.source] = ABSENT
    return after


def _orders(
    nodes: list[Any],
    edges: list[tuple[str, str]],
    problem: Problem,
) -> Iterator[tuple[str, ...]]:
    """Yield every order of `nodes` that respects windows and edges and replays."""
    present = {node.id for node in nodes}
    preds: dict[str, set[str]] = {node.id: set() for node in nodes}
    for a, b in edges:
        if a in present and b in present:
            preds[b].add(a)

    def extend(
        prefix: list[Any], rest: list[Any], state: dict[Cell, Value]
    ) -> Iterator[tuple[str, ...]]:
        if not rest:
            yield tuple(node.id for node in prefix)
            return
        placed = {node.id for node in prefix}
        for index, node in enumerate(rest):
            if any(
                other.window.hi < node.window.lo for other in rest if other is not node
            ):
                continue
            if not preds[node.id] <= placed:
                continue
            after = _step(node, state, problem)
            if after is None:
                continue
            yield from extend(prefix + [node], rest[:index] + rest[index + 1 :], after)

    yield from extend([], sorted(nodes, key=lambda node: node.id), {})


def satisfying(problem: Problem) -> Assignment | None:
    """Return one assignment that explains `problem`, or None when none does."""
    nodes = len(problem.groups) + len(problem.observations)
    if nodes > MAX_NODES:
        raise ValueError(f"reference semantics is bounded to {MAX_NODES} nodes")
    acked = {g.outcome_var for g in problem.groups if g.status == Status.ACKED}
    optional = sorted({g.outcome_var for g in problem.groups} - acked)
    exclusives = [c for c in problem.constraints if isinstance(c, Exclusive)]
    edges = _edges(problem)
    for size in range(len(optional) + 1):
        for chosen in itertools.combinations(optional, size):
            happened = frozenset(acked | set(chosen))
            if any(len(happened & set(c.outcome_vars)) > 1 for c in exclusives):
                continue
            nodes_now: list[Any] = [
                g for g in problem.groups if g.outcome_var in happened
            ]
            nodes_now += list(problem.observations)
            for order in _orders(nodes_now, edges, problem):
                return Assignment(happened, order)
    return None


def satisfiable(problem: Problem) -> bool:
    """Return whether some assignment explains `problem`."""
    return satisfying(problem) is not None


def _obligations(inputs: list[Input]) -> tuple[ObligationRef, ...]:
    refs = {ref for item in inputs for ref in item.obligations}
    return tuple(sorted(refs, key=ObligationRef.document))


def minimal_unsat_subsets(problem: Problem) -> tuple[ReferenceCore, ...]:
    """Return every minimal unsatisfiable set of obligation-bearing inputs.

    Unsatisfiability is upward closed in the inputs, so a set is minimal
    exactly when it is unsatisfiable and contains no smaller such set.
    Sets are enumerated by size; the result is sorted by size, then IDs.
    """
    inputs = list(problem.inputs())
    found: list[frozenset[str]] = []
    cores = []
    for size in range(len(inputs) + 1):
        for chosen in itertools.combinations(inputs, size):
            ids = frozenset(item.id for item in chosen)
            if any(core <= ids for core in found):
                continue
            if not satisfiable(problem.restrict(ids)):
                found.append(ids)
                cores.append(
                    ReferenceCore(tuple(sorted(ids)), _obligations(list(chosen)))
                )
    return tuple(cores)


def _deletion_core(problem: Problem, pool: list[str]) -> list[str]:
    core = list(pool)
    for candidate in pool:
        trial = [item for item in core if item != candidate]
        if not satisfiable(problem.restrict(trial)):
            core = trial
    return core


def deletion_results(problem: Problem) -> ReferenceResults:
    """Return the cores and per-obligation results of iterated deletion."""
    by_id = {item.id: item for item in problem.inputs()}
    keys = deletion_order(problem)
    ordered = sorted(by_id, key=keys.__getitem__)
    remaining = list(ordered)
    found: list[list[str]] = []
    while not satisfiable(problem.restrict(remaining)):
        core = _deletion_core(problem, remaining)
        found.append(core)
        remaining = [item for item in remaining if item not in set(core)]
    blamed = {item for core in found for item in core}
    index = 0
    while index < len(found):
        proof = found[index]
        index += 1
        for k in [item for item in ordered if item in proof]:
            while True:
                trial = [
                    item
                    for item in ordered
                    if (item in proof and item != k) or item not in blamed
                ]
                if satisfiable(problem.restrict(trial)):
                    break
                core = _deletion_core(problem, trial)
                found.append(core)
                blamed.update(core)
    cores = tuple(
        ReferenceCore(tuple(sorted(core)), _obligations([by_id[i] for i in core]))
        for core in found
    )
    violated: dict[ObligationRef, tuple[int, ...]] = {}
    for index, core in enumerate(cores):
        for ref in core.obligations:
            violated[ref] = (*violated.get(ref, ()), index)
    every = {ref for item in by_id.values() for ref in item.obligations}
    return ReferenceResults(cores, violated, frozenset(every - set(violated)))


def agree_obligations(problem: Problem) -> frozenset[ObligationRef]:
    """Return the obligations Agree constraints support; no explanation decides them."""
    return frozenset(c.obligation for c in problem.constraints if isinstance(c, Agree))
