# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Attribution of engine proofs from alternative witnesses.

A minimal core holds one observation per contradiction, so corroboration is
computed by re-solving the core with each same-cells, same-expectation
observation in place of a core observation. These tests reproduce the old
agreement semantics: sl status and sl diff (one Sapling serving path)
agreeing against the contract effects stay `undetermined`; FUSE and Thrift
(two serving paths) agreeing are `oracle-suspect`; a lone observer, or
repeats of one channel, are `product`; reference-only proofs are
`reference`. Observations a collector files under one obligation (sl
status and its `sl diff`) are two observers of one claim. Each proof has
its own attribution budget; an undecided or unrun re-solve leaves a proof
`undetermined` rather than `product`. An obligation several proofs violate
takes the most severe of their attributions.
"""

from __future__ import annotations

import unittest
from unittest import mock

from eden.integration.correctness.exploratory.core import checker, effects, solver
from eden.integration.correctness.exploratory.core.effects import (
    eq,
    Expect,
    ObligationRef,
    Observation,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.property import Attribution
from eden.integration.correctness.exploratory.core.semantics import (
    mapping_initial,
    Problem,
)
from eden.integration.correctness.exploratory.core.test.problems import (
    cell,
    group,
    produce,
)

OLD, NEW = Value("file", ("old",)), Value("file", ("new",))
SERVING = {
    "fuse": "edenfs-fuse",
    "thrift": "edenfs-thrift",
    "sl-status": "sapling",
    "sl-diff": "sapling",
}


def _attributions(
    problem: Problem, solution: solver.Solution
) -> tuple[dict[ObligationRef, Attribution], dict[str, int]]:
    """Each violated obligation's attribution (most severe over its proofs), and counters."""
    found, counters = checker._engine_attributions(problem, solution, SERVING)
    return {ref: attribution for ref, (attribution, _p) in found.items()}, counters


def _stale(
    oid: str, channel: str, lo: float, side: str = effects.PRIMARY
) -> Observation:
    """An observation of the pre-write value after an acked write completed."""
    return Observation(
        oid,
        "a-" + oid,
        (cell("p", side),),
        Expect(((eq(OLD),),)),
        Window(lo, lo + 1),
        channel,
        (ObligationRef("t.stale", oid),),
    )


def _attribute(*observations: Observation, side: str = effects.PRIMARY):
    problem = Problem(
        (group("w", 0, 1, produce(cell("p", side), NEW)),),
        observations,
        (),
        mapping_initial({cell("p", side): OLD}),
    )
    solution = solver.solve(problem)
    found, counters = _attributions(problem, solution)
    return solution, found, counters


class CorroborationTest(unittest.TestCase):
    def test_a_lone_observer_is_product(self) -> None:
        _solution, found, counters = _attribute(_stale("r", "fuse", 2))
        self.assertEqual({ObligationRef("t.stale", "r"): Attribution.PRODUCT}, found)
        self.assertNotIn("attribution_trials", counters)

    def test_sl_status_and_sl_diff_agreeing_are_undetermined(self) -> None:
        """Two outputs of one Sapling serving path agree by construction."""
        solution, found, counters = _attribute(
            _stale("s", "sl-status", 2), _stale("d", "sl-diff", 4)
        )
        # Each read alone contradicts the write: two one-observation cores,
        # each corroborated by the other read.
        self.assertEqual([1, 1], [len(p.observations) for p in solution.proofs])
        self.assertEqual({Attribution.UNDETERMINED}, set(found.values()))
        self.assertEqual(2, counters["attribution_trials"])

    def test_fuse_and_thrift_agreeing_are_oracle_suspect(self) -> None:
        """Observers on two serving paths agreeing implicate the contract effects."""
        _solution, found, _counters = _attribute(
            _stale("f", "fuse", 2), _stale("t", "thrift", 4)
        )
        self.assertEqual({Attribution.ORACLE_SUSPECT}, set(found.values()))

    def test_repeats_of_one_channel_stay_product(self) -> None:
        _solution, found, counters = _attribute(
            _stale("r1", "fuse", 2), _stale("r2", "fuse", 4)
        )
        self.assertEqual({Attribution.PRODUCT}, set(found.values()))
        self.assertNotIn("attribution_trials", counters)

    def test_a_candidate_that_does_not_keep_the_core_unsatisfiable_is_no_corroborator(
        self,
    ) -> None:
        """A Thrift read inside the write's window can see the old value legally."""
        early = _stale("t", "thrift", 0.5)
        _solution, found, counters = _attribute(_stale("f", "fuse", 2), early)
        self.assertEqual(
            {ObligationRef("t.stale", "f"): Attribution.PRODUCT},
            found,
        )
        self.assertEqual(1, counters["attribution_trials"])

    def test_undecided_trials_leave_the_proof_undetermined(self) -> None:
        """An undecided re-solve proves no corroboration, and no lone observer either."""
        with mock.patch.object(
            solver, "satisfiable", return_value=("unknown", 5)
        ) as decide:
            _solution, found, counters = _attribute(
                _stale("f", "fuse", 2), _stale("t", "thrift", 4)
            )
        self.assertTrue(decide.called)
        self.assertEqual({Attribution.UNDETERMINED}, set(found.values()))
        self.assertEqual(2, counters["attribution_unknown"])

    def test_an_exhausted_budget_leaves_the_proof_undetermined(self) -> None:
        with mock.patch.object(checker, "ATTRIBUTION_WORK", 0):
            _solution, found, counters = _attribute(
                _stale("f", "fuse", 2), _stale("t", "thrift", 4)
            )
        self.assertEqual({Attribution.UNDETERMINED}, set(found.values()))
        self.assertEqual(2, counters["attribution_exhausted"])

    def test_a_proof_without_candidates_needs_no_budget(self) -> None:
        with mock.patch.object(checker, "ATTRIBUTION_WORK", 0):
            _solution, found, counters = _attribute(_stale("r", "fuse", 2))
        self.assertEqual({Attribution.PRODUCT}, set(found.values()))
        self.assertNotIn("attribution_exhausted", counters)

    def test_each_proof_has_its_own_budget(self) -> None:
        """Unrelated proofs cannot use up another proof's attribution trials."""
        observations = []
        groups = []
        for key in ("p", "q"):
            groups.append(group("w" + key, 0, 1, produce(cell(key), NEW)))
            for channel, lo in (("fuse", 2), ("thrift", 4)):
                oid = f"{key}-{channel}"
                observations.append(
                    Observation(
                        oid,
                        "a-" + oid,
                        (cell(key),),
                        Expect(((eq(OLD),),)),
                        Window(lo, lo + 1),
                        channel,
                        (ObligationRef("t.stale", oid),),
                    )
                )
        problem = Problem(
            tuple(groups),
            tuple(observations),
            (),
            mapping_initial({cell("p"): OLD, cell("q"): OLD}),
        )
        solution = solver.solve(problem)
        self.assertEqual(4, len(solution.proofs))
        with (
            mock.patch.object(checker, "ATTRIBUTION_WORK", 10),
            mock.patch.object(solver, "satisfiable", return_value=("unsat", 10)),
        ):
            found, counters = _attributions(problem, solution)
        self.assertEqual({Attribution.ORACLE_SUSPECT}, set(found.values()))
        self.assertEqual(4, len(found))
        self.assertEqual(4, counters["attribution_trials"])
        self.assertNotIn("attribution_exhausted", counters)


