# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Optimized engine = exhaustive evaluator, in every engine mode, on generated problems."""

from __future__ import annotations

import collections
import itertools
import unittest
from collections.abc import Mapping

from eden.integration.correctness.exploratory.core import exhaustive, solver
from eden.integration.correctness.exploratory.core.effects import (
    Cell,
    ProofCertificate,
    UnknownInitial,
    Value,
)
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.semantics import explains, Problem
from eden.integration.correctness.exploratory.core.test.problems import (
    checkout_problem,
    counted_problem,
    random_problem,
    shared_variable_problem,
    VALUES,
)

# mode -> (eager, monolithic)
MODES = {
    "lazy-factored": (False, False),
    "eager-factored": (True, False),
    "lazy-monolithic": (False, True),
    "eager-monolithic": (True, True),
}
# (seed offset, count, max nodes, cell keys): hundreds of problems with
# transfers, guards, requirements, Exclusive, indeterminate groups, ties,
# infinite windows, and multi-cell observations. This size still catches
# every planted solver bug the 10x larger sweep caught.
BATCHES = (
    (0, 150, 6, "abc"),
    (100_000, 150, 8, "ab"),
    (200_000, 100, 7, "a"),
)
# Many cells coupled only through shared indeterminate outcome variables.
SHARED_VARIABLE_SEEDS = range(900_000, 900_150)
# Checkout-shaped problems (`checkout_problem`), also solved under these
# smaller budgets: whatever a smaller budget decides must equal the default.
CHECKOUT_SEEDS = range(1_000_000, 1_001_000)
# Checkout-shaped and random problems plus a count observation over
# guarded markers (`counted_problem`).
COUNTED_SEEDS = range(2_000_000, 2_001_000)
LIMITED = (
    solver.Budgets(explanation_work=32),
    solver.Budgets(explanation_work=256),
    solver.Budgets(decision_work=256),
)


def _summary(solution: solver.Solution) -> tuple[object, ...]:
    """Mode-independent results: statuses, reasons, proof cores, and proof groups."""
    proofs = {
        p.id: (tuple(sorted(p.observations + p.constraints)), p.groups)
        for p in solution.proofs
    }
    outcomes = tuple(
        sorted(
            (
                tuple(ref.document()),
                str(o.status),
                o.reason,
                tuple(sorted(proofs[proof][0] for proof in o.proofs)),
            )
            for ref, o in solution.outcomes.items()
        )
    )
    return outcomes, tuple(sorted(proofs.values()))


def _expected(problem: Problem) -> tuple[set[tuple[str, ...]], list[tuple[str, ...]]]:
    results = exhaustive.deletion_results(problem)
    violated = {tuple(ref.document()) for ref in results.violated}
    return violated, sorted(core.inputs for core in results.cores)


def _reconstructs(problem: Problem, proof: ProofCertificate) -> bool:
    """The proof's inputs plus its groups alone are unsatisfiable."""
    keep = set(proof.observations) | set(proof.constraints)
    restricted = problem.restrict(keep)
    alone = Problem(
        tuple(g for g in restricted.groups if g.id in proof.groups),
        restricted.observations,
        tuple(
            c
            for c in restricted.constraints
            if all(n in proof.groups or n in proof.observations for n in c.nodes())
        ),
        problem.initial,
    )
    return not exhaustive.satisfiable(alone)


def _problems() -> list[tuple[str, int, Problem]]:
    found = [
        (
            "random",
            seed,
            random_problem(
                seed, max_nodes=max_nodes, keys=tuple(keys), legal=seed % 3 == 0
            ),
        )
        for offset, count, max_nodes, keys in BATCHES
        for seed in range(offset, offset + count)
    ]
    found += [
        ("shared-variable", seed, shared_variable_problem(seed))
        for seed in SHARED_VARIABLE_SEEDS
    ]
    return found


