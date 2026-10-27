# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""How `check` turns property results into one verdict: every property runs in
declaration order; the kind ranks reject over harness-suspect over
not-evaluated over accept, and the reason is the first at that rank in
property, rule, then scope order, decided by typed attribution, never by
reason text. A collector that raises, exceeds its obligation bound, uses
undeclared vocabulary, or hands back hostile values becomes a visible property
error that keeps its partial results and never hides another property's
findings, whatever its error text.
"""

from __future__ import annotations

import json
import math
import os
import random
import unittest
from collections.abc import Callable
from dataclasses import replace
from typing import Any, cast
from unittest import mock

from eden.integration.correctness.exploratory.core import checker, effects
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    BLOCKED,
    check,
    decode_verdict,
    encode_verdict,
    evaluate_properties,
    HARNESS_SUSPECT,
    INVALID_HISTORY,
    NO_EVALUATION,
    NOT_EVALUATED,
    PROPERTY_ERROR,
    read_verdict_document,
    REJECT,
    Verdict,
    verdict_document,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    Event,
    FAIL,
    INVOKE,
    OK,
)
from eden.integration.correctness.exploratory.core.jsonio import MAX_SCALAR_BYTES
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    CoverageObligation,
    DAEMON_SUBJECT,
    MAX_RUN_OBLIGATIONS,
    PropertyDef,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.test import checker_testing as t


class CheckerTest(unittest.TestCase):
    def test_projection_suite_clean_violation_suspect_zero_blocked_error(self) -> None:
        """reject > harness-suspect > not-evaluated > accept; findings never vanish.

        The reason is the first at the winning rank in property, rule, then
        scope order, and typed attribution, never reason text, decides
        suspicion.
        """

        def ordered(_input: CheckInput, sink: CollectorSink) -> None:
            t.violate(sink, "p.zeta", "a", Attribution.PRODUCT, "p/x≠y: zeta")
            t.violate(sink, "p.bad", "b", Attribution.PRODUCT, "p/x≠y: bad b")
            t.violate(sink, "p.bad", "a", Attribution.PRODUCT, "p/x≠y: bad a")

        def blocked(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("stuck.bad", "r", subject=t.PATH)
            sink.not_evaluated(ref, "stuck.no-input", "no reads completed")

        def raises(_input: CheckInput, sink: CollectorSink) -> None:
            sink.satisfied(sink.obligation("broken.bad", "r", subject=t.PATH))
            raise KeyError("p")

        stuck = t.prop("stuck", blocked)
        broken = t.prop("broken", raises)
        zero = t.finding_property("zero", satisfied=0)
        suspect = t.finding_property("suspect", (Attribution.ORACLE_SUSPECT, "s"))
        harness = t.finding_property("harness", (Attribution.HARNESS, "h"))
        product = t.finding_property("product", (Attribution.PRODUCT, "p"))
        first = t.finding_property(
            "first", (Attribution.PRODUCT, "bytes/fuse≠model: 1")
        )
        second = t.finding_property(
            "second", (Attribution.PRODUCT, "status/sl≠model: 2")
        )
        rules = {f"p.{name}": t.rule(f"p.{name}") for name in ("bad", "zeta")}
        scoped = t.prop("p", ordered, rules=rules)
        text_suspect = t.finding_property(
            "status", (Attribution.ORACLE_SUSPECT, "status/sl≠model: M")
        )
        text_reject = t.finding_property(
            "status", (Attribution.UNDETERMINED, "status/model≠live: M")
        )
        late_reject = t.finding_property(
            "readdir", (Attribution.UNDETERMINED, "readdir/fuse≠model: listed twice")
        )
        reference = t.finding_property("reference", (Attribution.REFERENCE, "r"))
        cases = (
            ((t.quiet(),), ACCEPT, None, "all-properties-accept"),
            ((product,), REJECT, None, "p"),
            ((suspect,), HARNESS_SUSPECT, None, "s"),
            ((harness,), HARNESS_SUSPECT, None, "h"),
            ((zero,), NOT_EVALUATED, NO_EVALUATION, None),
            ((), NOT_EVALUATED, NO_EVALUATION, None),
            ((zero, t.quiet()), ACCEPT, None, "all-properties-accept"),
            (
                (stuck, t.quiet()),
                NOT_EVALUATED,
                BLOCKED,
                "blocked/stuck.no-input: no reads completed",
            ),
            (
                (stuck, broken, t.quiet()),
                NOT_EVALUATED,
                PROPERTY_ERROR,
                "property-error/broken≠checker: KeyError",
            ),
            ((broken, suspect), HARNESS_SUSPECT, None, "s"),
            ((stuck, product), REJECT, None, "p"),
            ((first, second), REJECT, None, "bytes/fuse≠model: 1"),
            ((scoped,), REJECT, None, "p/x≠y: bad a"),
            ((text_reject,), REJECT, None, "status/model≠live: M"),
            (
                (text_suspect, late_reject),
                REJECT,
                None,
                "readdir/fuse≠model: listed twice",
            ),
            (
                (text_suspect, harness, reference),
                HARNESS_SUSPECT,
                None,
                "status/sl≠model: M",
            ),
        )
        spec = t.spec([t.action("r", "read")])
        for properties, kind, cause, reason in cases:
            with self.subTest(properties=[p.id for p in properties]):
                verdict = check(t.target(*properties), spec, t.READ_OK)
                self.assertEqual(kind, verdict.kind, verdict.reason)
                self.assertEqual(cause, verdict.not_evaluated_cause)
                if reason is not None:
                    self.assertEqual(reason, verdict.reason)
        suspect_lanes = dict.fromkeys(
            ("status", "harness", "reference"), HARNESS_SUSPECT
        )
        for properties, lanes in (
            (
                (zero, stuck, broken, t.quiet()),
                {
                    "zero": NOT_EVALUATED,
                    "stuck": NOT_EVALUATED,
                    "broken": NOT_EVALUATED,
                    "quiet": ACCEPT,
                },
            ),
            ((first, second), {"first": REJECT, "second": REJECT}),
            (
                (text_suspect, late_reject),
                {"status": HARNESS_SUSPECT, "readdir": REJECT},
            ),
            ((text_suspect, harness, reference), suspect_lanes),
        ):
            with self.subTest(lanes=list(lanes)):
                verdict = check(t.target(*properties), spec, t.READ_OK)
                self.assertEqual(lanes, verdict.lanes)
        scoped_result = t.result(check(t.target(scoped), spec, t.READ_OK), "p")
        self.assertEqual(
            [("p.bad", "a"), ("p.bad", "b"), ("p.zeta", "a")],
            [(o.rule_id, o.scope) for o in scoped_result.obligations],
        )

    def test_exception_keeps_partial_obligations_and_siblings_continue(self) -> None:
        """A raising collector keeps what it resolved; later properties still evaluate."""

        def partial(_input: CheckInput, sink: CollectorSink) -> None:
            sink.satisfied(sink.obligation("partial.bad", "a", subject=t.PATH))
            t.violate(sink, "partial.bad", "b", Attribution.PRODUCT, "first")
            t.violate(sink, "partial.bad", "c", Attribution.ORACLE_SUSPECT, "second")
            sink.obligation("partial.bad", "d", subject=t.PATH)
            raise RuntimeError("boom")

        spec = t.spec([t.action("r", "read")])
        verdict = check(
            t.target(t.prop("partial", partial), t.quiet("after")), spec, t.READ_OK
        )
        result = t.result(verdict, "partial")
        self.assertFalse(result.complete)
        self.assertEqual(
            ("RuntimeError", "boom"), (result.error.type, result.error.message)
        )
        self.assertEqual(["first", "second"], [f.reason for f in result.findings])
        self.assertEqual(
            [("a", "satisfied"), ("b", "violated"), ("c", "violated")],
            t.statuses(result),
        )
        self.assertEqual([("ok0", "satisfied")], t.statuses(t.result(verdict, "after")))
        self.assertEqual((REJECT, "first"), (verdict.kind, verdict.reason))

    def test_sink_rejects_undeclared_vocabulary_without_losing_results(
        self,
    ) -> None:
        misuses: dict[str, Callable[[CollectorSink, effects.ObligationRef], Any]] = {
            "reason": lambda sink, ref: sink.not_evaluated(ref, "m.other"),
            "cover": lambda sink, ref: sink.covered("m.unknown"),
            "rule": lambda sink, ref: sink.obligation("m.gone", "x", subject=t.PATH),
            "subject": lambda sink, ref: sink.obligation(
                "m.bad", "x", subject=Subject(SubjectKind.PATH, "/tmp/f")
            ),
            "subject kind": lambda sink, ref: sink.obligation(
                "m.bad", "x", subject=DAEMON_SUBJECT
            ),
            "scope": lambda sink, ref: sink.obligation("m.bad", "", subject=t.PATH),
            "redeclared": lambda sink, ref: sink.obligation(
                "m.bad", "other", subject=t.PATH, witnesses=(Witness("a", "b"),)
            ),
            "undeclared ref": lambda sink, ref: sink.satisfied(
                effects.ObligationRef("m.bad", "never")
            ),
            "not a ref": lambda sink, ref: sink.satisfied(
                cast(Any, ("m.bad", "other"))
            ),
            "attribution": lambda sink, ref: sink.violated(
                ref, cast(Any, "model-suspect"), "x"
            ),
            "oversize": lambda sink, ref: sink.violated(
                ref, Attribution.PRODUCT, "x" * (MAX_SCALAR_BYTES + 1)
            ),
            "witness": lambda sink, ref: sink.violated(
                ref, Attribution.PRODUCT, "x", (Witness("A", "b"),)
            ),
            "count": lambda sink, ref: sink.covered("m.seen", -1),
            "observation": lambda sink, ref: sink.observe(cast(Any, "obs")),
            "constraint": lambda sink, ref: sink.constrain(cast(Any, ("a", "b"))),
        }

        def misuse(action: str) -> t.Collect:
            def run(_input: CheckInput, sink: CollectorSink) -> None:
                t.violate(sink, "m.bad", "kept", Attribution.PRODUCT, "kept")
                ref = sink.obligation("m.bad", "other", subject=t.PATH)
                misuses[action](sink, ref)
                sink.satisfied(ref)

            return run

        spec = t.spec([t.action("r", "read")])
        target = t.target()
        check_input = t.check_input(spec, t.READ_OK, target)
        for action in misuses:
            with self.subTest(action=action):
                (result,), _work = evaluate_properties(
                    [
                        t.prop(
                            "m",
                            misuse(action),
                            coverage=(CoverageObligation("m.seen", "seen"),),
                        )
                    ],
                    check_input,
                )
                self.assertFalse(result.complete)
                assert result.error is not None
                self.assertEqual("PropertyContractError", result.error.type)
                self.assertEqual(["kept"], [f.reason for f in result.findings])

    def test_unresolved_obligation_becomes_property_error(self) -> None:
        """An obligation with no result and no solver input is a collector bug."""

        def forgets(_input: CheckInput, sink: CollectorSink) -> None:
            t.violate(sink, "p.bad", "a", Attribution.PRODUCT, "kept")
            sink.obligation("p.bad", "b", subject=t.PATH)
            sink.obligation("p.bad", "c", subject=t.PATH)

        verdict = check(
            t.target(t.prop("p", forgets)), t.spec([t.action("r", "read")]), t.READ_OK
        )
        result = t.result(verdict, "p")
        assert result.error is not None
        self.assertEqual(
            (
                "PropertyContractError",
                "p.bad['b'] has no result and no solver input (+1 more)",
            ),
            (result.error.type, result.error.message),
        )
        self.assertEqual([("a", "violated")], t.statuses(result))
        self.assertEqual(REJECT, verdict.kind)

    def test_direct_violation_survives_the_run_obligation_cap(self) -> None:
        """Collectors declaring more than 16,384 obligations in all skip the
        engine, but a direct violation still rejects the run; exactly 16,384
        is evaluated whole."""
        spec = t.spec([t.action("r", "read")])
        core = 4  # daemon exit, activity, and one legality and schema per action
        huge = t.finding_property(
            "huge",
            (Attribution.PRODUCT, "direct"),
            satisfied=MAX_RUN_OBLIGATIONS - core,
        )
        over = check(t.target(huge), spec, t.READ_OK)
        self.assertEqual(REJECT, over.kind)
        self.assertEqual(
            ["direct"], [f.reason for f in t.result(over, "huge").findings]
        )
        fits = t.finding_property("fits", satisfied=MAX_RUN_OBLIGATIONS - core)
        result = t.result(check(t.target(fits), spec, t.READ_OK), "fits")
        self.assertIsNone(result.error)
        self.assertEqual(MAX_RUN_OBLIGATIONS - core, len(result.obligations))

    def test_structural_failures_stay_outside_property_evaluation(self) -> None:
        """Malformed histories are typed invalid-history with every problem kept."""
        spec = t.spec([t.action("r", "read")])
        target = t.target(t.quiet())
        cases = {
            "duplicate terminal": [*t.READ_OK, t.event(FAIL, "r", 3.0)],
            "terminal before invocation": [
                t.event(OK, "r", 1.0),
                t.event(INVOKE, "r", 2.0),
            ],
            "unknown action": [*t.READ_OK, t.event(INVOKE, "zz", 3.0)],
        }
        for name, events in cases.items():
            with self.subTest(history=name):
                verdict = check(target, spec, events)
                self.assertEqual(NOT_EVALUATED, verdict.kind)
                self.assertEqual(INVALID_HISTORY, verdict.not_evaluated_cause)
                self.assertRegex(verdict.reason, r"^invalid-history/history≠spec: ")
                assert verdict.report is not None
                self.assertEqual((), verdict.report.results)
                self.assertTrue(verdict.report.gate)
        missing = check(
            target, spec, [t.event(INVOKE, "r", 1.0)], execution={"status": "completed"}
        )
        self.assertEqual(INVALID_HISTORY, missing.not_evaluated_cause)
        prefix = check(
            target,
            spec,
            [t.event(INVOKE, "r", 1.0)],
            execution={"status": "harness-failure"},
        )
        self.assertEqual(NOT_EVALUATED, prefix.kind)
        self.assertEqual(
            [*t.CORE_IDS, "quiet"],
            [r.property_id for r in t.report(prefix).results],
        )

    def test_a_checker_defect_is_this_runs_invalid_history_with_its_traceback(
        self,
    ) -> None:
        """No exception escapes `check`: one run's defect never ends a campaign."""
        spec = t.spec([t.action("r", "read")])
        target = t.target(t.quiet())
        cleanup = Event(CLEANUP, 3.0, {"scheduler": {"status": ["completed"]}})
        unhashable = check(target, spec, [*t.READ_OK, cleanup])
        self.assertEqual(INVALID_HISTORY, unhashable.not_evaluated_cause)
        self.assertIn("bad status", unhashable.reason)

        def defect(*_args: object) -> None:
            raise TypeError("unhashable type: 'list'")

        with mock.patch.object(checker, "_judge", side_effect=defect):
            verdict = check(target, spec, t.READ_OK)
        self.assertEqual(NOT_EVALUATED, verdict.kind)
        self.assertEqual(INVALID_HISTORY, verdict.not_evaluated_cause)
        assert verdict.report is not None
        (problem,) = verdict.report.gate
        # Frames are package-relative, so the verdict bytes match on every host.
        self.assertIn("core/test/checker_test.py:", problem)
        self.assertNotIn(os.path.dirname(os.path.abspath(__file__)), problem)
        self.assertIn("TypeError: unhashable type: 'list'", problem)


