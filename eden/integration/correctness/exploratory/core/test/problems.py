# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Seeded generators of small constraint problems for engine cross-checks."""

from __future__ import annotations

import dataclasses
import math
import random
import unittest
from collections.abc import Callable, Iterable
from typing import Any

from eden.integration.correctness.exploratory.core import effects, solver
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    Agree,
    ANY,
    Atom,
    Before,
    Cell,
    Count,
    Effect,
    EffectGroup,
    EffectKind,
    eq,
    Exclusive,
    Expect,
    Guard,
    MustAppearIn,
    none_of,
    ObligationRef,
    Observation,
    of_class,
    one_of,
    Requirement,
    Status,
    Supersedes,
    Value,
    VisibleAfterAck,
    Window,
)
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.semantics import (
    Assignment,
    explains,
    mapping_initial,
    Problem,
)

VALUES = (ABSENT, Value("file", ("v1",)), Value("file", ("v2",)), Value("dir"))


def cell(key: str, side: str = effects.PRIMARY) -> Cell:
    return Cell(side, "path", key)


def window(lo: float, hi: float) -> Window:
    return Window(lo, hi)


def produce(target: Cell, value: Value, guard: Guard | None = None) -> Effect:
    return Effect(target, EffectKind.PRODUCE, value, None, "test", guard)


def transfer(source: Cell, target: Cell, guard: Guard | None = None) -> Effect:
    return Effect(target, EffectKind.TRANSFER, None, source, "test", guard)


def group(
    gid: str,
    lo: float,
    hi: float,
    *effects_: Effect,
    acked: bool = True,
    var: str = "",
    requires: tuple[Requirement, ...] = (),
) -> EffectGroup:
    return EffectGroup(
        gid,
        "a-" + gid,
        Status.ACKED if acked else Status.INDETERMINATE,
        Window(lo, hi),
        tuple(effects_),
        var,
        requires,
    )


def observe(
    oid: str,
    lo: float,
    hi: float,
    cells: tuple[Cell, ...],
    rows: tuple[tuple[Atom, ...], ...],
    *refs: ObligationRef,
) -> Observation:
    return Observation(
        oid,
        "a-" + oid,
        cells,
        Expect(rows),
        Window(lo, hi),
        "test",
        refs or (ObligationRef("t.rule", oid),),
    )


def _atom(rng: random.Random, likely: Value) -> Atom:
    roll = rng.random()
    if roll < 0.45:
        return eq(likely if rng.random() < 0.7 else rng.choice(VALUES))
    if roll < 0.6:
        return one_of(rng.sample(VALUES, 2))
    if roll < 0.72:
        return none_of([rng.choice(VALUES)])
    if roll < 0.85:
        return of_class([rng.choice(("absent", "file", "dir"))])
    return ANY


def _expect(rng: random.Random, seen: tuple[Value, ...]) -> Expect:
    rows = tuple(
        tuple(_atom(rng, value) for value in seen) for _ in range(rng.choice((1, 1, 2)))
    )
    return Expect(rows)


def _window(rng: random.Random, *, open_end: bool) -> Window:
    lo = rng.randrange(0, 7)
    if open_end and rng.random() < 0.35:
        return Window(lo, math.inf)
    return Window(lo, lo + rng.randrange(0, 4))