class CoObserverTest(unittest.TestCase):
    """Observations supporting one obligation observe one claim from their channels."""

    def _claim(self, oid: str, channel: str, value: Value, lo: float) -> Observation:
        return Observation(
            oid,
            "s",
            (cell("p"),),
            Expect(((eq(value),),)),
            Window(lo, lo + 1),
            channel,
            (ObligationRef("t.claim", "s"),),
        )

    def _attribute(self, *observations: Observation):
        problem = Problem(
            (group("w", 0, 1, produce(cell("p"), NEW)),),
            observations,
            (),
            mapping_initial({cell("p"): OLD}),
        )
        solution = solver.solve(problem)
        found, _counters = _attributions(problem, solution)
        return solution, found

    def test_two_outputs_of_one_serving_path_are_undetermined(self) -> None:
        """Only the sl-diff observation contradicts the write; sl status agrees with it."""
        solution, found = self._attribute(
            self._claim("o:status", "sl-status", NEW, 2),
            self._claim("o:diff", "sl-diff", OLD, 4),
        )
        (proof,) = solution.proofs
        self.assertEqual(("o:diff",), proof.observations)
        self.assertEqual(
            {ObligationRef("t.claim", "s"): Attribution.UNDETERMINED}, found
        )

    def test_outputs_of_two_serving_paths_are_oracle_suspect(self) -> None:
        _solution, found = self._attribute(
            self._claim("o:status", "sl-status", NEW, 2),
            self._claim("o:read", "fuse", OLD, 4),
        )
        self.assertEqual(
            {ObligationRef("t.claim", "s"): Attribution.ORACLE_SUSPECT}, found
        )

    def test_one_channel_alone_stays_product(self) -> None:
        _solution, found = self._attribute(self._claim("o:diff", "sl-diff", OLD, 4))
        self.assertEqual({ObligationRef("t.claim", "s"): Attribution.PRODUCT}, found)

    def test_reference_only_proofs_are_reference(self) -> None:
        _solution, found, _counters = _attribute(
            _stale("r", "fuse", 2, effects.REFERENCE), side=effects.REFERENCE
        )
        self.assertEqual({Attribution.REFERENCE}, set(found.values()))

    def test_an_undeclared_channel_is_its_own_group(self) -> None:
        _solution, found, _counters = _attribute(
            _stale("f", "fuse", 2), _stale("n", "nfs", 4)
        )
        self.assertEqual({Attribution.ORACLE_SUSPECT}, set(found.values()))


class MostSevereTest(unittest.TestCase):
    def test_an_obligation_takes_its_most_severe_proof_whatever_the_ids(self) -> None:
        """`r.b` has a corroborated (oracle-suspect) and a lone (product) proof."""
        a, b, c = (ObligationRef(f"r.{n}", f"s{n}") for n in "abc")
        for tag in range(6):
            reads = []
            for name, key, channel, refs in (
                ("x", "p", "fuse", (a, b)),
                ("y", "p", "thrift", (c,)),
                ("z", "q", "fuse", (b,)),
            ):
                oid = f"{name}{tag}"
                reads.append(
                    Observation(
                        oid,
                        "a-" + oid,
                        (cell(key),),
                        Expect(((eq(OLD),),)),
                        Window(5, 6),
                        channel,
                        refs,
                    )
                )
            problem = Problem(
                (group("w", 0, 1, produce(cell("p"), NEW), produce(cell("q"), NEW)),),
                tuple(reads),
                (),
                mapping_initial({cell("p"): OLD, cell("q"): OLD}),
            )
            solution = solver.solve(problem)
            found, _counters = _attributions(problem, solution)
            self.assertEqual(
                {
                    a: Attribution.ORACLE_SUSPECT,
                    b: Attribution.PRODUCT,
                    c: Attribution.ORACLE_SUSPECT,
                },
                found,
                tag,
            )