class PropertyErrorTest(unittest.TestCase):
    _SPEC = t.spec([t.action("r", "read")])

    def test_contract_violation_is_sticky_when_the_collector_catches_it(self) -> None:
        def swallow(_input: CheckInput, sink: CollectorSink) -> None:
            ref = sink.obligation("sticky.bad", "a", subject=t.PATH)
            try:
                sink.not_evaluated(ref, "sticky.undeclared")
            except ValueError:
                pass
            sink.violated(ref, Attribution.PRODUCT, "kept")

        def quiet_swallow(_input: CheckInput, sink: CollectorSink) -> None:
            try:
                sink.obligation("quiet.bad", "a", subject=DAEMON_SUBJECT)
            except ValueError:
                pass

        verdict = check(t.target(t.prop("sticky", swallow)), self._SPEC, t.READ_OK)
        result = t.result(verdict, "sticky")
        assert result.error is not None
        self.assertFalse(result.complete)
        self.assertEqual("PropertyContractError", result.error.type)
        self.assertIn("undeclared reason 'sticky.undeclared'", result.error.message)
        self.assertEqual(["kept"], [f.reason for f in result.findings])
        self.assertEqual((REJECT, "kept"), (verdict.kind, verdict.reason))
        quiet = check(t.target(t.prop("quiet", quiet_swallow)), self._SPEC, t.READ_OK)
        self.assertEqual(
            (
                NOT_EVALUATED,
                PROPERTY_ERROR,
                "property-error/quiet≠checker: PropertyContractError",
            ),
            (quiet.kind, quiet.not_evaluated_cause, quiet.reason),
        )

    def test_oversized_error_survives_whole_and_is_visibly_not_evaluated(self) -> None:
        """An oversized error message is never cut; the marker blocks the run."""
        message = "x" * 10000 + "\ud800" + "é" * 3

        def loud(_input: CheckInput, sink: CollectorSink) -> None:
            raise RuntimeError(message)

        verdict = check(t.target(t.prop("loud", loud)), self._SPEC, t.READ_OK)
        result = t.result(verdict, "loud")
        assert result.error is not None
        self.assertEqual(message, result.error.message)
        self.assertEqual(10009, result.error.message_bytes)
        self.assertTrue(result.error.oversize)
        self.assertEqual({"loud": NOT_EVALUATED}, verdict.lanes)
        self.assertEqual(
            (
                NOT_EVALUATED,
                PROPERTY_ERROR,
                "property-error/loud≠checker: RuntimeError (oversize)",
            ),
            (verdict.kind, verdict.not_evaluated_cause, verdict.reason),
        )
        encoded = encode_verdict(verdict)
        encoded.decode("ascii")
        decoded = decode_verdict(encoded)
        self.assertEqual(verdict, decoded)
        decoded_error = t.result(decoded, "loud").error
        assert decoded_error is not None
        self.assertEqual(
            message.encode("utf-8", errors="surrogatepass"),
            decoded_error.message.encode("utf-8", errors="surrogatepass"),
        )
        self.assertEqual(encoded, encode_verdict(decoded))
        document = json.loads(encoded)
        self.assertTrue(document["properties"][4]["error"]["oversize"])
        tampered = json.loads(encoded)
        tampered["properties"][4]["error"]["oversize"] = False
        with self.assertRaisesRegex(ValueError, "oversize marker"):
            read_verdict_document(tampered)
        clipped = json.loads(encoded)
        clipped["properties"][4]["error"]["message"] = message[:4096]
        with self.assertRaisesRegex(ValueError, "error message is 4096 bytes"):
            read_verdict_document(clipped)

    def test_oversized_type_is_kept_and_named_by_digest(self) -> None:
        long_type = type("E" * 200, (Exception,), {})

        def odd(_input: CheckInput, sink: CollectorSink) -> None:
            raise long_type("m")

        verdict = check(t.target(t.prop("odd", odd)), self._SPEC, t.READ_OK)
        error = t.result(verdict, "odd").error
        assert error is not None
        self.assertEqual(("E" * 200, True), (error.type, error.oversize))
        self.assertRegex(
            verdict.reason,
            r"^property-error/odd≠checker: type:sha256:[0-9a-f]{64} \(oversize\)$",
        )
        self.assertEqual(verdict, decode_verdict(encode_verdict(verdict)))