def random_problem(
    seed: int,
    *,
    max_nodes: int = 6,
    keys: tuple[str, ...] = ("a", "b", "c"),
    legal: bool = False,
) -> Problem:
    """Return one seeded random problem with at most `max_nodes` nodes.

    With `legal`, every observation's first row is what a randomly chosen
    explanation shows, so the problem is satisfiable (other rows and
    constraints are still random, but constraints the chosen explanation
    breaks are dropped).
    """
    rng = random.Random(seed)
    cells = [cell(k) for k in keys]
    initial = {c: rng.choice(VALUES) for c in cells}
    total = rng.randrange(2, max_nodes + 1)
    groups_n = rng.randrange(1, total) if total > 1 else 1
    refs = [ObligationRef(f"t.r{rng.randrange(2)}", f"s{i}") for i in range(4)]
    groups: list[EffectGroup] = []
    for index in range(groups_n):
        acked = rng.random() < 0.6
        written = rng.sample(cells, min(len(cells), rng.choice((1, 1, 2))))
        effects_: list[Effect] = []
        used: set[Cell] = set()
        for target in written:
            if target in used:
                continue
            guard = None
            if rng.random() < 0.2:
                watched = rng.choice(cells)
                guard = Guard((watched,), Expect(((_atom(rng, initial[watched]),),)))
            others = [c for c in cells if c not in used and c != target]
            if rng.random() < 0.25 and others:
                source = rng.choice(others)
                effects_.append(transfer(source, target, guard))
                used.update((source, target))
            else:
                effects_.append(produce(target, rng.choice(VALUES), guard))
                used.add(target)
        requires: tuple[Requirement, ...] = ()
        if rng.random() < 0.15:
            needed = rng.choice(cells)
            requires = (
                Requirement(
                    (needed,),
                    Expect(((_atom(rng, initial[needed]),),)),
                    rng.choice(refs),
                ),
            )
        var = ""
        if groups and rng.random() < 0.15:
            var = rng.choice(groups).outcome_var
        groups.append(
            EffectGroup(
                f"g{index}",
                f"a-g{index}",
                Status.ACKED if acked else Status.INDETERMINATE,
                _window(rng, open_end=not acked),
                tuple(effects_),
                var,
                requires,
            )
        )
    observations: list[Observation] = []
    for index in range(total - groups_n):
        width = min(len(cells), rng.choice((1, 1, 2)))
        observed = tuple(rng.sample(cells, width))
        observations.append(
            Observation(
                f"o{index}",
                f"a-o{index}",
                observed,
                _expect(rng, tuple(initial[c] for c in observed)),
                _window(rng, open_end=False),
                "test",
                tuple(rng.sample(refs, rng.choice((1, 1, 2)))),
            )
        )
    constraints = _constraints(rng, groups, observations, refs)
    problem = Problem(
        tuple(groups), tuple(observations), tuple(constraints), mapping_initial(initial)
    )
    if legal:
        problem = _legalize(rng, problem, initial)
    return problem


def _constraints(
    rng: random.Random,
    groups: list[EffectGroup],
    observations: list[Observation],
    refs: list[ObligationRef],
) -> list[effects.Constraint]:
    nodes = [g.id for g in groups] + [o.id for o in observations]
    found: dict[str, effects.Constraint] = {}

    def add(constraint: effects.Constraint) -> None:
        found[effects.constraint_id(constraint)] = constraint

    for _ in range(rng.randrange(0, 3) if len(nodes) > 1 else 0):
        a, b = rng.sample(nodes, 2)
        add(Before(a, b, rng.choice(refs)))
    if len(groups) > 1 and rng.random() < 0.3:
        a, b = rng.sample(groups, 2)
        add(Supersedes(a.id, b.id, rng.choice(refs)))
    if groups and observations and rng.random() < 0.25:
        add(
            MustAppearIn(
                rng.choice(groups).id, rng.choice(observations).id, rng.choice(refs)
            )
        )
    if groups and observations and rng.random() < 0.25:
        add(
            VisibleAfterAck(
                rng.choice(groups).id, rng.choice(observations).id, rng.choice(refs)
            )
        )
    variables = sorted({g.outcome_var for g in groups})
    if len(variables) > 1 and rng.random() < 0.3:
        add(Exclusive(tuple(rng.sample(variables, 2)), rng.choice(refs)))
    if len(observations) > 1 and rng.random() < 0.05:
        a, b = rng.sample(observations, 2)
        add(Agree(a.id, b.id, "same", rng.choice(refs)))
    return list(found.values())


