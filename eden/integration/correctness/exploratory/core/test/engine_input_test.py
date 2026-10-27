# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""What collectors hand the engine and how its answers come back: the engine
decides observations and constraints, agree obligations still need a direct
check, an excluded constraint names its reason, direct violations survive
undecided engine input, the earliest and most severe result wins, derived
inputs and hook constraints are claimed by their obligations and checked
against the history, and a failing effect hook blocks only target properties.
The engine verdict is identical across hash seeds.
"""

from __future__ import annotations

import math
import unittest
from collections.abc import Callable
from typing import Any

from eden.integration.correctness.exploratory.core import effects
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    BLOCKED,
    check,
    CORE_ACTIVITY,
    decode_verdict,
    encode_verdict,
    NOT_EVALUATED,
    PROPERTY_ERROR,
    REJECT,
    Verdict,
)
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    EffectKind,
    eq,
    Expect,
    Status,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    ObligationStatus,
    SOLVER_INPUT_EXCLUDED,
    SOLVER_UNAVAILABLE,
    SOLVER_UNKNOWN_INITIAL,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.test import checker_testing as t
from eden.integration.correctness.exploratory.core.test.testing import (
    run_in_fresh_interpreter,
)


def _record_one(_input: CheckInput, sink: CollectorSink) -> None:
    sink.satisfied(sink.obligation("p.bad", "a", subject=t.PATH))


def _group_and_observation_share_g1(sink: effects.ContractSink) -> None:
    sink.group(t.group())
    sink.observe(t.observation(effects.ObligationRef("p.bad", "a"), "g1"))


def _engine_verdict_bytes() -> bytes:
    """Verdict bytes of a run whose engine proves one violation and satisfies the rest."""
    spec = t.spec([t.action("r", "read")])

    def contracts(
        _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: Any
    ) -> None:
        sink.group(t.group())
        sink.group(
            t.group(
                "g2",
                effects=(
                    effects.Effect(t.FILE_CELL, EffectKind.PRODUCE, ABSENT, None, "r"),
                ),
            )
        )

    def collect(_input: CheckInput, sink: CollectorSink) -> None:
        for index, value in enumerate((Value("file"), ABSENT, Value("dir"))):
            ref = sink.obligation("p.bad", f"o{index}", subject=t.PATH)
            sink.observe(
                t.observation(ref, f"o{index}", expect=Expect(((eq(value),),)))
            )
        cycle = sink.obligation("p.bad", "cycle", subject=t.PATH)
        sink.constrain(effects.Supersedes("g1", "g2", cycle))
        sink.constrain(effects.Supersedes("g2", "g1", cycle))

    target = t.target(
        t.prop("p", collect), contracts=contracts, pinned=t.pinned(ABSENT)
    )
    return encode_verdict(check(target, spec, t.READ_OK))


class EngineInputTest(unittest.TestCase):
    _SPEC = t.spec([t.action("r", "read")])

    def _run(self, collect: t.Collect, **target: Any) -> Verdict:
        return check(t.target(t.prop("p", collect), **target), self._SPEC, t.READ_OK)

    def test_engine_decides_observations_and_constraints(self) -> None:
        """The engine decides solver input; an unknown initial value never decides."""

        def observed(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("p.bad", "observed", subject=t.PATH)
            sink.observe(t.observation(ref))
            done = sink.obligation("p.bad", "satisfied-too", subject=t.PATH)
            sink.observe(t.observation(done, "o2"))
            sink.satisfied(done)
            constrained = sink.obligation("p.bad", "constrained", subject=t.PATH)
            sink.constrain(effects.Before("o1", "o2", constrained))

        decided = self._run(observed, pinned=t.pinned(ABSENT))
        self.assertEqual(
            [
                ("constrained", ObligationStatus.SATISFIED),
                ("observed", ObligationStatus.SATISFIED),
                ("satisfied-too", ObligationStatus.SATISFIED),
            ],
            [(o.scope, o.status) for o in t.result(decided, "p").obligations],
        )
        self.assertEqual(ACCEPT, decided.kind)
        verdict = self._run(observed)
        result = t.result(verdict, "p")
        self.assertEqual(
            [
                ("constrained", SOLVER_UNKNOWN_INITIAL),
                ("observed", SOLVER_UNKNOWN_INITIAL),
                ("satisfied-too", SOLVER_UNKNOWN_INITIAL),
            ],
            [(o.scope, o.not_evaluated_reason) for o in result.obligations],
        )
        self.assertEqual(
            (NOT_EVALUATED, BLOCKED, f"blocked/{SOLVER_UNKNOWN_INITIAL}"),
            (verdict.kind, verdict.not_evaluated_cause, verdict.reason),
        )
        construction = t.report(verdict).work.phases["construction"]
        self.assertEqual(
            (1, 0, 2),
            (
                construction["constraints"],
                construction["groups"],
                construction["observations"],
            ),
        )

    def test_agree_obligations_need_a_direct_check(self) -> None:
        """Agree is a direct comparison: a direct satisfaction stands."""

        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            mixed = sink.obligation("p.bad", "mixed", subject=t.PATH)
            sink.observe(t.observation(mixed))
            sink.observe(t.observation(mixed, "o2"))
            sink.constrain(effects.Agree("o1", "o2", "raw", mixed))
            sink.satisfied(mixed)
            agreed = sink.obligation("p.bad", "agreed", subject=t.PATH)
            sink.constrain(effects.Agree("o1", "o2", "raw", agreed))
            sink.satisfied(agreed)
            undecided = sink.obligation("p.bad", "undecided", subject=t.PATH)
            sink.constrain(effects.Agree("o1", "o2", "raw", undecided))

        result = t.result(self._run(collect, pinned=t.pinned(ABSENT)), "p")
        self.assertEqual(
            [
                ("agreed", ObligationStatus.SATISFIED, None),
                ("mixed", ObligationStatus.SATISFIED, None),
                ("undecided", ObligationStatus.NOT_EVALUATED, SOLVER_UNAVAILABLE),
            ],
            [(o.scope, o.status, o.not_evaluated_reason) for o in result.obligations],
        )

    def test_excluded_constraint_names_its_reason(self) -> None:
        """A healthy constraint over an excluded contract observation says so."""

        def contracts(
            _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: Any
        ) -> None:
            sink.group(t.group())
            sink.observe(t.observation(effects.ObligationRef("p.bad", "a"), "o1"))

        def broken(_input: CheckInput, sink: CollectorSink) -> None:
            sink.obligation("p.bad", "a", subject=t.PATH)
            raise RuntimeError("collector bug")

        def healthy(_input: CheckInput, sink: CollectorSink) -> None:
            ordered = sink.obligation("q.bad", "ordered", subject=t.PATH)
            sink.constrain(effects.Before("g1", "o1", ordered))

        verdict = check(
            t.target(
                t.prop("p", broken),
                t.prop("q", healthy),
                contracts=contracts,
                pinned=t.pinned(ABSENT),
            ),
            self._SPEC,
            t.READ_OK,
        )
        (obligation,) = t.result(verdict, "q").obligations
        self.assertEqual(
            (ObligationStatus.NOT_EVALUATED, SOLVER_INPUT_EXCLUDED),
            (obligation.status, obligation.not_evaluated_reason),
        )

    def test_direct_violation_survives_unsolved_input_and_later_calls(self) -> None:
        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation(
                "p.bad", "v", subject=t.PATH, witnesses=(Witness("channel", "fuse"),)
            )
            sink.observe(t.observation(ref))
            sink.violated(
                ref, Attribution.PRODUCT, "first", (Witness("read-action", "r"),)
            )
            sink.violated(ref, Attribution.ORACLE_SUSPECT, "second")
            sink.satisfied(ref)
            sink.not_evaluated(ref, "p.no-input")
            unrelated = sink.obligation("p.bad", "w", subject=t.PATH)
            sink.observe(t.observation(unrelated, "o2"))

        verdict = self._run(collect)
        violated, unrelated = t.result(verdict, "p").obligations
        self.assertEqual(
            (ObligationStatus.VIOLATED, Attribution.PRODUCT, "first"),
            (violated.status, violated.attribution, violated.reason),
        )
        self.assertEqual(
            (Witness("channel", "fuse"), Witness("read-action", "r")),
            violated.witnesses,
        )
        self.assertEqual(ObligationStatus.NOT_EVALUATED, unrelated.status)
        self.assertEqual((REJECT, "first"), (verdict.kind, verdict.reason))

    def test_not_evaluated_outranks_satisfied_and_keeps_its_first_reason(self) -> None:
        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("p.bad", "a", subject=t.PATH)
            sink.satisfied(ref)
            sink.not_evaluated(ref, "p.no-input", "first")
            sink.not_evaluated(ref, SOLVER_UNAVAILABLE, "second")
            sink.satisfied(ref)

        (obligation,) = t.result(self._run(collect), "p").obligations
        self.assertEqual(
            ("p.no-input", "first"),
            (obligation.not_evaluated_reason, obligation.reason),
        )

    def test_earliest_violation_wins_among_equal_severity(self) -> None:
        """An equally severe later violation keeps the first reason and witnesses."""

        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("p.bad", "a", subject=t.PATH)
            sink.violated(ref, Attribution.PRODUCT, "first loss", (Witness("n", "1"),))
            sink.violated(ref, Attribution.PRODUCT, "second loss", (Witness("n", "2"),))

        verdict = self._run(collect)
        self.assertEqual((REJECT, "first loss"), (verdict.kind, verdict.reason))
        self.assertEqual(
            (Witness("n", "1"),), t.result(verdict, "p").obligations[0].witnesses
        )

    def test_constraints_other_than_agree_stay_on_one_side(self) -> None:
        """Only Agree compares the two sides; every other constraint is one-sided."""
        reference = effects.Cell("reference", "path", "file")
        produce = effects.Effect(
            reference, EffectKind.PRODUCE, Value("file"), None, "r"
        )

        def contracts(_spec: RunSpec, _facts: Any, _pinned: Any, sink: Any) -> None:
            sink.group(t.group())
            sink.group(t.group("g2", effects=(produce,)))

        constraints: dict[str, Callable[[effects.ObligationRef], Any]] = {
            "before": lambda ref: effects.Before("o1", "or", ref),
            "supersedes": lambda ref: effects.Supersedes("g1", "g2", ref),
            "must-appear-in": lambda ref: effects.MustAppearIn("g1", "or", ref),
            "visible-after-ack": lambda ref: effects.VisibleAfterAck("g2", "o1", ref),
            "exclusive": lambda ref: effects.Exclusive(("g1", "g2"), ref),
            "agree": lambda ref: effects.Agree("o1", "or", "raw", ref),
        }
        for name, build in constraints.items():
            with self.subTest(constraint=name):

                def collect(
                    _input: CheckInput, sink: CollectorSink, build=build
                ) -> None:
                    ref = sink.obligation("p.bad", "a", subject=t.PATH)
                    sink.observe(t.observation(ref))
                    sink.observe(t.observation(ref, "or", cells=(reference,)))
                    sink.constrain(build(ref))

                error = t.result(self._run(collect, contracts=contracts), "p").error
                if name == "agree":
                    self.assertIsNone(error)
                    continue
                assert error is not None
                self.assertIn("joins sides ['primary', 'reference']", error.message)

    def test_most_severe_direct_attribution_is_kept(self) -> None:
        """A later, more severe violation replaces a milder one, never the reverse."""
        orders = {
            "harness then product": (
                (Attribution.HARNESS, "recorder looked odd"),
                (Attribution.PRODUCT, "product lost a write"),
            ),
            "product then harness": (
                (Attribution.PRODUCT, "product lost a write"),
                (Attribution.HARNESS, "recorder looked odd"),
            ),
            "reference then undetermined": (
                (Attribution.REFERENCE, "reference differs"),
                (Attribution.UNDETERMINED, "product lost a write"),
            ),
        }
        for name, violations in orders.items():
            with self.subTest(order=name):

                def collect(
                    _input: CheckInput, sink: CollectorSink, violations=violations
                ) -> None:
                    ref = sink.obligation("p.bad", "a", subject=t.PATH)
                    for attribution, reason in violations:
                        sink.violated(ref, attribution, reason)

                verdict = self._run(collect)
                self.assertEqual(
                    (REJECT, "product lost a write"), (verdict.kind, verdict.reason)
                )

    def test_contract_inputs_are_claimed_by_their_obligation(self) -> None:
        def contracts(
            _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: Any
        ) -> None:
            ref = effects.ObligationRef("p.bad", "a")
            requirement = effects.Requirement(
                (t.FILE_CELL,),
                Expect(((effects.ANY,),)),
                effects.ObligationRef("p.bad", "b"),
            )
            sink.group(t.group(requires=(requirement,)))
            sink.observe(t.observation(ref, "derived"))

        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            sink.satisfied(sink.obligation("p.bad", "a", subject=t.PATH))
            sink.satisfied(sink.obligation("p.bad", "b", subject=t.PATH))

        verdict = self._run(collect, contracts=contracts)
        self.assertEqual(
            [("a", SOLVER_UNKNOWN_INITIAL), ("b", SOLVER_UNKNOWN_INITIAL)],
            [
                (o.scope, o.not_evaluated_reason)
                for o in t.result(verdict, "p").obligations
            ],
        )

        def collect_a(_input: CheckInput, sink: CollectorSink) -> None:
            sink.satisfied(sink.obligation("p.bad", "a", subject=t.PATH))

        unclaimed = t.result(self._run(collect_a, contracts=contracts), "p").error
        assert unclaimed is not None
        self.assertIn("p.bad", unclaimed.message)
        self.assertIn("never declared", unclaimed.message)

    def test_contracts_hook_constraints_are_engine_input(self) -> None:
        cycle = effects.ObligationRef("p.bad", "cycle")

        def contracts(
            _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: Any
        ) -> None:
            sink.group(t.group())
            sink.group(t.group("g2"))
            sink.constrain(effects.Supersedes("g1", "g2", cycle))
            sink.constrain(effects.Supersedes("g2", "g1", cycle))

        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            sink.obligation("p.bad", "cycle", subject=t.PATH)

        verdict = self._run(collect, contracts=contracts, pinned=t.pinned(ABSENT))
        (obligation,) = t.result(verdict, "p").obligations
        self.assertEqual(ObligationStatus.VIOLATED, obligation.status)
        (proof,) = t.report(verdict).proofs
        self.assertEqual((("g1", "g2"), (cycle,)), (proof.groups, proof.obligations))

        def dangling(
            _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: Any
        ) -> None:
            sink.group(t.group())
            sink.constrain(effects.Supersedes("g1", "nowhere", cycle))

        error = t.result(self._run(collect, contracts=dangling), "p").error
        assert error is not None
        self.assertEqual("EffectContractError", error.type)
        self.assertIn("unknown node", error.message)

    def test_contract_nodes_are_checked_against_the_history(self) -> None:
        unknown_rule = effects.ObligationRef("z.bad", "a")
        cases: dict[str, tuple[str, Callable[[Any], None]]] = {
            "group of an action not in the spec": (
                "never invoked",
                lambda sink: sink.group(t.group(action="zz")),
            ),
            "acked group after completion": (
                "ends after 'r' completed",
                lambda sink: sink.group(t.group(window=Window(1.0, 3.0))),
            ),
            "observation of an unknown rule": (
                "unknown rule 'z.bad'",
                lambda sink: sink.observe(t.observation(unknown_rule)),
            ),
            "group and observation share an id": (
                "repeated contract node id",
                _group_and_observation_share_g1,
            ),
        }
        for name, (message, body) in cases.items():
            with self.subTest(case=name):

                def contracts(
                    _spec: RunSpec,
                    _facts: HistoryFacts,
                    _pinned: Any,
                    sink: Any,
                    body=body,
                ) -> None:
                    body(sink)

                verdict = self._run(_record_one, contracts=contracts)
                error = t.result(verdict, "p").error
                assert error is not None
                self.assertEqual("EffectContractError", error.type)
                self.assertIn(message, error.message)
        indeterminate = self._run(
            _record_one,
            contracts=lambda _s, _f, _p, sink: sink.group(
                t.group(status=Status.INDETERMINATE, window=Window(1.0, math.inf))
            ),
        )
        self.assertIsNone(t.result(indeterminate, "p").error)

    def test_effect_hook_failure_blocks_target_properties_only(self) -> None:
        def explode(*_args: Any) -> None:
            raise OSError("fixture unreadable")

        for hook in ("contracts", "pinned"):
            with self.subTest(hook=hook):
                verdict = check(
                    t.target(t.quiet(), **{hook: explode}), self._SPEC, t.READ_OK
                )
                quiet = t.result(verdict, "quiet")
                assert quiet.error is not None
                self.assertEqual(
                    ("EffectContractError", "OSError: fixture unreadable"),
                    (quiet.error.type, quiet.error.message),
                )
                self.assertEqual((), quiet.obligations)
                self.assertTrue(t.result(verdict, CORE_ACTIVITY).complete)
                self.assertEqual(PROPERTY_ERROR, verdict.not_evaluated_cause)


class EngineVerdictTest(unittest.TestCase):
    def test_engine_verdict_is_identical_across_hash_seeds(self) -> None:
        verdict = decode_verdict(_engine_verdict_bytes())
        report = t.report(verdict)
        self.assertEqual(REJECT, verdict.kind)
        self.assertEqual(
            [
                ("cycle", ObligationStatus.VIOLATED),
                ("o0", ObligationStatus.SATISFIED),
                ("o1", ObligationStatus.SATISFIED),
                ("o2", ObligationStatus.VIOLATED),
            ],
            [(o.scope, o.status) for o in t.result(verdict, "p").obligations],
        )
        self.assertEqual(2, len(report.proofs))
        self.assertIn("search", report.work.phases)
        code = (
            "import sys\n"
            "from eden.integration.correctness.exploratory.core.test import engine_input_test\n"
            "sys.stdout.write(engine_input_test._engine_verdict_bytes().decode())\n"
        )
        outputs = [run_in_fresh_interpreter(code, seed) for seed in ("0", "12345")]
        self.assertEqual(outputs[0], outputs[1])
        self.assertEqual(_engine_verdict_bytes().decode(), outputs[0])
