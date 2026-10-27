# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The collector sink: obligations are idempotent, constraint endpoints match
their signatures, observation nodes are unique per run and bound to their
action, a shared node unions the obligations of every property that recorded
it, and node conflicts roll back the same way whatever the property order. A
violation recorded "unless the engine decides" waits for the engine, and the
sink keeps the most severe one.
"""

from __future__ import annotations

import unittest
from collections.abc import Callable
from itertools import permutations
from typing import Any

from eden.integration.correctness.exploratory.core import effects
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    check,
    evaluate_with_nodes,
    REJECT,
    Verdict,
)
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    eq,
    Expect,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    ObligationStatus,
    PropertyDef,
    SOLVER_UNAVAILABLE,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.test import checker_testing as t


def _contract_one_group(
    _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: effects.ContractSink
) -> None:
    sink.group(t.group())


def _contract_two_groups(
    _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: effects.ContractSink
) -> None:
    sink.group(t.group())
    sink.group(t.group("g2"))


def _constrain_twice(sink: CollectorSink, ref: effects.ObligationRef) -> None:
    for _ in range(2):
        sink.constrain(effects.Before("o1", "o2", ref))


class CollectorSinkTest(unittest.TestCase):
    _SPEC = t.spec([t.action("r", "read")])

    def _run(self, collect: t.Collect, **target: Any) -> Verdict:
        return check(t.target(t.prop("p", collect), **target), self._SPEC, t.READ_OK)

    def _contract_error(self, body: Callable[..., None]) -> str:
        def bad(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("p.bad", "a", subject=t.PATH)
            sink.observe(t.observation(ref))
            sink.observe(t.observation(ref, "o2"))
            body(sink, ref)

        result = t.result(self._run(bad, contracts=_contract_two_groups), "p")
        assert result.error is not None
        self.assertEqual("PropertyContractError", result.error.type)
        return result.error.message

    def test_one_obligations_observations_share_a_serving_path(self) -> None:
        """Attribution groups co-observers by obligation, so they share a serving path."""

        def co_observed(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("p.bad", "a", subject=t.PATH)
            sink.observe(t.observation(ref))
            sink.observe(t.observation(ref, "o2", channel="sl"))

        for paths, error in (
            ({"fuse": "daemon", "sl": "daemon"}, None),
            ({"fuse": "daemon", "sl": "cli"}, "on serving paths 'daemon' and 'cli'"),
        ):
            with self.subTest(paths=paths):
                verdict = self._run(
                    co_observed, contracts=_contract_one_group, serving_paths=paths
                )
                result = t.result(verdict, "p")
                if error is None:
                    self.assertIsNone(result.error)
                else:
                    assert result.error is not None
                    self.assertIn(error, result.error.message)

    def test_obligations_are_idempotent_and_constraints_name_known_nodes(self) -> None:
        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            first = sink.obligation("p.bad", "a", subject=t.PATH)
            self.assertEqual(first, sink.obligation("p.bad", "a", subject=t.PATH))
            sink.observe(t.observation(first))
            sink.observe(t.observation(first))
            sink.constrain(effects.MustAppearIn("g1", "o1", first))
            sink.satisfied(first)

        verdict = self._run(collect, contracts=_contract_one_group)
        construction = t.report(verdict).work.phases["construction"]
        self.assertEqual((1, 1), (construction["groups"], construction["observations"]))
        self.assertEqual([("a", "not-evaluated")], t.statuses(t.result(verdict, "p")))

    def test_constraint_endpoints_match_their_signatures(self) -> None:
        """Each constraint class names only the node types it is defined over."""
        legal: dict[str, Callable[[effects.ObligationRef], effects.Constraint]] = {
            "before group-observation": lambda ref: effects.Before("g1", "o1", ref),
            "before observations": lambda ref: effects.Before("o1", "o2", ref),
            "supersedes groups": lambda ref: effects.Supersedes("g1", "g2", ref),
            "must-appear-in": lambda ref: effects.MustAppearIn("g1", "o1", ref),
            "visible-after-ack": lambda ref: effects.VisibleAfterAck("g1", "o1", ref),
            "agree": lambda ref: effects.Agree("o1", "o2", "raw", ref),
            "exclusive": lambda ref: effects.Exclusive(("g1", "g2"), ref),
        }
        for name, build in legal.items():
            with self.subTest(legal=name):

                def ok(_input: CheckInput, sink: CollectorSink, build=build) -> None:
                    ref = sink.obligation("p.bad", "a", subject=t.PATH)
                    sink.observe(t.observation(ref))
                    sink.observe(t.observation(ref, "o2"))
                    sink.constrain(build(ref))

                result = t.result(self._run(ok, contracts=_contract_two_groups), "p")
                self.assertIsNone(result.error)
        wrong: dict[str, tuple[str, Callable[..., None]]] = {
            "observation as group": (
                "'o1' is not a group",
                lambda sink, ref: sink.constrain(effects.MustAppearIn("o1", "g1", ref)),
            ),
            "supersedes observations": (
                "'o1' is not a group",
                lambda sink, ref: sink.constrain(effects.Supersedes("o1", "o2", ref)),
            ),
            "visible-after-ack reversed": (
                "'o2' is not a group",
                lambda sink, ref: sink.constrain(
                    effects.VisibleAfterAck("o2", "o1", ref)
                ),
            ),
            "agree over a group": (
                "'g1' is not an observation",
                lambda sink, ref: sink.constrain(effects.Agree("g1", "o1", "raw", ref)),
            ),
            "unknown node": (
                "unknown node 'zz'",
                lambda sink, ref: sink.constrain(effects.Before("o1", "zz", ref)),
            ),
            "unknown outcome variable": (
                "unknown outcome variables ['zz']",
                lambda sink, ref: sink.constrain(effects.Exclusive(("g1", "zz"), ref)),
            ),
            "duplicate constraint": ("recorded twice", _constrain_twice),
        }
        for name, (message, body) in wrong.items():
            with self.subTest(wrong=name):
                self.assertIn(message, self._contract_error(body))

    def test_observation_nodes_are_run_unique_and_bound_to_their_action(self) -> None:
        cases: dict[str, tuple[str, Callable[..., None]]] = {
            "group id reused": (
                "reuses group id 'g1'",
                lambda sink, ref: sink.observe(t.observation(ref, "g1")),
            ),
            "same id, other content": (
                "names a different observation",
                lambda sink, ref: sink.observe(
                    t.observation(ref, expect=Expect(((eq(Value("file")),),)))
                ),
            ),
            "action never invoked": (
                "never invoked",
                lambda sink, ref: sink.observe(t.observation(ref, "o3", action="zz")),
            ),
            "window before invocation": (
                "starts before 'r' was invoked",
                lambda sink, ref: sink.observe(
                    t.observation(ref, "o3", window=Window(0.5, 2.0))
                ),
            ),
            "window after completion": (
                "ends after 'r' completed",
                lambda sink, ref: sink.observe(
                    t.observation(ref, "o3", window=Window(1.0, 2.5))
                ),
            ),
            "another property's obligation": (
                "undeclared obligation",
                lambda sink, ref: sink.observe(
                    t.observation(effects.ObligationRef("q.bad", "a"), "o3")
                ),
            ),
        }
        for name, (message, body) in cases.items():
            with self.subTest(case=name):
                self.assertIn(message, self._contract_error(body))

    def test_one_observation_node_supports_obligations_across_properties(
        self,
    ) -> None:
        """Identical observations merge into one node; a conflicting reuse fails."""

        def make(pid: str, expect: Expect) -> t.Collect:
            def collect(_input: CheckInput, sink: CollectorSink) -> None:
                ref = sink.obligation(f"{pid}.bad", "r", subject=t.PATH)
                sink.observe(t.observation(ref, "read:r", expect=expect))

            return collect

        same = Expect(((eq(ABSENT),),))
        verdict = check(
            t.target(t.prop("p", make("p", same)), t.prop("q", make("q", same))),
            self._SPEC,
            t.READ_OK,
        )
        self.assertEqual(
            [None, None], [t.result(verdict, pid).error for pid in ("p", "q")]
        )
        self.assertEqual(
            1, t.report(verdict).work.phases["construction"]["observations"]
        )
        conflicting = check(
            t.target(
                t.prop("p", make("p", same)),
                t.prop("q", make("q", Expect(((eq(Value("file")),),)))),
            ),
            self._SPEC,
            t.READ_OK,
        )
        for pid, other in (("p", "q"), ("q", "p")):
            error = t.result(conflicting, pid).error
            assert error is not None
            self.assertIn(
                f"'read:r' names a different observation in ['{other}']",
                error.message,
            )

    def test_merged_node_unions_the_obligations_of_finished_properties(self) -> None:
        """A shared node carries every finished property's obligations."""

        def make(pid: str, crash: bool = False) -> PropertyDef:
            def collect(_input: CheckInput, sink: CollectorSink) -> None:
                ref = sink.obligation(f"{pid}.bad", "r", subject=t.PATH)
                sink.observe(t.observation(ref, "read:r"))
                if crash:
                    raise RuntimeError(f"{pid} crashed")

            return t.prop(pid, collect)

        for crashed, expected in ((False, ["p.bad", "q.bad"]), (True, ["q.bad"])):
            with self.subTest(p_crashes=crashed):
                target = t.target(make("p", crashed), make("q"))
                check_input = t.check_input(self._SPEC, t.READ_OK, target)
                results, _work, nodes = evaluate_with_nodes(
                    target.properties, check_input
                )
                self.assertEqual(
                    [effects.ObligationRef(rule, "r") for rule in expected],
                    list(nodes.observations["read:r"].obligations),
                )
                self.assertEqual(
                    [crashed, False], [r.error is not None for r in results]
                )

    def test_node_conflicts_and_rollback_do_not_depend_on_property_order(self) -> None:
        """Every order gives the same errors, results, and merged nodes."""
        absent, file = Expect(((eq(ABSENT),),)), Expect(((eq(Value("file")),),))

        def make(
            pid: str,
            shared: tuple[str, Expect],
            *,
            crash: bool = False,
            after: str = "",
        ) -> PropertyDef:
            def collect(_input: CheckInput, sink: CollectorSink) -> None:
                ref = sink.obligation(f"{pid}.bad", "r", subject=t.PATH)
                sink.observe(t.observation(ref, f"own-{pid}"))
                sink.observe(t.observation(ref, shared[0], expect=shared[1]))
                if after:
                    sink.constrain(effects.Before(f"own-{pid}", after, ref))
                if crash:
                    raise RuntimeError(f"{pid} crashed")

            return t.prop(pid, collect)

        properties = (
            make("p", ("shared", absent)),
            make("q", ("shared", file)),
            make("c", ("solo", file), crash=True),
            make("u", ("solo", absent)),
            make("e", ("extra", absent), after="own-u"),
        )

        def outcome(order: tuple[PropertyDef, ...]) -> tuple[Any, ...]:
            verdict = check(t.target(*order), self._SPEC, t.READ_OK)
            results = {r.property_id: r for r in t.report(verdict).results}
            return (
                {pid: results[pid] for pid in sorted(results)},
                t.report(verdict).work.phases["construction"]["observations"],
            )

        expected = outcome(properties)
        errors = {
            pid: None if result.error is None else result.error.message
            for pid, result in expected[0].items()
            if not pid.startswith("core.")
        }
        self.assertEqual(
            {
                "c": "c crashed",
                "e": "ValueError: constraint names unknown node 'own-u'",
                "p": "node id 'shared' names a different observation in ['q']",
                "q": "node id 'shared' names a different observation in ['p']",
                "u": None,
            },
            errors,
        )
        self.assertEqual(2, expected[1])
        for order in permutations(properties):
            with self.subTest(order=[d.id for d in order]):
                self.assertEqual(expected, outcome(order))

    def test_pinned_facts_reach_collectors_and_the_lazy_query(self) -> None:
        queries: list[effects.Cell] = []

        class Pinned:
            def initial(self, cell: effects.Cell) -> Value:
                queries.append(cell)
                return ABSENT

        seen: list[Any] = []

        def collect(check_input: CheckInput, sink: CollectorSink) -> None:
            subject = t.FILE_CELL
            seen.append(check_input.pinned)
            seen.append(check_input.effects.initial(subject))
            seen.append(check_input.effects.initial(subject))
            sink.satisfied(sink.obligation("p.bad", "a", subject=t.PATH))

        pinned = Pinned()
        self.assertEqual(ACCEPT, self._run(collect, pinned=lambda _spec: pinned).kind)
        self.assertEqual([pinned, ABSENT, ABSENT], seen)
        self.assertEqual(1, len(queries))