def _legalize(
    rng: random.Random, problem: Problem, initial: dict[Cell, Value]
) -> Problem:
    """Rewrite observations so one random explanation satisfies them."""
    acked = {g.outcome_var for g in problem.groups if g.status == Status.ACKED}
    optional = sorted({g.outcome_var for g in problem.groups} - acked)
    happened = acked | {v for v in optional if rng.random() < 0.5}
    nodes = [g for g in problem.groups if g.outcome_var in happened]
    nodes += list(problem.observations)
    points = {
        n.id: rng.uniform(n.window.lo, min(n.window.hi, n.window.lo + 4)) for n in nodes
    }
    order = sorted(nodes, key=lambda n: (points[n.id], rng.random()))
    state = dict(initial)
    rewritten = {}
    keep_groups = {}
    for node in order:
        if isinstance(node, EffectGroup):
            requires = tuple(
                r
                for r in node.requires
                if r.expect.holds(tuple(state[c] for c in r.cells))
            )
            keep_groups[node.id] = requires
            writes: dict[Cell, Value] = {}
            for e in node.effects:
                if e.guard is not None and not e.guard.expect.holds(
                    tuple(state[c] for c in e.guard.cells)
                ):
                    continue
                if e.value is not None:
                    writes[e.cell] = e.value
                elif e.source is not None:
                    writes[e.cell] = state[e.source]
                    writes[e.source] = ABSENT
            state.update(writes)
        else:
            seen = tuple(state[c] for c in node.cells)
            rows = (
                tuple(eq(v) if rng.random() < 0.8 else ANY for v in seen),
            ) + node.expect.rows[1:]
            rewritten[node.id] = Observation(
                node.id,
                node.action,
                node.cells,
                Expect(rows),
                node.window,
                node.channel,
                node.obligations,
            )
    groups = tuple(
        EffectGroup(
            g.id,
            g.action,
            g.status,
            g.window,
            g.effects,
            g.outcome_var,
            keep_groups.get(g.id, g.requires),
        )
        for g in problem.groups
    )
    position = {n.id: i for i, n in enumerate(order)}
    constraints = []
    for c in problem.constraints:
        if isinstance(c, Exclusive):
            if len(happened & set(c.outcome_vars)) > 1:
                continue
        elif not isinstance(c, Agree):
            before = c.as_before()
            if (
                before.a in position
                and before.b in position
                and position[before.a] > position[before.b]
            ):
                continue
        constraints.append(c)
    witness = Assignment(frozenset(happened), tuple(n.id for n in order))
    legal = Problem(
        groups,
        tuple(rewritten[o.id] for o in problem.observations),
        tuple(constraints),
        problem.initial,
    )
    object.__setattr__(legal, "_witness", witness)
    return legal


ProblemFactory = Callable[[int], Problem]


def history_problem(
    seed: int,
    *,
    actions: int,
    keys: int,
    actors: int = 8,
    info_rate: float = 0.05,
    read_rate: float = 0.5,
    stale: bool = False,
) -> Problem:
    """Return a register-style workload produced by one real execution.

    `actors` sequential clients issue `actions` writes (unique values) and
    reads over `keys` cells; each operation takes effect at a random point
    of its interval, and an indeterminate write (window to infinity) takes
    effect at most once, maybe never. Reads observe the value at their
    point, so the problem is satisfiable. With `stale`, the last read of
    a cell that some later-acked write had already overwritten instead
    observes the initial value, a planted stale read.
    """
    rng = random.Random(seed)
    cells = [cell(f"k{i}") for i in range(keys)]
    initial = {c: Value("file", ("init", c.key)) for c in cells}
    clock = [0.0] * actors
    events: list[tuple[float, str, Cell, Value | None, float, float, bool]] = []
    for index in range(actions):
        actor = rng.randrange(actors)
        lo = clock[actor] + rng.uniform(0.0, 1.0)
        hi = lo + rng.uniform(0.1, 3.0)
        clock[actor] = hi
        target = rng.choice(cells)
        if rng.random() < read_rate:
            events.append(
                (rng.uniform(lo, hi), f"r{index:04d}", target, None, lo, hi, True)
            )
            continue
        value = Value("file", (f"w{index:04d}",))
        if rng.random() < info_rate:
            point = lo + rng.uniform(0.0, 6.0) if rng.random() < 0.5 else math.inf
            events.append((point, f"w{index:04d}", target, value, lo, math.inf, False))
        else:
            events.append(
                (rng.uniform(lo, hi), f"w{index:04d}", target, value, lo, hi, True)
            )
    state = dict(initial)
    groups: list[EffectGroup] = []
    observations: list[Observation] = []
    last_read: dict[Cell, int] = {}
    for point, name, target, value, lo, hi, acked in sorted(events, key=lambda e: e[0]):
        if value is not None:
            groups.append(group(name, lo, hi, produce(target, value), acked=acked))
            if point != math.inf:
                state[target] = value
            continue
        last_read[target] = len(observations)
        observations.append(
            observe(
                name,
                lo,
                hi,
                (target,),
                ((eq(state[target]),),),
                ObligationRef("t.read", name),
            )
        )
    if stale:
        for target, index in sorted(last_read.items(), key=lambda item: item[0].key):
            read = observations[index]
            overwritten = any(
                g.status == Status.ACKED
                and g.effects[0].cell == target
                and g.window.hi < read.window.lo
                for g in groups
            )
            if overwritten:
                observations[index] = observe(
                    read.id,
                    read.window.lo,
                    read.window.hi,
                    (target,),
                    ((eq(initial[target]),),),
                    *read.obligations,
                )
                break
    return Problem(
        tuple(sorted(groups, key=lambda g: g.id)),
        tuple(sorted(observations, key=lambda o: o.id)),
        (),
        mapping_initial(initial),
    )