class EngineCrossCheckTest(unittest.TestCase):
    def test_engine_matches_reference_in_every_mode(self) -> None:  # noqa: C901
        discrepancies: collections.Counter[str] = collections.Counter()
        tally: collections.Counter[str] = collections.Counter()
        examples: list[str] = []
        for family, seed, problem in _problems():
            tally["problems"] += 1
            tally[f"family:{family}"] += 1
            violated, cores = _expected(problem)
            decided = {ref for item in problem.inputs() for ref in item.obligations}
            tally["unsat" if cores else "sat"] += 1
            if family == "shared-variable" and cores:
                tally["shared-variable-unsat"] += 1
            tally["cores"] += len(cores)
            reference_witness = exhaustive.satisfying(problem)
            if reference_witness is not None and not explains(
                problem, reference_witness
            ):
                discrepancies["reference-witness"] += 1
            summaries = {}
            for mode, (eager, monolithic) in MODES.items():
                solution = solver.solve(problem, eager=eager, monolithic=monolithic)
                summaries[mode] = _summary(solution)
                got = {
                    tuple(ref.document())
                    for ref, o in solution.outcomes.items()
                    if o.status == ObligationStatus.VIOLATED
                }
                unevaluated = {
                    ref
                    for ref, o in solution.outcomes.items()
                    if o.status == ObligationStatus.NOT_EVALUATED
                }
                if set(solution.outcomes) != decided:
                    discrepancies[f"{mode}:outcome-set"] += 1
                proofs = sorted(
                    tuple(sorted(p.observations + p.constraints))
                    for p in solution.proofs
                )
                if got != violated or proofs != cores:
                    discrepancies[f"{mode}:violations"] += 1
                    examples.append(f"{mode} seed={seed}")
                if unevaluated:
                    discrepancies[f"{mode}:not-evaluated"] += 1
                    examples.append(f"{mode} seed={seed} ne")
                if solution.witness is not None:
                    tally["witnesses"] += 1
                    if not explains(problem, solution.witness):
                        discrepancies[f"{mode}:witness"] += 1
                        examples.append(f"{mode} seed={seed} witness")
                elif not cores:
                    discrepancies[f"{mode}:missing-witness"] += 1
                if mode == "lazy-factored":
                    for proof in solution.proofs:
                        tally["proofs-reconstructed"] += 1
                        if not _reconstructs(problem, proof):
                            discrepancies["proof-reconstruction"] += 1
                            examples.append(f"reconstruct seed={seed} {proof.id}")
            if len(set(summaries.values())) != 1:
                discrepancies["modes-differ"] += 1
                examples.append(f"modes seed={seed}")
        self.assertEqual({}, dict(discrepancies), examples[:10])
        self.assertGreater(tally["unsat"], 200, tally)
        self.assertGreater(tally["sat"], 250, tally)
        self.assertGreater(tally["shared-variable-unsat"], 40, tally)

    def test_every_proof_is_a_minimal_unsatisfiable_subset(self) -> None:
        checked = 0
        for seed in range(300_000, 300_600):
            problem = random_problem(seed, max_nodes=6, keys=("a", "b"))
            if len(problem.inputs()) > 7:
                continue
            muses = {core.inputs for core in exhaustive.minimal_unsat_subsets(problem)}
            solution = solver.solve(problem)
            for proof in solution.proofs:
                checked += 1
                self.assertIn(
                    tuple(sorted(proof.observations + proof.constraints)), muses, seed
                )
        self.assertGreater(checked, 100)


def _unknown_cells(problem: Problem) -> list[Cell]:
    cells = {c for o in problem.observations for c in o.cells}
    for g in problem.groups:
        for e in g.effects:
            cells.update(e.writes())
    missing = []
    for c in sorted(cells, key=Cell.document):
        try:
            problem.initial(c)
        except UnknownInitial:
            missing.append(c)
    return missing


# Completions are enumerated, not sampled: VALUES ** k of them for k unknown
# initial cells, so the checkout-shaped problems must keep k at most this.
_MAX_UNKNOWN_CELLS = 3


def _completed(problem: Problem, assignment: Mapping[Cell, Value]) -> Problem:
    initial = problem.initial

    def lookup(c: Cell) -> Value:
        return assignment[c] if c in assignment else initial(c)

    return Problem(problem.groups, problem.observations, problem.constraints, lookup)


