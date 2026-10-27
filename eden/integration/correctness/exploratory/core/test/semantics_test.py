# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import math
import unittest

from eden.integration.correctness.exploratory.core import exhaustive
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    Agree,
    ANY,
    Before,
    constraint_id,
    eq,
    Exclusive,
    Expect,
    Guard,
    MustAppearIn,
    ObligationRef,
    Requirement,
    Supersedes,
    UnknownInitial,
    Value,
    VisibleAfterAck,
)
from eden.integration.correctness.exploratory.core.semantics import (
    Assignment,
    explains,
    mapping_initial,
    precedence,
    Problem,
    requirement_id,
)
from eden.integration.correctness.exploratory.core.test.problems import (
    cell,
    group,
    observe,
    produce,
    transfer,
)

A, B = cell("a"), cell("b")
V1, V2 = Value("file", ("v1",)), Value("file", ("v2",))
REF = ObligationRef("t.rule", "s")


def _problem(groups=(), observations=(), constraints=(), initial=None) -> Problem:
    return Problem(
        tuple(groups),
        tuple(observations),
        tuple(constraints),
        mapping_initial(initial if initial is not None else {A: V1, B: ABSENT}),
    )


def _sees(oid: str, lo: float, hi: float, target, value) -> object:
    return observe(oid, lo, hi, (target,), ((eq(value),),))


def _explains(problem: Problem, happened, *order: str) -> bool:
    return explains(problem, Assignment(frozenset(happened), tuple(order)))


class ExplainsTest(unittest.TestCase):
    def test_visible_value_is_the_last_happened_effect_before_the_observation(
        self,
    ) -> None:
        problem = _problem(
            [group("w", 0, 2, produce(A, V2))], [_sees("r", 1, 3, A, V2)]
        )
        self.assertTrue(_explains(problem, {"w"}, "w", "r"))
        self.assertFalse(_explains(problem, {"w"}, "r", "w"))

    def test_real_time_forces_order_and_ties_are_orderable_either_way(self) -> None:
        # w ends (2) before r starts (3): r can never see the initial value.
        late = _problem([group("w", 0, 2, produce(A, V2))], [_sees("r", 3, 4, A, V1)])
        self.assertFalse(_explains(late, {"w"}, "r", "w"))
        self.assertFalse(_explains(late, {"w"}, "w", "r"))
        # Windows touching at one instant may be ordered either way.
        tie = _problem([group("w", 0, 2, produce(A, V2))], [_sees("r", 2, 4, A, V1)])
        self.assertTrue(_explains(tie, {"w"}, "r", "w"))

    def test_infinite_window_never_forces_a_later_node(self) -> None:
        problem = _problem(
            [group("w", 0, math.inf, produce(A, V2), acked=False)],
            [_sees("r1", 5, 6, A, V1), _sees("r2", 7, 8, A, V2)],
        )
        self.assertTrue(_explains(problem, {"w"}, "r1", "w", "r2"))
        self.assertTrue(_explains(problem, set(), "r1") is False)
        self.assertEqual(
            Assignment(frozenset({"w"}), ("r1", "w", "r2")),
            exhaustive.satisfying(problem),
        )

    def test_acked_groups_happen_and_shared_outcome_variables_happen_together(
        self,
    ) -> None:
        problem = _problem(
            [
                group("w1", 0, 1, produce(A, V2), acked=False, var="x"),
                group("w2", 0, 1, produce(B, V2), acked=True, var="x"),
            ]
        )
        self.assertFalse(_explains(problem, set()))
        self.assertFalse(_explains(problem, {"x"}, "w2"))
        self.assertTrue(_explains(problem, {"x"}, "w1", "w2"))

    def test_before_cycle_contradicts_even_with_equal_points(self) -> None:
        g1, g2 = group("c", 0, 4, produce(A, V2)), group("w", 0, 4, produce(A, V1))
        cycle = [Before("c", "w", REF), Before("w", "c", REF)]
        self.assertFalse(_explains(_problem([g1, g2], [], cycle), {"c", "w"}, "c", "w"))
        self.assertFalse(_explains(_problem([g1, g2], [], cycle), {"c", "w"}, "w", "c"))
        self.assertIsNone(exhaustive.satisfying(_problem([g1, g2], [], cycle)))

    def test_precedence_aliases(self) -> None:
        self.assertEqual(Before("o", "g", REF), precedence(MustAppearIn("g", "o", REF)))
        self.assertEqual(
            Before("g", "o", REF), precedence(VisibleAfterAck("g", "o", REF))
        )
        self.assertEqual(Before("a", "b", REF), precedence(Supersedes("a", "b", REF)))
        self.assertIsNone(precedence(Agree("a", "b", "same", REF)))
        self.assertIsNone(precedence(Exclusive(("x", "y"), REF)))

    def test_precedence_with_a_dropped_endpoint_is_vacuous(self) -> None:
        problem = _problem(
            [group("w", 3, 4, produce(A, V2), acked=False)],
            [_sees("r", 0, 1, A, V1)],
            [Before("w", "r", REF)],
        )
        self.assertTrue(_explains(problem, set(), "r"))
        self.assertFalse(_explains(problem, {"w"}, "r", "w"))

    def test_exclusive_allows_at_most_one_variable(self) -> None:
        groups = [
            group("c1", 0, 4, produce(A, V2), acked=False),
            group("c2", 0, 4, produce(A, ABSENT), acked=False),
        ]
        problem = _problem(groups, [], [Exclusive(("c1", "c2"), REF)])
        self.assertTrue(_explains(problem, {"c1"}, "c1"))
        self.assertFalse(_explains(problem, {"c1", "c2"}, "c1", "c2"))

    def test_guards_and_requirements_read_pre_group_values(self) -> None:
        # The guard reads A before the group writes it: A=V1, so the guarded
        # write to B applies even though the same group sets A to V2.
        guard = Guard((A,), Expect(((eq(V1),),)))
        requirement = Requirement((A,), Expect(((eq(V1),),)), REF)
        g = group(
            "g", 0, 1, produce(A, V2), produce(B, V2, guard), requires=(requirement,)
        )
        problem = _problem([g], [observe("r", 2, 3, (A, B), ((eq(V2), eq(V2)),))])
        self.assertTrue(_explains(problem, {"g"}, "g", "r"))
        blocked = _problem(
            [group("g0", 0, 1, produce(A, ABSENT)), g],
            [observe("r", 2, 3, (B,), ((eq(ABSENT),),))],
        )
        self.assertFalse(_explains(blocked, {"g0", "g"}, "g0", "g", "r"))

    def test_false_guard_makes_only_its_effect_a_no_op(self) -> None:
        guard = Guard((A,), Expect(((eq(V2),),)))
        g = group("g", 0, 1, produce(A, ABSENT, guard), produce(B, V1))
        problem = _problem([g], [observe("r", 2, 3, (A, B), ((eq(V1), eq(V1)),))])
        self.assertTrue(_explains(problem, {"g"}, "g", "r"))

    def test_transfer_moves_the_pre_group_value_atomically(self) -> None:
        problem = _problem(
            [group("mv", 0, 1, transfer(A, B))],
            [observe("r", 2, 3, (A, B), ((eq(ABSENT), eq(V1)),))],
        )
        self.assertTrue(_explains(problem, {"mv"}, "mv", "r"))

    def test_multi_cell_rows_are_a_disjunction_of_conjunctions(self) -> None:
        rows = ((eq(V1), eq(ABSENT)), (eq(ABSENT), eq(V1)))
        problem = _problem([], [observe("r", 0, 1, (A, B), rows)])
        self.assertTrue(_explains(problem, set(), "r"))
        mixed = _problem(
            [], [observe("r", 0, 1, (A, B), ((eq(V1), eq(V1)), (ANY, eq(V2))))]
        )
        self.assertFalse(_explains(mixed, set(), "r"))

    def test_order_must_list_exactly_the_happened_nodes(self) -> None:
        problem = _problem(
            [group("w", 0, 1, produce(A, V2), acked=False)], [_sees("r", 2, 3, A, V1)]
        )
        self.assertFalse(_explains(problem, set(), "r", "w"))
        self.assertFalse(_explains(problem, {"w"}, "r"))
        self.assertFalse(_explains(problem, {"nope"}, "r"))

    def test_agree_never_constrains_an_explanation(self) -> None:
        problem = _problem(
            [],
            [_sees("o1", 0, 1, A, V1), _sees("o2", 0, 1, B, ABSENT)],
            [Agree("o1", "o2", "same", REF)],
        )
        self.assertTrue(_explains(problem, set(), "o1", "o2"))
        self.assertEqual(frozenset({REF}), exhaustive.agree_obligations(problem))

    def test_unknown_initial_value_raises(self) -> None:
        problem = _problem([], [_sees("r", 0, 1, A, V1)], initial={})
        with self.assertRaises(UnknownInitial):
            _explains(problem, set(), "r")