_MIXED_VALUES = (ABSENT, *(Value("file", (f"v{i}",)) for i in range(4)), Value("dir"))


def _true_atom(rng: random.Random, value: Value) -> Atom:
    roll = rng.random()
    if roll < 0.6:
        return eq(value)
    if roll < 0.75:
        return one_of({value, rng.choice(_MIXED_VALUES)})
    if roll < 0.85:
        return none_of(rng.sample([w for w in _MIXED_VALUES if w != value], 2))
    if roll < 0.93:
        return of_class({value.cls})
    return ANY


def _any_atom(rng: random.Random) -> Atom:
    return rng.choice(
        [
            eq(rng.choice(_MIXED_VALUES)),
            none_of({rng.choice(_MIXED_VALUES)}),
            of_class({rng.choice(["file", "absent"])}),
        ]
    )


def _true_expect(rng: random.Random, seen: list[Value]) -> Expect:
    rows = [tuple(_true_atom(rng, value) for value in seen)]
    for _ in range(rng.choice((0, 0, 1, 2))):
        rows.append(tuple(_any_atom(rng) for _ in seen))
    rng.shuffle(rows)
    return Expect(tuple(rows))


def _any_expect(rng: random.Random, width: int) -> Expect:
    return Expect(
        tuple(
            tuple(_any_atom(rng) for _ in range(width))
            for _ in range(rng.choice((1, 2)))
        )
    )


def _apply_group(g: EffectGroup, state: dict[Cell, Value]) -> None:
    writes: dict[Cell, Value] = {}
    for e in g.effects:
        if e.guard is not None and not e.guard.expect.holds(
            tuple(state[c] for c in e.guard.cells)
        ):
            continue
        if e.kind == EffectKind.PRODUCE:
            assert e.value is not None
            writes[e.cell] = e.value
        else:
            assert e.source is not None
            writes[e.cell] = state[e.source]
            writes[e.source] = ABSENT
    state.update(writes)


