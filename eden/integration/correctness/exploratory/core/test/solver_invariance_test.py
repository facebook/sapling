# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Solver results depend only on the problem's content: renaming actions,
reordering inputs, or adding independent operations changes no status, proof,
or blamed cell; work does not grow with untouched cells; results and counters
are identical across hash seeds; and a larger budget only turns not-evaluated
obligations into satisfied or violated ones.
"""

from __future__ import annotations

import dataclasses
import json
import random
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core import exhaustive, solver
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    Agree,
    Cell,
    Constraint,
    constraint_id,
    Effect,
    EffectGroup,
    EffectKind,
    eq,
    Exclusive,
    MustAppearIn,
    ObligationRef,
    Status,
    UnknownInitial,
    Value,
    VisibleAfterAck,
    Window,
)
from eden.integration.correctness.exploratory.core.semantics import (
    deletion_order,
    Problem,
    requirement_id,
)
from eden.integration.correctness.exploratory.core.test.problems import (
    cell,
    checkout_problem,
    group,
    hard_component,
    history_problem,
    lost_write_problem,
    make_problem,
    NOT_EVALUATED,
    observe,
    produce,
    random_problem,
    SATISFIED,
    sees,
    V1,
    VIOLATED,
)
from eden.integration.correctness.exploratory.core.test.testing import (
    run_in_fresh_interpreter,
)


def _rename(problem: Problem, names: dict[str, str]) -> Problem:
    def node(name: str) -> str:
        return names.get(name, name)

    groups = tuple(
        dataclasses.replace(
            g, id=node(g.id), action=node(g.action), outcome_var=node(g.outcome_var)
        )
        for g in problem.groups
    )
    observations = tuple(
        dataclasses.replace(o, id=node(o.id), action=node(o.action))
        for o in problem.observations
    )
    constraints: list[Constraint] = []
    for c in problem.constraints:
        if isinstance(c, Exclusive):
            constraints.append(
                Exclusive(tuple(node(v) for v in c.outcome_vars), c.obligation)
            )
        elif isinstance(c, Agree):
            constraints.append(
                Agree(node(c.o1), node(c.o2), c.normalize_id, c.obligation)
            )
        elif isinstance(c, (MustAppearIn, VisibleAfterAck)):
            constraints.append(
                type(c)(node(c.group), node(c.observation), c.obligation)
            )
        else:
            constraints.append(type(c)(node(c.a), node(c.b), c.obligation))
    return Problem(groups, observations, tuple(constraints), problem.initial)


def _input_names(problem: Problem, names: dict[str, str]) -> dict[str, str]:
    """Map every input ID of `problem` to its ID after `_rename`."""
    renamed = _rename(problem, names)
    old = list(problem.constraints)
    mapping = {o.id: names.get(o.id, o.id) for o in problem.observations}
    for before, after in zip(old, renamed.constraints):
        mapping[constraint_id(before)] = constraint_id(after)
    for g in problem.groups:
        for index in range(len(g.requires)):
            mapping[requirement_id(g.id, index)] = requirement_id(
                names.get(g.id, g.id), index
            )
    return mapping


def _content_ties(problem: Problem) -> bool:
    """Whether two inputs share a deletion key up to their IDs (the only ID tie-break)."""
    keys = [key[:-1] for key in deletion_order(problem).values()]
    return len(set(keys)) != len(keys)


class _CountingPinned:
    """Pinned facts over a sparse domain of `size` cells; counts every query.

    Key `f<index>` exists for index < size; hot cells (the ones writes
    target) start at a per-cell value, every other cell at "stable".
    """

    def __init__(self, size: int) -> None:
        self.size = size
        self.queries = 0
        self.highest = -1
        self.hot: set[str] = set()

    def key(self, fraction: float, offset: int) -> str:
        """Return the key at `fraction` of the domain plus `offset` (scales with size)."""
        return f"f{int(self.size * fraction) + offset:08d}"

    def initial(self, c: Cell) -> Value:
        self.queries += 1
        index = int(c.key[1:])
        if index >= self.size:
            raise UnknownInitial(c.key)
        self.highest = max(self.highest, index)
        return (
            Value("file", (c.key,)) if c.key in self.hot else Value("file", ("stable",))
        )


def _sparse_problem(pinned: _CountingPinned) -> Problem:
    """A fixed interaction surface whose keys spread over the whole domain.

    Hot, listed, and cold keys sit at fixed fractions of the domain, so a
    larger domain queries larger keys while the structure is unchanged.
    """
    rng = random.Random(7)
    hot = [Cell("primary", "path", pinned.key(0.01 + i * 0.04, i)) for i in range(20)]
    pinned.hot = {c.key for c in hot}
    groups, observations = [], []
    clock = 0.0
    for i in range(200):
        target = rng.choice(hot)
        lo = clock
        clock += rng.uniform(0.2, 1.0)
        groups.append(
            group(f"w{i:03d}", lo, lo + 1.5, produce(target, Value("file", (f"w{i}",))))
        )
    for i in range(100):
        cold = Cell("primary", "path", pinned.key(0.9, i))
        groups.append(group(f"x{i:03d}", 0, 300, produce(cold, V1)))
    # Last writes before t=300 decide every hot cell; reads observe them.
    state = {c: pinned.initial(c) for c in hot}
    for g in sorted(groups, key=lambda g: g.window.hi):
        if g.id.startswith("w"):
            state[g.effects[0].cell] = g.effects[0].value
    for i, c in enumerate(hot):
        observations.append(
            sees(f"r{i:02d}", 400, 401, c, state[c], ObligationRef("t.read", f"{i}"))
        )
    untouched = tuple(Cell("primary", "path", pinned.key(0.5, i)) for i in range(50))
    observations.append(
        observe(
            "listing",
            400,
            401,
            untouched,
            (tuple(eq(Value("file", ("stable",))) for _ in untouched),),
            ObligationRef("t.listing", "untouched"),
        )
    )
    pinned.queries = 0
    pinned.highest = -1
    return Problem(tuple(groups), tuple(observations), (), pinned.initial)


def _payload() -> str:
    """Canonical engine output for fixed problems, for cross-process determinism."""
    documents = []
    for seed in range(150):
        problem = random_problem(
            seed, max_nodes=7, keys=("a", "b", "c"), legal=seed % 2 == 0
        )
        solution = solver.solve(problem)
        documents.append(
            {
                "outcomes": sorted(
                    [ref.document(), str(o.status), o.component, o.proofs, o.reason]
                    for ref, o in solution.outcomes.items()
                ),
                "proofs": [p.document() for p in solution.proofs],
                "phases": solution.phases,
                "components": solution.components,
            }
        )
    solution = solver.solve(history_problem(3, actions=300, keys=8, stale=True))
    documents.append(
        {
            "phases": solution.phases,
            "components": solution.components,
            "proofs": [p.document() for p in solution.proofs],
        }
    )
    return json.dumps(documents, sort_keys=True)


# Increasing budgets along each axis, smallest first; the default budget ends each.
LADDERS = (
    tuple(solver.Budgets(explanation_work=w) for w in (16, 64, 256, 1024, 4096)),
    tuple(solver.Budgets(decision_work=w) for w in (64, 512, 4096)),
)


class InvarianceTest(unittest.TestCase):
    def test_overlapping_cores_blame_the_same_cells_under_renaming(self) -> None:
        """The MUSes {x-read, late read} and {y-read, late read} survive renaming."""
        x, y = cell("x"), cell("y")
        a, b = Value("file", ("A",)), Value("file", ("B",))

        def build(x_read: str, y_read: str) -> Problem:
            checkout = group("chk", 0, 10, produce(x, b), produce(y, b))
            return make_problem(
                [checkout],
                [
                    sees(x_read, 1, 2, x, b, ObligationRef("p.bytes", x_read)),
                    sees(y_read, 1, 2, y, b, ObligationRef("p.bytes", y_read)),
                    sees("r-mid", 3, 4, x, a, ObligationRef("p.bytes", "r-mid")),
                ],
                initial={x: a, y: a},
            )

        outcomes = []
        for x_read, y_read in (("r-a", "r-z"), ("r-z", "r-a")):
            solution = solver.solve(build(x_read, y_read))
            cells = {x_read: "x", y_read: "y", "r-mid": "x-late"}
            outcomes.append(
                {cells[r.scope]: o.status for r, o in solution.outcomes.items()}
            )
        self.assertEqual(outcomes[0], outcomes[1])
        # Each early read is in its own minimal core with the late read.
        self.assertEqual(
            {"x": VIOLATED, "y": VIOLATED, "x-late": VIOLATED}, outcomes[0]
        )

    def test_provenance_naming_actions_does_not_order_deletion(self) -> None:
        """Effect provenance carries the action ID; swapping IDs keeps the blamed rule."""
        x = cell("x")
        a, b, c = (Value("file", (name,)) for name in ("A", "B", "C"))

        def build(first: str, second: str) -> Problem:
            writes = [
                EffectGroup(
                    f"grp-{action}",
                    action,
                    Status.ACKED,
                    Window(0, 10),
                    (Effect(x, EffectKind.PRODUCE, value, None, action),),
                )
                for action, value in ((first, b), (second, c))
            ]
            read = sees("obs-r", 5, 6, x, a, ObligationRef("p.bytes", "obs-r"))
            constraints = [
                VisibleAfterAck(writes[0].id, read.id, ObligationRef("p.visible", "w")),
                VisibleAfterAck(writes[1].id, read.id, ObligationRef("q.visible", "w")),
            ]
            return make_problem(writes, [read], constraints, initial={x: a})

        violated = [
            sorted(
                ref.rule_id
                for ref, outcome in solver.solve(build(*names)).outcomes.items()
                if outcome.status == VIOLATED
            )
            for names in (("act-1", "act-2"), ("act-2", "act-1"))
        ]
        self.assertEqual(violated[0], violated[1])

    def test_action_id_renaming_preserves_results_and_proofs(self) -> None:
        tally = {"checked": 0, "overlapping": 0, "content-ties": 0}
        for seed in range(400_000, 401_200):
            problem = random_problem(seed, max_nodes=6, keys=("a", "b"))
            if len(problem.inputs()) > 7:
                continue
            muses = exhaustive.minimal_unsat_subsets(problem)
            ids = [set(m.inputs) for m in muses]
            if any(a & b for i, a in enumerate(ids) for b in ids[i + 1 :]):
                tally["overlapping"] += 1
            if _content_ties(problem):
                # Identical-content inputs fall back to their IDs; their
                # subjects are identical too, but scopes are not.
                tally["content-ties"] += 1
                continue
            rng = random.Random(seed)
            nodes = [g.id for g in problem.groups] + [
                o.id for o in problem.observations
            ]
            variables = {g.outcome_var for g in problem.groups} - set(nodes)
            shuffled = list(nodes)
            rng.shuffle(shuffled)
            names = {n: f"renamed-{s}" for n, s in zip(nodes, shuffled)}
            names.update({v: f"renamed-var-{v}" for v in variables})
            renamed = _rename(problem, names)
            inputs = _input_names(problem, names)
            before, after = solver.solve(problem), solver.solve(renamed)
            self.assertEqual(
                {r: o.status for r, o in before.outcomes.items()},
                {r: o.status for r, o in after.outcomes.items()},
                seed,
            )
            self.assertEqual(
                sorted(
                    tuple(sorted(inputs[i] for i in p.observations + p.constraints))
                    for p in before.proofs
                ),
                sorted(
                    tuple(sorted(p.observations + p.constraints)) for p in after.proofs
                ),
                seed,
            )
            self.assertEqual(
                sorted(
                    tuple(sorted(names.get(g, g) for g in p.groups))
                    for p in before.proofs
                ),
                sorted(p.groups for p in after.proofs),
                seed,
            )
            tally["checked"] += 1
        self.assertGreater(tally["checked"], 500, tally)
        self.assertGreater(tally["overlapping"], 30, tally)

    def test_every_proof_of_an_obligation_survives_renaming(self) -> None:
        """An obligation two proofs violate exposes both, whatever the node IDs."""
        p, q = cell("p"), cell("q")
        old, new = Value("file", ("old",)), Value("file", ("new",))
        a, b, c = (ObligationRef(f"r.{n}", f"s{n}") for n in "abc")
        found = set()
        for tag in range(6):
            write = group("w", 0, 1, produce(p, new), produce(q, new))
            reads = [
                sees(f"x{tag}", 5, 6, p, old, a, b),
                sees(f"y{tag}", 5, 6, p, old, c),
                sees(f"z{tag}", 5, 6, q, old, b),
            ]
            solution = solver.solve(
                make_problem([write], reads, initial={p: old, q: old})
            )
            proofs = {proof.id: proof for proof in solution.proofs}
            found.add(
                tuple(
                    (
                        ref.rule_id,
                        tuple(
                            sorted(
                                "".join(n[0] for n in proofs[i].observations)
                                for i in solution.outcomes[ref].proofs
                            )
                        ),
                    )
                    for ref in (a, b, c)
                )
            )
        self.assertEqual(
            {(("r.a", ("x",)), ("r.b", ("x", "z")), ("r.c", ("y",)))}, found
        )

    def test_input_order_and_independent_operations_change_nothing(self) -> None:
        for seed in range(500):
            problem = random_problem(seed, max_nodes=6, keys=("a", "b"))
            rng = random.Random(seed)
            groups, observations, constraints = (
                list(problem.groups),
                list(problem.observations),
                list(problem.constraints),
            )
            for items in (groups, observations, constraints):
                rng.shuffle(items)
            permuted = Problem(
                tuple(groups), tuple(observations), tuple(constraints), problem.initial
            )
            base = solver.solve(problem)
            self.assertEqual(base, solver.solve(permuted), seed)
            z = cell("z")
            initial = problem.initial

            def extended(c: Cell, z: Cell = z, initial: Any = initial) -> Value:
                return ABSENT if c == z else initial(c)

            harmless = Problem(
                (
                    *problem.groups,
                    group("zz-independent", 0, 9, produce(z, V1), acked=seed % 2 == 0),
                ),
                problem.observations,
                problem.constraints,
                extended,
            )
            other = solver.solve(harmless)
            self.assertEqual(base.outcomes, other.outcomes, seed)
            self.assertEqual(base.proofs, other.proofs, seed)


class SparseScaleTest(unittest.TestCase):
    def test_work_does_not_grow_with_untouched_cells(self) -> None:
        baseline = None
        for size in (10_000, 100_000, 1_000_000):
            for eager in (False, True):
                pinned = _CountingPinned(size)
                solution = solver.solve(_sparse_problem(pinned), eager=eager)
                self.assertTrue(
                    all(o.status == SATISFIED for o in solution.outcomes.values())
                )
                # Queried keys follow the domain: the 1M domain reads keys
                # far beyond 10,000 (cold keys sit at 90% when eager).
                self.assertGreater(pinned.highest, size * (0.85 if eager else 0.45))
                counters = (pinned.queries, solution.phases, solution.components)
                if baseline is None or baseline[0] != eager:
                    baseline = (eager, counters)
                self.assertEqual(baseline[1], counters, (size, eager))
        lazy = _CountingPinned(1_000_000)
        solver.solve(_sparse_problem(lazy))
        self.assertEqual(
            70, lazy.queries
        )  # 20 hot + 50 listed; 100 cold cells never queried
        self.assertLess(lazy.highest, 900_000)
        beyond = _CountingPinned(10_000)
        problem = _sparse_problem(beyond)
        far = Cell("primary", "path", "f00010000")
        with self.assertRaises(UnknownInitial):
            problem.initial(far)


class DeterminismTest(unittest.TestCase):
    def test_results_and_counters_are_identical_across_hash_seeds(self) -> None:
        code = (
            "from eden.integration.correctness.exploratory.core.test import solver_invariance_test\n"
            "print(solver_invariance_test._payload())\n"
        )
        outputs = [run_in_fresh_interpreter(code, seed) for seed in ("0", "12345")]
        self.assertEqual(outputs[0], outputs[1])
        self.assertEqual(_payload() + "\n", outputs[0])
        self.assertGreater(len(outputs[0]), 10_000)


class BudgetMonotonicityTest(unittest.TestCase):
    """More budget only turns not-evaluated into satisfied or violated."""

    def _check(self, problem: Problem, tally: dict[str, int], label: Any) -> None:
        full = {r: o.status for r, o in solver.solve(problem).outcomes.items()}
        for ladder in LADDERS:
            runs = [
                {
                    r: o.status
                    for r, o in solver.solve(problem, budgets=b).outcomes.items()
                }
                for b in ladder
            ] + [full]
            for i, smaller in enumerate(runs[:-1]):
                tally["runs"] += 1
                for ref, status in smaller.items():
                    if status == NOT_EVALUATED:
                        tally["not-evaluated"] += 1
                        continue
                    tally["decided"] += 1
                    for larger in runs[i + 1 :]:
                        self.assertEqual(status, larger[ref], (label, ladder[i], ref))

    def test_checkout_shaped_problems(self) -> None:
        tally = {"runs": 0, "decided": 0, "not-evaluated": 0, "unsat": 0}
        for seed in range(1_100_000, 1_100_300):
            problem = checkout_problem(seed, max_nodes=12)
            tally["unsat"] += bool(solver.solve(problem).proofs)
            self._check(problem, tally, seed)
        self.assertGreater(tally["unsat"], 80, tally)
        self.assertGreater(tally["not-evaluated"], 200, tally)

    def test_lost_write_and_hard_components(self) -> None:
        tally = {"runs": 0, "decided": 0, "not-evaluated": 0}
        self._check(lost_write_problem(("fuse",) * 4), tally, "lost")
        x = cell("x")
        refs = (ObligationRef("t.hard", "r1"), ObligationRef("t.hard", "r2"))
        groups, reads = hard_component("h", x, 6, refs)
        self._check(make_problem(groups, reads, initial={x: ABSENT}), tally, "hard")
        self.assertGreater(tally["not-evaluated"], 0)
