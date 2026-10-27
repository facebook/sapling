# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Engine edges no corpus history exercises alone.

An input whose replay reads a cell with no initial value (through a guard,
a transfer source, or transitively) is not evaluated; transfers chained in
either order reach a settled reader; and the checker's projection of
engine outcomes keeps a violation over a collector's not-evaluated status
and over an excluded constraint, and decides nothing from a witness that
fails replay.
"""

from __future__ import annotations

import dataclasses
import unittest

from eden.integration.correctness.exploratory.core import checker, solver
from eden.integration.correctness.exploratory.core.effects import (
    Cell,
    eq,
    Expect,
    Guard,
    ObligationRef,
    Value,
)
from eden.integration.correctness.exploratory.core.property import (
    ObligationStatus,
    SOLVER_WITNESS_REJECTED,
    Subject,
    SubjectKind,
)
from eden.integration.correctness.exploratory.core.semantics import Problem
from eden.integration.correctness.exploratory.core.test.problems import (
    cell,
    group,
    make_problem,
    NOT_EVALUATED,
    produce,
    SATISFIED,
    sees,
    transfer,
    V1,
    V2,
    VIOLATED,
)

REF = ObligationRef("t.rule", "s")
A, B, C, D = cell("a"), cell("b"), cell("c"), cell("d")


def _value(name: str) -> Value:
    return Value("file", (name,))


def _when(target: Cell, name: str) -> Guard:
    """A guard that holds while `target` holds the file `name`."""
    return Guard((target,), Expect(((eq(_value(name)),),)))


def _outcome(problem: Problem) -> tuple[ObligationStatus, str | None]:
    outcome = solver.solve(problem).outcomes[REF]
    return outcome.status, outcome.reason


class UnknownInitialTest(unittest.TestCase):
    """`D` has no initial value, so every input that reads it is unsupported."""

    def test_guard_read_of_an_unknown_cell(self) -> None:
        problem = make_problem(
            [group("g", 0, 1, produce(A, _value("new"), _when(D, "on")))],
            [sees("o", 2, 3, A, _value("new"), REF)],
            initial={A: _value("old")},
        )
        self.assertEqual((NOT_EVALUATED, solver.UNKNOWN_INITIAL), _outcome(problem))

    def test_transfer_from_an_unknown_cell(self) -> None:
        problem = make_problem(
            [group("g", 0, 1, transfer(D, A))],
            [sees("o", 2, 3, A, _value("x"), REF)],
            initial={A: _value("old")},
        )
        self.assertEqual((NOT_EVALUATED, solver.UNKNOWN_INITIAL), _outcome(problem))

    def test_unknown_value_carried_into_a_guard(self) -> None:
        problem = make_problem(
            [
                group("g1", 0, 1, transfer(D, B)),
                group("g2", 2, 3, produce(C, _value("set"), _when(B, "k"))),
            ],
            [sees("o", 4, 5, C, _value("set"), REF)],
            initial={B: _value("b0"), C: _value("c0")},
        )
        self.assertEqual((NOT_EVALUATED, solver.UNKNOWN_INITIAL), _outcome(problem))


class ChainedTransferTest(unittest.TestCase):
    def test_chained_transfers_reach_a_settled_reader_in_either_order(self) -> None:
        for chain in ((A, B, C, D), (D, C, B, A)):
            first, last = chain[0], chain[-1]
            groups = [group("g0", 0, 0, produce(last, _value("zero")))]
            for index, (source, target) in enumerate(zip(chain, chain[1:])):
                lo, hi = 2 * index + 1, 2 * index + 2
                groups.append(group(f"g{index + 1}", lo, hi, transfer(source, target)))
            problem = make_problem(
                groups,
                [sees("o", 7, 8, last, _value("aa"), REF)],
                initial={
                    first: _value("aa"),
                    **{other: _value("zz") for other in chain[1:]},
                },
            )
            with self.subTest(chain=[item.key for item in chain]):
                self.assertEqual(SATISFIED, _outcome(problem)[0])


class ProjectionTest(unittest.TestCase):
    def test_engine_violation_wins_over_a_collector_not_evaluated(self) -> None:
        pending = checker._Pending(
            Subject(SubjectKind.PATH, "path:x"),
            (),
            status=NOT_EVALUATED,
            reason_id="x.y",
            inputs=1,
        )
        outcome = solver.Outcome(VIOLATED, "component:c", ("proof:p",))
        result = pending.resolve("r", "s", outcome)
        assert result is not None
        self.assertEqual(VIOLATED, result.status)

    def test_excluded_constraint_keeps_a_violation(self) -> None:
        other = ObligationRef("t.other", "s")
        solution = solver.Solution(
            {
                REF: solver.Outcome(VIOLATED, "component:c", ("proof:p",)),
                other: solver.Outcome(SATISFIED, "component:c"),
            },
            (),
            {},
            {},
        )
        outcomes = checker._with_excluded(solution, frozenset({REF, other}))
        self.assertEqual(VIOLATED, outcomes[REF].status)

    def test_a_witness_that_fails_replay_leaves_every_outcome_not_evaluated(
        self,
    ) -> None:
        problem = make_problem(
            [group("w", 0, 1, produce(A, V1))],
            [sees("o", 2, 3, A, V1, REF)],
            initial={A: V2},
        )
        solution = solver.solve(problem)
        assert solution.witness is not None
        self.assertIs(solution, checker._certified(problem, solution))
        reversed_order = dataclasses.replace(
            solution,
            witness=dataclasses.replace(
                solution.witness, order=solution.witness.order[::-1]
            ),
        )
        outcome = checker._certified(problem, reversed_order).outcomes[REF]
        self.assertEqual(
            (NOT_EVALUATED, SOLVER_WITNESS_REJECTED), (outcome.status, outcome.reason)
        )


if __name__ == "__main__":
    unittest.main()