class ViolatedUnlessEngineTest(unittest.TestCase):
    """A violation recorded with `violated_unless_engine` waits for the engine."""

    _SPEC = t.spec([t.action("r", "read")])

    def _resolve(
        self,
        seen: Value | None,
        *,
        direct: bool = False,
        raises: bool = False,
        **target: Any,
    ) -> Any:
        """Resolve one such obligation; `seen` is an observation that is engine input."""

        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("unless.bad", "a", subject=t.PATH)
            if seen is not None:
                sink.observe(t.observation(ref, expect=Expect(((eq(seen),),))))
            if direct:
                sink.violated(ref, Attribution.PRODUCT, "direct")
            sink.violated_unless_engine(ref, Attribution.REFERENCE, "the other side")
            if raises:
                raise RuntimeError("collector stopped")

        verdict = check(
            t.target(t.prop("unless", collect), **target),
            self._SPEC,
            t.READ_OK,
        )
        (found,) = t.result(verdict, "unless").obligations
        return found

    def test_the_engine_decides_first(self) -> None:
        pinned = t.pinned(ABSENT)
        violated = self._resolve(Value("file"), pinned=pinned)
        self.assertEqual(
            (ObligationStatus.VIOLATED, Attribution.PRODUCT, True),
            (violated.status, violated.attribution, violated.proof is not None),
        )
        satisfied = self._resolve(ABSENT, pinned=pinned)
        self.assertEqual(
            (ObligationStatus.VIOLATED, Attribution.REFERENCE, None),
            (satisfied.status, satisfied.attribution, satisfied.proof),
        )
        undecided = self._resolve(ABSENT)
        self.assertEqual(ObligationStatus.NOT_EVALUATED, undecided.status)
        unavailable = self._resolve(ABSENT, raises=True, pinned=pinned)
        self.assertEqual(
            (ObligationStatus.NOT_EVALUATED, SOLVER_UNAVAILABLE),
            (unavailable.status, unavailable.not_evaluated_reason),
        )

    def test_a_direct_violation_still_wins(self) -> None:
        for raises in (False, True):
            with self.subTest(raises=raises):
                found = self._resolve(
                    ABSENT, direct=True, raises=raises, pinned=t.pinned(ABSENT)
                )
                self.assertEqual(Attribution.PRODUCT, found.attribution)

    def test_without_engine_input_it_stands(self) -> None:
        found = self._resolve(None)
        self.assertEqual(
            (ObligationStatus.VIOLATED, Attribution.REFERENCE),
            (found.status, found.attribution),
        )

    def test_the_sink_keeps_the_most_severe(self) -> None:
        def collect(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("unless.bad", "a", subject=t.PATH)
            sink.violated_unless_engine(ref, Attribution.HARNESS, "first")
            sink.violated_unless_engine(ref, Attribution.PRODUCT, "second")
            sink.violated_unless_engine(ref, Attribution.REFERENCE, "third")

        target = t.target(t.prop("unless", collect))
        spec = t.spec([t.action("r", "read")])
        events = [t.event("invoke", "r", 1.0), t.event("ok", "r", 2.0)]
        verdict = check(target, spec, events)
        self.assertEqual(REJECT, verdict.kind, verdict.reason)
        (found,) = t.result(verdict, "unless").obligations
        self.assertEqual(
            (ObligationStatus.VIOLATED, Attribution.PRODUCT, "second"),
            (found.status, found.attribution, found.reason),
        )