def mixed_history(  # noqa: C901 - one generated execution, step by step
    seed: int,
    groups_n: int = 30,
    observations_n: int = 25,
    cells_n: int = 8,
    acked_rate: float = 0.6,
) -> Problem:
    """Return a checkout-like history produced by one real execution (satisfiable).

    Legal by construction:
    multi-cell groups (1-3 effects), guards over 1-3 cells (half true at
    their point), transfers, requirements, outcome variables shared by
    several groups, an Exclusive over 3-4 indeterminate variables,
    indeterminate groups with infinite windows (some never happen),
    point windows and ties, and Before constraints the execution keeps
    (vacuous ones name groups that never happened). Shapes used by the
    tests: 12/10/5 (22 nodes), 30/25/8 (55), 80/60/12 (140).
    """
    rng = random.Random(seed)
    cells = [Cell("primary", "path", f"k{i}") for i in range(cells_n)]
    initial = {c: rng.choice(_MIXED_VALUES) for c in cells}
    var_of: list[str] = []
    for index in range(groups_n):
        if var_of and rng.random() < 0.2:
            var_of.append(rng.choice(var_of))
        else:
            var_of.append(f"g{index}")
    variables = sorted(set(var_of))
    acked_var = {v: rng.random() < acked_rate for v in variables}
    exclusive_sets: list[list[str]] = []
    undecided = [v for v in variables if not acked_var[v]]
    happened = {v for v in variables if acked_var[v] or rng.random() < 0.5}
    if len(undecided) >= 3:
        chosen = rng.sample(undecided, rng.choice((3, 4)) if len(undecided) >= 4 else 3)
        keep = rng.choice(chosen + [None])
        for v in chosen:
            if v != keep:
                happened.discard(v)
        if keep:
            happened.add(keep)
        exclusive_sets.append(chosen)
    points: list[tuple[str, int, float]] = []
    for index in range(groups_n):
        points.append(("g", index, rng.uniform(0, 50)))
    for index in range(observations_n):
        points.append(("o", index, rng.uniform(0, 55)))
    points = [(k, i, round(p) if rng.random() < 0.3 else p) for k, i, p in points]
    order = sorted(points, key=lambda n: (n[2], rng.random()))
    state = dict(initial)
    groups: dict[str, EffectGroup] = {}
    observations: dict[str, Observation] = {}
    position: dict[str, int] = {}
    for step, (kind, index, point) in enumerate(order):

        def around(open_end: bool, point: float = point) -> Window:
            roll = rng.random()
            if roll < 0.15:
                return Window(point, point)
            lo = max(0.0, point - rng.uniform(0, 4))
            if open_end and rng.random() < 0.4:
                return Window(lo, math.inf)
            return Window(lo, point + rng.uniform(0, 4))

        if kind == "o":
            oid = f"o{index}"
            seen = tuple(rng.sample(cells, rng.choice((1, 1, 2, 3))))
            observations[oid] = Observation(
                oid,
                "a" + oid,
                seen,
                _true_expect(rng, [state[c] for c in seen]),
                around(rng.random() < 0.1),
                "c",
                (ObligationRef("t.obs", oid),),
            )
            position[oid] = step
            continue
        gid = f"g{index}"
        var = var_of[index]
        status = Status.ACKED if acked_var[var] else Status.INDETERMINATE
        count = rng.choice((1, 1, 2, 3))
        free = list(cells)
        rng.shuffle(free)
        effects_: list[Effect] = []
        used: set[Cell] = set()
        for _ in range(count):
            avail = [c for c in free if c not in used]
            if not avail:
                break
            target = avail[0]
            guard = None
            if rng.random() < 0.3:
                watched = tuple(rng.sample(cells, rng.choice((1, 2, 3))))
                guard = Guard(
                    watched,
                    (
                        _true_expect(rng, [state[c] for c in watched])
                        if rng.random() < 0.5
                        else _any_expect(rng, len(watched))
                    ),
                )
            if avail[1:] and rng.random() < 0.3:
                source = avail[1]
                effects_.append(
                    Effect(target, EffectKind.TRANSFER, None, source, "t", guard)
                )
                used |= {source, target}
            else:
                effects_.append(
                    Effect(
                        target,
                        EffectKind.PRODUCE,
                        rng.choice(_MIXED_VALUES),
                        None,
                        "t",
                        guard,
                    )
                )
                used.add(target)
        requires: list[Requirement] = []
        if var in happened and rng.random() < 0.25:
            needed = tuple(rng.sample(cells, rng.choice((1, 2))))
            requires.append(
                Requirement(
                    needed,
                    _true_expect(rng, [state[c] for c in needed]),
                    ObligationRef("t.req", gid),
                )
            )
        if var in happened:
            made = EffectGroup(
                gid,
                "a" + gid,
                status,
                around(status == Status.INDETERMINATE),
                tuple(effects_),
                var,
                tuple(requires),
            )
            _apply_group(made, state)
            position[gid] = step
        else:
            lo = rng.uniform(0, 50)
            window = Window(
                lo, math.inf if rng.random() < 0.5 else lo + rng.uniform(0, 6)
            )
            if rng.random() < 0.3:
                needed = tuple(rng.sample(cells, 1))
                requires.append(
                    Requirement(
                        needed, _any_expect(rng, 1), ObligationRef("t.req", gid)
                    )
                )
            made = EffectGroup(
                gid, "a" + gid, status, window, tuple(effects_), var, tuple(requires)
            )
        groups[gid] = made
    constraints: dict[str, effects.Constraint] = {}
    placed = sorted(position, key=position.__getitem__)
    for _ in range(rng.randrange(0, 12)):
        a, b = sorted(rng.sample(placed, 2), key=position.__getitem__)
        before = Before(a, b, ObligationRef("t.before", f"{a}<{b}"))
        constraints[effects.constraint_id(before)] = before
    never = [g for g in groups if g not in position]
    for _ in range(min(3, len(never))):
        a = rng.choice(never)
        b = rng.choice(placed)
        ends = (a, b) if rng.random() < 0.5 else (b, a)
        before = Before(*ends, ObligationRef("t.before", f"nh-{a}-{b}"))
        constraints[effects.constraint_id(before)] = before
    for chosen in exclusive_sets:
        exclusive = Exclusive(
            tuple(chosen), ObligationRef("t.excl", "-".join(sorted(chosen)))
        )
        constraints[effects.constraint_id(exclusive)] = exclusive
    return Problem(
        tuple(groups[g] for g in sorted(groups)),
        tuple(observations[o] for o in sorted(observations)),
        tuple(constraints.values()),
        mapping_initial(initial),
    )


NEVER = Value("file", ("never",))


def planted_mixed_history(seed: int, *shape: int) -> tuple[Problem, ObligationRef]:
    """Return `mixed_history` with one observation changed to read a value nobody writes.

    Returns the planted obligation.
    """
    legal = mixed_history(seed, *shape)
    rng = random.Random(seed)
    observations = list(legal.observations)
    index = rng.randrange(len(observations))
    target = observations[index]
    observations[index] = Observation(
        target.id,
        target.action,
        target.cells,
        Expect((tuple(eq(NEVER) for _ in target.cells),)),
        target.window,
        target.channel,
        target.obligations,
    )
    planted = Problem(
        legal.groups, tuple(observations), legal.constraints, legal.initial
    )
    return planted, target.obligations[0]