class _Hostile(Exception):
    def __str__(self) -> str:
        raise RuntimeError("str exploded")


class _NotText(Exception):
    def __str__(self) -> str:
        return cast(Any, 7)


class _HostileRepr:
    def __repr__(self) -> str:
        raise RuntimeError("repr exploded")


class HostileInputTest(unittest.TestCase):
    """No property output, exception, execution, or spec escapes `check`."""

    _SPEC = t.spec([t.action("r", "read")])

    def _round_trip(self, verdict: Verdict, limit: int = 256 * 1024) -> bytes:
        data = encode_verdict(verdict)
        self.assertLess(len(data), limit)
        self.assertEqual(
            verdict_document(verdict), verdict_document(decode_verdict(data))
        )
        return data

    def test_hostile_exception_text_is_a_property_error(self) -> None:
        def hostile(_input: CheckInput, sink: CollectorSink) -> None:
            raise _Hostile()

        def not_text(_input: CheckInput, sink: CollectorSink) -> None:
            raise _NotText()

        cases = {
            "collect __str__ raises": (
                t.prop("hostile", hostile),
                ("_Hostile", "<unprintable _Hostile>"),
            ),
            "collect __str__ returns non-text": (
                t.prop("hostile", not_text),
                ("_NotText", "<unprintable _NotText>"),
            ),
        }
        for name, (hostile_property, expected) in cases.items():
            with self.subTest(case=name):
                verdict = check(
                    t.target(hostile_property, t.quiet("sibling")),
                    self._SPEC,
                    t.READ_OK,
                )
                error = t.result(verdict, "hostile").error
                assert error is not None
                self.assertEqual(expected, (error.type, error.message))
                self.assertEqual(1, len(t.result(verdict, "sibling").obligations))
                self.assertEqual(PROPERTY_ERROR, verdict.not_evaluated_cause)
                self._round_trip(verdict)

    def test_hostile_values_given_to_the_sink_stay_sticky(self) -> None:
        def swallows(value: Any) -> t.Collect:
            def body(_input: CheckInput, sink: CollectorSink) -> None:
                try:
                    sink.covered("p.seen", value)
                except Exception:
                    pass
                sink.satisfied(sink.obligation("p.bad", "a", subject=t.PATH))

            return body

        for name, value, message in (
            ("hostile repr", _HostileRepr(), "got <_HostileRepr>"),
            ("huge count", -(10**5000), "got <int of 16610 bits>"),
        ):
            with self.subTest(case=name):
                definition = t.prop(
                    "p",
                    swallows(value),
                    coverage=(CoverageObligation("p.seen", "seen"),),
                )
                verdict = check(t.target(definition), self._SPEC, t.READ_OK)
                error = t.result(verdict, "p").error
                assert error is not None
                self.assertEqual("PropertyContractError", error.type)
                self.assertIn(message, error.message)
                self._round_trip(verdict)

    def test_fuzzed_hostile_inputs_never_escape(self) -> None:
        rng = random.Random(918)
        values: list[Any] = [
            10**5000,
            -(10**5000),
            math.nan,
            math.inf,
            2**53 + 1,
            "\ud800",
            "x" * 5000,
            None,
            True,
            [1, [2, [3]]],
            {"k": 10**5000},
            {1: "a"},
            _HostileRepr(),
            object(),
        ]

        def hostile_property(seed: int) -> PropertyDef:
            value = values[seed % len(values)]

            def body(_input: CheckInput, sink: CollectorSink) -> None:
                choice = seed % 6
                if choice == 0:
                    raise _Hostile()
                if choice == 1:
                    sink.obligation("p.bad", cast(Any, value), subject=t.PATH)
                ref = sink.obligation("p.bad", "a", subject=t.PATH)
                if choice == 2:
                    sink.violated(ref, cast(Any, value), "p/x≠y: bad")
                if choice == 3:
                    raise RuntimeError("m" * (seed * 997 % 200000))
                if choice == 4:
                    sink.not_evaluated(ref, cast(Any, value))
                sink.satisfied(ref)

            return t.prop("p", body)

        for iteration in range(400):
            seed = rng.randrange(10**6)
            value = values[seed % len(values)]
            data: dict[str, Any] = {"action": "r", "fact": value}
            if seed % 3 == 0:
                data[rng.choice(["type", "time"])] = value
            events = [
                Event(INVOKE, rng.choice([1.0, value]), {"action": "r"}),
                Event(OK, 2.0, data),
            ]
            spec = self._SPEC
            if seed % 4 == 0:
                spec = replace(spec, seed=cast(Any, value))
            execution = rng.choice([None, {"status": "completed"}, {"status": value}])
            with self.subTest(iteration=iteration, seed=seed):
                verdict = check(
                    t.target(hostile_property(seed), t.quiet("sibling")),
                    spec,
                    events,
                    execution=execution,
                )
                self._round_trip(verdict, limit=2 * 1024 * 1024)
                # Hostile input is judged; it never becomes a checker defect.
                gate = verdict.report.gate if verdict.report is not None else ()
                self.assertFalse([p for p in gate if "core/checker.py:" in p])