class CheckoutShapedCrossCheckTest(unittest.TestCase):
    def test_engine_matches_reference_on_checkout_shaped_problems(self) -> None:
        """Multi-cell requirements, sides, open acked windows, restarts, unknown initials.

        Inputs the engine leaves to an unknown initial value are compared by
        solving the rest with the reference under every completion of the
        unknown initial cells, each cell independently (all must agree).
        Smaller budgets may only leave obligations not evaluated.
        """
        tally: collections.Counter[str] = collections.Counter()
        for seed in CHECKOUT_SEEDS:
            problem = checkout_problem(seed)
            full = solver.solve(problem)
            unknown = {
                ref
                for ref, o in full.outcomes.items()
                if o.status == ObligationStatus.NOT_EVALUATED
                and o.reason == solver.UNKNOWN_INITIAL
            }
            self.assertEqual(
                unknown,
                {
                    r
                    for r, o in full.outcomes.items()
                    if o.status == ObligationStatus.NOT_EVALUATED
                },
                seed,
            )
            keep = [i.id for i in problem.inputs() if not set(i.obligations) & unknown]
            missing = _unknown_cells(problem)
            self.assertLessEqual(len(missing), _MAX_UNKNOWN_CELLS, seed)
            completions = [
                _completed(problem, dict(zip(missing, values)))
                for values in itertools.product(VALUES, repeat=len(missing))
            ]
            tally["completions"] += len(completions)
            tally[f"unknown-cells:{len(missing)}"] += 1
            got = {
                r
                for r, o in full.outcomes.items()
                if o.status == ObligationStatus.VIOLATED
            }
            cores = sorted(
                tuple(sorted(p.observations + p.constraints)) for p in full.proofs
            )
            for completed in completions:
                expected = exhaustive.deletion_results(completed.restrict(keep))
                self.assertEqual(
                    (set(expected.violated), sorted(c.inputs for c in expected.cores)),
                    (got, cores),
                    seed,
                )
            for proof in full.proofs:
                self.assertTrue(_reconstructs(completions[0], proof), seed)
            tally["problems"] += 1
            tally["unsat"] += bool(full.proofs)
            tally["unknown-initial"] += bool(missing)
            tally["reference-side"] += any(
                g.side == "reference" for g in problem.groups
            )
            tally["multi-proof"] += any(
                len(o.proofs) > 1 for o in full.outcomes.values()
            )
            for budgets in LIMITED:
                limited = solver.solve(problem, budgets=budgets)
                for ref, o in limited.outcomes.items():
                    if o.status == ObligationStatus.NOT_EVALUATED:
                        tally["limited-not-evaluated"] += ref not in unknown
                        continue
                    self.assertEqual(
                        full.outcomes[ref].status, o.status, (seed, budgets)
                    )
                self.assertLessEqual(
                    {p.id for p in limited.proofs}, {p.id for p in full.proofs}, seed
                )
        self.assertGreater(tally["unsat"], 400, tally)
        self.assertGreater(tally["unknown-initial"], 200, tally)
        self.assertGreater(tally["reference-side"], 400, tally)
        self.assertGreater(tally["limited-not-evaluated"], 300, tally)


class CountedCrossCheckTest(unittest.TestCase):
    def test_engine_matches_reference_with_terminal_count_observations(self) -> None:
        """A count observation read after every marker writer.

        Every mode must equal the reference (statuses and cores), every
        witness must explain the problem, and smaller budgets may only
        leave obligations not evaluated.
        """
        tally: collections.Counter[str] = collections.Counter()
        for seed in COUNTED_SEEDS:
            bases = (
                random_problem(seed, max_nodes=8),
                checkout_problem(seed, unknown=False),
            )
            for base in bases:
                problem = counted_problem(seed, base)
                if problem is None or (
                    len(problem.groups) + len(problem.observations)
                    > exhaustive.MAX_NODES
                ):
                    continue
                violated, cores = _expected(problem)
                summaries = set()
                for mode, (eager, monolithic) in MODES.items():
                    solution = solver.solve(problem, eager=eager, monolithic=monolithic)
                    summaries.add(_summary(solution))
                    got = {
                        tuple(ref.document())
                        for ref, o in solution.outcomes.items()
                        if o.status == ObligationStatus.VIOLATED
                    }
                    proofs = sorted(
                        tuple(sorted(p.observations + p.constraints))
                        for p in solution.proofs
                    )
                    self.assertEqual((violated, cores), (got, proofs), (mode, seed))
                    if solution.witness is not None:
                        self.assertTrue(explains(problem, solution.witness), seed)
                    else:
                        self.assertTrue(cores, (mode, seed))
                self.assertEqual(1, len(summaries), seed)
                full = solver.solve(problem)
                tally["problems"] += 1
                tally["unsat"] += bool(cores)
                for budgets in LIMITED:
                    limited = solver.solve(problem, budgets=budgets)
                    for ref, o in limited.outcomes.items():
                        if o.status == ObligationStatus.NOT_EVALUATED:
                            tally["limited-not-evaluated"] += 1
                            continue
                        self.assertEqual(full.outcomes[ref].status, o.status, seed)
                    self.assertLessEqual(
                        {p.id for p in limited.proofs}, {p.id for p in full.proofs}
                    )
        self.assertGreater(tally["unsat"], 25, tally)
        self.assertGreater(tally["limited-not-evaluated"], 100, tally)