def shared_variable_problem(seed: int, *, max_nodes: int = 8) -> Problem:
    """Return a problem whose groups couple only through shared outcome variables.

    Each group writes its own cell (no two groups share a cell, no guards
    or transfers), indeterminate groups share a few outcome variables,
    and every observation reads one cell after the writes: only the
    outcome-variable coupling relates different cells.
    """
    rng = random.Random(seed)
    groups_n = rng.randrange(2, max_nodes - 1)
    cells = [cell(f"c{i}") for i in range(groups_n)]
    initial = {c: VALUES[0] for c in cells}
    variables = [f"v{i}" for i in range(rng.randrange(1, 3))]
    groups = []
    for index, target in enumerate(cells):
        acked = rng.random() < 0.2
        var = "" if acked else rng.choice(variables)
        groups.append(
            group(
                f"g{index}",
                0,
                rng.choice((1, 2, math.inf)),
                produce(target, VALUES[1]),
                acked=acked,
                var=var,
            )
        )
    observations = []
    for index in range(rng.randrange(1, max_nodes - groups_n + 1)):
        target = rng.choice(cells)
        seen = rng.choice(VALUES[:2])
        observations.append(
            observe(
                f"o{index}",
                5,
                6,
                (target,),
                ((eq(seen),),),
                ObligationRef("t.read", f"o{index}"),
            )
        )
    return Problem(tuple(groups), tuple(observations), (), mapping_initial(initial))


REVISIONS = tuple(Value("rev", (str(i),)) for i in range(3))
MARK = Value("mark")
RESTART = 8.0


def _content(revision: int, path: int) -> Value:
    return (
        ABSENT if (revision + path) % 3 == 2 else Value("file", (f"{revision}{path}",))
    )


def checkout_problem(
    seed: int, *, max_nodes: int = 8, unknown: bool | None = None
) -> Problem:
    """Return a checkout-shaped problem with at most `max_nodes` nodes.

    Each checkout is a parent group (the side's parent cell becomes the
    destination and the checkout's mark cell is set) and one group per
    path that requires, on two cells, the parent at the destination and the
    mark set (one order obligation shared by the checkout's requirements),
    and writes the destination's content under a guard that the path holds
    some revision's content. Checkouts pick a side (primary or reference).
    Acked windows may be open-ended; indeterminate windows end at a daemon
    restart or never. Reads (one cell or the parent plus a path) are
    legalized against a random explanation, then up to two are corrupted;
    some seeds keep the requirements that explanation broke.
    With `unknown` (default: a quarter of the seeds) one path cell has no
    initial value.
    """
    rng = random.Random(seed)
    checkouts = rng.choice((1, 2))
    paths = rng.choice((1, 2)) if max_nodes - checkouts * 3 >= 1 else 1
    reads = max(1, min(3, max_nodes - checkouts * (1 + paths)))
    sides = [rng.choice((effects.PRIMARY, effects.PRIMARY, effects.REFERENCE))]
    sides += [rng.choice((sides[0], effects.REFERENCE)) for _ in range(checkouts - 1)]
    distinct = sorted(set(sides))
    path_cells = {
        side: [cell(f"r{i}", side) for i in range(paths)] for side in distinct
    }
    parents = {side: Cell(side, "parent", "") for side in distinct}
    initial: dict[Cell, Value] = {}
    for side in distinct:
        initial[parents[side]] = REVISIONS[0]
        for i, target in enumerate(path_cells[side]):
            dirty = rng.random() < 0.3
            initial[target] = Value("file", ("dirty",)) if dirty else _content(0, i)
    groups: list[EffectGroup] = []
    for j, side in enumerate(sides):
        mark = Cell(side, "visit", f"k{j}")
        initial[mark] = ABSENT
        destination = rng.randrange(1, len(REVISIONS))
        acked = rng.random() < 0.6
        lo = float(rng.randrange(0, 6))
        hi = (
            rng.choice((lo + 1, lo + 3, math.inf))
            if acked
            else rng.choice((RESTART, math.inf))
        )
        groups.append(
            group(
                f"c{j}",
                lo,
                hi,
                produce(parents[side], REVISIONS[destination]),
                produce(mark, MARK),
                acked=acked,
            )
        )
        order = ObligationRef("t.order", f"c{j}")
        for i, target in enumerate(path_cells[side]):
            clean = one_of([_content(r, i) for r in range(len(REVISIONS))])
            groups.append(
                group(
                    f"c{j}p{i}",
                    lo,
                    hi,
                    produce(
                        target,
                        _content(destination, i),
                        Guard((target,), Expect(((clean,),))),
                    ),
                    acked=acked,
                    requires=(
                        Requirement(
                            (parents[side], mark),
                            Expect(((eq(REVISIONS[destination]), eq(MARK)),)),
                            order,
                        ),
                    ),
                )
            )
    observations = []
    for index in range(reads):
        side = rng.choice(sides)
        target = rng.choice(path_cells[side])
        cells = (parents[side], target) if rng.random() < 0.3 else (target,)
        lo = float(rng.randrange(0, 11))
        observations.append(
            observe(
                f"o{index}",
                lo,
                lo + rng.randrange(0, 3),
                cells,
                (tuple(ANY for _ in cells),),
                ObligationRef(f"t.{side}-read", f"o{index}"),
            )
        )
    problem = _legalize(
        rng,
        Problem(tuple(groups), tuple(observations), (), mapping_initial(initial)),
        initial,
    )
    corrupted = list(problem.observations)
    for _ in range(rng.choice((0, 1, 1, 2))):
        index = rng.randrange(len(corrupted))
        o = corrupted[index]
        seen = rng.choice((ABSENT, Value("file", ("dirty",)), _content(1, 0)))
        corrupted[index] = Observation(
            o.id,
            o.action,
            o.cells,
            Expect((tuple(ANY for _ in o.cells[:-1]) + (eq(seen),),)),
            o.window,
            o.channel,
            o.obligations,
        )
    # Keep the requirements the explanation broke, so order obligations fail too.
    kept = tuple(groups) if rng.random() < 0.3 else problem.groups
    if unknown is None:
        unknown = rng.random() < 0.25
    if unknown:
        side = rng.choice(sides)
        del initial[rng.choice(path_cells[side])]
    return Problem(
        kept, tuple(corrupted), problem.constraints, mapping_initial(initial)
    )