class ProblemTest(unittest.TestCase):
    def test_inputs_and_restrict(self) -> None:
        requirement = Requirement((A,), Expect(((ANY,),)), REF)
        g = group("g", 0, 1, produce(A, V2), requires=(requirement,))
        before = Before("g", "r", REF)
        agree = Agree("r", "r2", "same", REF)
        problem = _problem(
            [g],
            [_sees("r", 2, 3, A, V2), _sees("r2", 2, 3, B, ABSENT)],
            [before, agree],
        )
        ids = [item.id for item in problem.inputs()]
        self.assertEqual(
            sorted(["r", "r2", constraint_id(before), requirement_id("g", 0)]), ids
        )
        smaller = problem.restrict(["r2"])
        self.assertEqual(["r2"], [item.id for item in smaller.inputs()])
        self.assertEqual((), smaller.groups[0].requires)
        self.assertEqual((), smaller.constraints)

    def test_validation(self) -> None:
        with self.assertRaisesRegex(ValueError, "repeats a node id"):
            _problem([group("x", 0, 1, produce(A, V1))], [_sees("x", 0, 1, A, V1)])
        requirement = Requirement((A,), Expect(((ANY,),)), REF)
        with self.assertRaisesRegex(ValueError, "repeats an input id"):
            _problem(
                [group("g", 0, 1, produce(A, V2), requires=(requirement,))],
                [_sees(requirement_id("g", 0), 2, 3, A, V2)],
            )
        with self.assertRaisesRegex(ValueError, "unknown node"):
            _problem([], [], [Before("p", "q", REF)])
        with self.assertRaisesRegex(ValueError, "not a group"):
            _problem(
                [],
                [_sees("o", 0, 1, A, V1), _sees("p", 0, 1, A, V1)],
                [Supersedes("o", "p", REF)],
            )
        with self.assertRaisesRegex(ValueError, "unknown variables"):
            _problem(
                [group("g", 0, 1, produce(A, V1))], [], [Exclusive(("g", "h"), REF)]
            )
        with self.assertRaisesRegex(ValueError, "repeats a constraint"):
            _problem(
                [group("g", 0, 1, produce(A, V1)), group("h", 0, 1, produce(A, V1))],
                [],
                [Before("g", "h", REF), Before("g", "h", REF)],
            )