COUNT_MARK = Value("yes")


def counted_problem(seed: int, base: Problem) -> Problem | None:
    """Return `base` plus one count observation over guarded markers, or None.

    One to three groups that produce a value also set their own marker cell
    (initially absent) under a guard on the produced cell's value. One
    observation at the latest marking group's end, ordered after every
    marking group (`Before`), expects exactly `total` markers set: the shape
    of a counted checkout's conflict count. None when no group produces, a
    marking group never ends, markers span sides, or `total` exceeds the
    markers.
    """
    rng = random.Random(seed)
    groups = list(base.groups)
    producing = [
        i
        for i, g in enumerate(groups)
        if any(e.kind == EffectKind.PRODUCE for e in g.effects)
    ]
    if not producing:
        return None
    picked = rng.sample(producing, min(len(producing), rng.choice((1, 2, 3))))
    hi = max(groups[i].window.hi for i in picked)
    if hi == math.inf:
        return None
    ref = ObligationRef("t.count", "c")
    produced = {e.value for g in base.groups for e in g.effects if e.value is not None}
    values = sorted(produced | {ABSENT}, key=repr)
    markers: list[Cell] = []
    orderings: list[Before] = []
    for i in picked:
        g = groups[i]
        target = next(e.cell for e in g.effects if e.kind == EffectKind.PRODUCE)
        marker = Cell(target.side, "marker", f"m{i}")
        guard = Guard((target,), Expect(((eq(rng.choice(values)),),)))
        mark = Effect(marker, EffectKind.PRODUCE, COUNT_MARK, None, "mark", guard)
        groups[i] = EffectGroup(
            g.id,
            g.action,
            g.status,
            g.window,
            g.effects + (mark,),
            g.outcome_var,
            g.requires,
        )
        markers.append(marker)
        orderings.append(Before(g.id, "count", ref))
    weights = tuple(rng.choice(((1, 1), (2, 2), (1, 2), (0, 1))) for _ in markers)
    total = rng.randrange(0, sum(high for _low, high in weights) + 2)
    if len({m.side for m in markers}) > 1 or total > sum(h for _l, h in weights):
        return None
    count = Observation(
        "count",
        "count",
        tuple(markers),
        Expect(count=Count(COUNT_MARK, weights, total)),
        Window(hi, hi),
        "thrift",
        (ref,),
    )
    initial = base.initial

    def lookup(c: Cell) -> Value:
        return ABSENT if c.space == "marker" else initial(c)

    return Problem(
        tuple(groups),
        base.observations + (count,),
        base.constraints + tuple(orderings),
        lookup,
    )


def check_legal_batches(
    test: unittest.TestCase, batches: Iterable[tuple[Any, ...]]
) -> list[tuple[int, str]]:
    """Solve `mixed_history` batches at default budgets; return (satisfied, summary) per batch.

    Each batch is (groups, observations, cells, acked rate, seeds, decided
    at least). A legal history is never violated; it is satisfied with a
    witness that explains it, or left to the budget. The caller compares
    each satisfied count with its floor.
    """
    rows = []
    results: list[tuple[int, str]] = []
    for groups, observations, cells, rate, seeds, _floor in batches:
        decided = undecided = 0
        work = states = 0
        for seed in seeds:
            problem = mixed_history(seed, groups, observations, cells, rate)
            solution = solver.solve(problem)
            statuses = {o.status for o in solution.outcomes.values()}
            test.assertNotIn(ObligationStatus.VIOLATED, statuses, seed)
            if statuses == {ObligationStatus.SATISFIED}:
                decided += 1
                assert solution.witness is not None
                test.assertTrue(explains(problem, solution.witness), seed)
            else:
                undecided += 1
                test.assertEqual(
                    {solver.BUDGET},
                    {
                        o.reason
                        for o in solution.outcomes.values()
                        if o.status == ObligationStatus.NOT_EVALUATED
                    },
                    seed,
                )
            components = solution.components.values()
            work = max(work, max(c["decision_work"] for c in components))
            states = max(states, max(c["states_high_water"] for c in components))
        rows.append(
            f"nodes={groups + observations} acked={rate} "
            f"satisfied={decided}/{len(seeds)} budget={undecided} "
            f"max_component_work={work} max_states={states}"
        )
        results.append((decided, rows[-1]))
    return results


SATISFIED = ObligationStatus.SATISFIED


VIOLATED = ObligationStatus.VIOLATED


NOT_EVALUATED = ObligationStatus.NOT_EVALUATED


V1, V2 = Value("file", ("v1",)), Value("file", ("v2",))


def make_problem(groups=(), observations=(), constraints=(), initial=None) -> Problem:
    return Problem(
        tuple(groups),
        tuple(observations),
        tuple(constraints),
        mapping_initial(initial or {}),
    )


def sees(
    oid: str, lo: float, hi: float, target: Cell, value: Value, *refs: ObligationRef
) -> Observation:
    return observe(oid, lo, hi, (target,), ((eq(value),),), *refs)


def hard_component(
    prefix: str, target: Cell, n: int, refs: tuple[ObligationRef, ...]
) -> tuple[list, list]:
    """n concurrent writes, then two sequential reads of different written values.

    A late write overlaps both reads with a value neither expects, so the
    reads are not past every writer (`_settled_conflict` does not apply).
    Unsatisfiable, but propagation cannot see it (every value may be last),
    so search explores every write order: exponential in n.
    """
    groups = [
        group(
            f"{prefix}w{i:02d}",
            0,
            100,
            produce(target, Value("file", (f"{prefix}{i}",))),
        )
        for i in range(n)
    ]
    groups.append(
        group(
            f"{prefix}late",
            150,
            250,
            produce(target, Value("file", (f"{prefix}late",))),
        )
    )
    reads = [
        sees(f"{prefix}r1", 200, 201, target, Value("file", (f"{prefix}0",)), refs[0]),
        sees(f"{prefix}r2", 202, 203, target, Value("file", (f"{prefix}1",)), refs[1]),
    ]
    return groups, reads


OLD, NEW = Value("file", ("old",)), Value("file", ("new",))


def lost_write_problem(channels: tuple[str, ...]) -> Problem:
    """One read sees an unacked write's value, then later reads see the old one."""
    p = cell("p")
    fresh = sees("fresh", 2, 3, p, NEW, ObligationRef("fuse.read", "fresh"))
    stale = [
        dataclasses.replace(
            sees(
                f"s{i}",
                4 + 2 * i,
                5 + 2 * i,
                p,
                OLD,
                ObligationRef(f"{channel}.stale-read", f"s{i}"),
            ),
            channel=channel,
        )
        for i, channel in enumerate(channels)
    ]
    write = group("w", 0, 1, produce(p, NEW), acked=False)
    return make_problem([write], [fresh, *stale], initial={p: OLD})
