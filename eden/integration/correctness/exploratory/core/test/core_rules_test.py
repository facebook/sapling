# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The core properties every target shares besides input legality: a daemon exit
outside a declared window rejects (one before any modeled action is a crash,
one during close is not), a run that reached the target must exercise it with
modeled operations, and the result-schema property judges each decoded action
result (a bad result is a harness or reference violation; an undecodable
failure leaves input legality not evaluated).
"""

from __future__ import annotations

import unittest
from dataclasses import replace
from typing import Any

from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    BLOCKED,
    check,
    CORE_ACTIVITY,
    CORE_DAEMON_EXIT,
    CORE_INPUT_LEGALITY,
    CORE_RESULT_SCHEMA,
    decode_verdict,
    encode_verdict,
    HARNESS_SUSPECT,
    NOT_EVALUATED,
    REJECT,
)
from eden.integration.correctness.exploratory.core.facts import (
    FieldKind,
    FieldSpec,
    HistoryFacts,
    MISSING_REFERENCE,
    paired,
    ResultSchema,
    UNEXPECTED_REFERENCE,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    Event,
    FAIL,
    INFO,
    INVOKE,
    OK,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    ObligationStatus,
    Witness,
)
from eden.integration.correctness.exploratory.core.target import (
    Binding,
    Grouping,
    Role,
    Target,
)
from eden.integration.correctness.exploratory.core.test import checker_testing as t


_SIZED = paired(
    ResultSchema(
        ok=(FieldSpec("size", FieldKind.INT),),
        fail=(FieldSpec("errno", FieldKind.INT),),
    )
)


class CoreRulesTest(unittest.TestCase):
    def test_daemon_exit_outside_window_rejects(self) -> None:
        """Every unowned daemon exit before cleanup is a crash."""
        spec = t.spec([t.action("write", "write")])
        prefix = [t.event(INVOKE, "write", 1.0), t.event(OK, "write", 2.0)]
        exit_event = Event(
            type=INFO,
            time=3.0,
            data={"reason": "daemon-exit", "returncode": -11, "signal": "SIGSEGV"},
        )
        rejected = check(t.input_target(), spec, [*prefix, exit_event])
        self.assertEqual(REJECT, rejected.kind)
        self.assertEqual(
            "daemon/crashed: exit -11 (SIGSEGV) at t=3.0; no nemesis window",
            rejected.reason,
        )
        (finding,) = t.result(rejected, CORE_DAEMON_EXIT).findings
        self.assertEqual("daemon:all", finding.subject.token)
        self.assertEqual(Witness("exit-class", "signal:SIGSEGV"), finding.witnesses[0])

    def test_daemon_exit_before_any_modeled_action_is_a_crash(self) -> None:
        """A crash during a leading sleep wins over the modeled-evidence rule."""
        spec = t.spec([t.action("z", "sleep", seconds=1.0), t.action("write", "write")])
        events = [
            t.event(INVOKE, "z", 1.0),
            Event(
                type=INFO,
                time=2.0,
                data={"reason": "daemon-exit", "returncode": 1, "signal": "SIGSEGV"},
            ),
        ]
        rejected = check(t.input_target(), spec, events)
        self.assertEqual(REJECT, rejected.kind)
        self.assertEqual(
            "daemon/crashed: exit 1 (SIGSEGV) at t=2.0; no nemesis window",
            rejected.reason,
        )
        self.assertEqual(1, len(t.result(rejected, CORE_ACTIVITY).findings))

    def test_daemon_exit_during_close_is_not_a_crash(self) -> None:
        """Shutdown after the cleanup event is the harness's own doing."""
        spec = t.spec([t.action("write", "write")])
        events = [
            t.event(INVOKE, "write", 1.0),
            t.event(OK, "write", 2.0),
            Event(type=CLEANUP, time=3.0, data={"unwind": {}, "close_error": None}),
            Event(
                type=INFO,
                time=4.0,
                data={"reason": "daemon-exit", "returncode": -9, "signal": "SIGKILL"},
            ),
        ]
        verdict = check(t.input_target(), spec, events)
        self.assertEqual(ACCEPT, verdict.kind)
        self.assertEqual(
            [("run", "satisfied")], t.statuses(t.result(verdict, CORE_DAEMON_EXIT))
        )

    def test_an_unmodeled_mutation_exercises_the_target(self) -> None:
        """A completed action whose static roles mutate state satisfies `core.activity`.

        `move` has a checkout's shape (unmodeled input, aggregate binding,
        progressive grouping, mutating role), so a history whose only
        completed action is a checkout is exercised. An unmodeled action
        without mutating roles still does not.
        """
        mutation = replace(
            t.action_def("move", "input"),
            roles=frozenset({Role.PRODUCE}),
            binding=Binding.AGGREGATE,
            grouping=Grouping.PROGRESSIVE,
            footprint=t.root_footprint,
        )
        target = t.target(
            t.quiet(),
            actions={"move": mutation, "peek": t.action_def("peek", "input")},
        )
        for kind, expected in (("move", ACCEPT), ("peek", REJECT)):
            with self.subTest(kind=kind):
                spec = t.spec([t.action("x", kind)])
                events = [t.event("invoke", "x", 1.0), t.event("ok", "x", 2.0)]
                verdict = check(target, spec, events)
                self.assertEqual(expected, verdict.kind, verdict.reason)
                activity = t.report(verdict).results[1]
                self.assertEqual(CORE_ACTIVITY, activity.property_id)

    def test_activity_is_not_evaluated_for_incomplete_executions(self) -> None:
        """An execution that did not complete cannot show that no operation would.

        Without an exercising ok/fail completion, `core.activity` is not
        evaluated when the execution did not complete, whether or not it
        invoked an action or the sweep completed one as info; with only info
        completions it is not evaluated either; it is violated only when
        nothing completed and the execution completed or is unknown. An
        exercising ok/fail completion satisfies it either way.
        """
        spec = t.spec([t.action("r", "read")])
        target = t.target(t.quiet())
        swept = [
            t.event(INVOKE, "r", 1.0),
            t.event(INFO, "r", 2.0, reason="runner-deadline"),
        ]
        histories = {
            "empty": [],
            "invoked": [t.event(INVOKE, "r", 1.0)],
            "swept": swept,
        }
        for status in (
            "harness-failure",
            "environment-degraded",
            "containment-timeout",
        ):
            for name, events in histories.items():
                with self.subTest(status=status, history=name):
                    verdict = check(target, spec, events, execution={"status": status})
                    self.assertEqual(
                        (
                            NOT_EVALUATED,
                            BLOCKED,
                            f"blocked/core.activity.incomplete: execution {status}",
                        ),
                        (verdict.kind, verdict.not_evaluated_cause, verdict.reason),
                    )
                    activity = t.report(verdict).results[1]
                    self.assertEqual(CORE_ACTIVITY, activity.property_id)
                    (obligation,) = activity.obligations
                    self.assertEqual(
                        (ObligationStatus.NOT_EVALUATED, "core.activity.incomplete"),
                        (obligation.status, obligation.not_evaluated_reason),
                    )
                    self.assertEqual(verdict, decode_verdict(encode_verdict(verdict)))
        unknown = check(target, spec, [])
        self.assertEqual(REJECT, unknown.kind)
        only_info = check(target, spec, swept)
        self.assertEqual(
            "blocked/core.activity.incomplete: only info completions",
            only_info.reason,
        )
        completed = check(
            target, spec, t.READ_OK, execution={"status": "harness-failure"}
        )
        activity = t.report(completed).results[1]
        (obligation,) = activity.obligations
        self.assertEqual(ObligationStatus.SATISFIED, obligation.status)

    def test_no_modeled_operations_rejects(self) -> None:
        """A run promises evidence from at least one completed modeled operation."""
        spec = t.spec([t.action("r", "read")])
        rejected = check(t.target(t.quiet()), spec, [])
        accepted = check(t.target(t.quiet()), spec, t.READ_OK)

        self.assertEqual(REJECT, rejected.kind)
        self.assertEqual(
            "no-modeled-operations/history≠spec: no modeled action completed in history",
            rejected.reason,
        )
        self.assertEqual({"quiet": ACCEPT}, rejected.lanes)
        self.assertEqual(ACCEPT, accepted.kind)


class ResultSchemaTest(unittest.TestCase):
    """Recorder problems on modeled results are direct harness violations."""

    def _target(self, *, modeled: bool = True, owed: bool | None = None) -> Target:
        read = t.action_def("read", "input", modeled=modeled, result=_SIZED)
        consumer = replace(
            t.quiet(), consumes=frozenset({("read", "size"), ("read", "errno")})
        )
        return t.target(
            consumer,
            actions={"read": read},
            expects_reference=(None if owed is None else lambda _spec, _action: owed),
        )

    def test_decoded_results_satisfy_and_bad_results_violate_as_harness(self) -> None:
        spec = t.spec([t.action("r", "read")])
        cases = {
            "declared": ({"size": 3}, None),
            "undeclared field": (
                {"size": 3, "extra": 1},
                "result-schema/read≠schema: r ok: result: undeclared fields ['extra']",
            ),
            "missing field": (
                {},
                "result-schema/read≠schema: r ok: size: required field is missing",
            ),
            "bool is not int": (
                {"size": True},
                "result-schema/read≠schema: r ok: size: True is not int",
            ),
        }
        for name, (facts, reason) in cases.items():
            with self.subTest(case=name):
                events = [t.event(INVOKE, "r", 1.0), t.event(OK, "r", 2.0, **facts)]
                verdict = check(self._target(), spec, events)
                (obligation,) = t.result(verdict, CORE_RESULT_SCHEMA).obligations
                if reason is None:
                    self.assertEqual(ObligationStatus.SATISFIED, obligation.status)
                    self.assertEqual(ACCEPT, verdict.kind)
                    continue
                self.assertEqual(
                    (Attribution.HARNESS, reason),
                    (obligation.attribution, obligation.reason),
                )
                self.assertEqual(
                    Witness("action-kind", "read"), obligation.witnesses[0]
                )
                self.assertEqual(
                    (HARNESS_SUSPECT, reason), (verdict.kind, verdict.reason)
                )

    def test_a_bad_reference_side_is_a_reference_violation(self) -> None:
        spec = t.spec([t.action("r", "read")])

        def side(facts: dict[str, Any]) -> dict[str, Any]:
            return {
                "schema_version": 1,
                "outcome": "ok",
                "started": 1.1,
                "completed": 1.9,
                "facts": facts,
            }

        reason = (
            "result-schema/read≠schema: r reference ok: result: undeclared "
            "fields ['extra']"
        )
        for modeled in (True, False):
            with self.subTest(modeled=modeled):
                events = [
                    t.event(INVOKE, "r", 1.0),
                    t.event(
                        OK,
                        "r",
                        2.0,
                        size=3,
                        reference_result=side({"size": 3, "extra": 1}),
                    ),
                ]
                verdict = check(self._target(modeled=modeled, owed=True), spec, events)
                (obligation,) = t.result(verdict, CORE_RESULT_SCHEMA).obligations
                self.assertEqual(
                    (Attribution.REFERENCE, reason),
                    (obligation.attribution, obligation.reason),
                )
                self.assertIn(Witness("side", "reference"), obligation.witnesses)
                if modeled:
                    self.assertEqual(HARNESS_SUSPECT, verdict.kind)
        events = [
            t.event(INVOKE, "r", 1.0),
            t.event(OK, "r", 2.0, size=3, reference_result=side({"size": 3})),
        ]
        verdict = check(self._target(owed=True), spec, events)
        self.assertEqual(
            [("r", "satisfied")], t.statuses(t.result(verdict, CORE_RESULT_SCHEMA))
        )
        both = [
            t.event(INVOKE, "r", 1.0),
            t.event(OK, "r", 2.0, reference_result=side({"extra": 1})),
        ]
        (obligation,) = t.result(
            check(self._target(owed=True), spec, both), CORE_RESULT_SCHEMA
        ).obligations
        self.assertEqual(Attribution.HARNESS, obligation.attribution)

    def test_reference_presence_is_judged_against_expects_reference(self) -> None:
        """A missing owed side is the reference's; an unowed side is the harness's."""
        spec = t.spec([t.action("r", "read")])
        side = {
            "schema_version": 1,
            "outcome": "ok",
            "started": 1.1,
            "completed": 1.9,
            "facts": {"size": 3},
        }
        cases = {
            "missing": (
                True,
                {},
                Attribution.REFERENCE,
                f"result-schema/read≠schema: r reference None: {MISSING_REFERENCE}",
            ),
            "unexpected": (
                False,
                {"reference_result": side},
                Attribution.HARNESS,
                f"result-schema/read≠schema: r reference None: {UNEXPECTED_REFERENCE}",
            ),
        }
        for name, (owed, extra, attribution, reason) in cases.items():
            for modeled in (True, False):
                with self.subTest(case=name, modeled=modeled):
                    events = [
                        t.event(INVOKE, "r", 1.0),
                        t.event(OK, "r", 2.0, size=3, **extra),
                    ]
                    verdict = check(
                        self._target(modeled=modeled, owed=owed), spec, events
                    )
                    (obligation,) = t.result(verdict, CORE_RESULT_SCHEMA).obligations
                    self.assertEqual(
                        (attribution, reason),
                        (obligation.attribution, obligation.reason),
                    )
                    if modeled:
                        self.assertEqual(
                            (HARNESS_SUSPECT, reason), (verdict.kind, verdict.reason)
                        )
        for owed, extra in ((True, {"reference_result": side}), (False, {})):
            with self.subTest(owed=owed):
                events = [
                    t.event(INVOKE, "r", 1.0),
                    t.event(OK, "r", 2.0, size=3, **extra),
                ]
                verdict = check(self._target(owed=owed), spec, events)
                self.assertEqual(
                    [("r", "satisfied")],
                    t.statuses(t.result(verdict, CORE_RESULT_SCHEMA)),
                )

    def test_unmodeled_results_are_not_obligations(self) -> None:
        spec = t.spec([t.action("r", "read")])
        events = [t.event(INVOKE, "r", 1.0), t.event(OK, "r", 2.0, junk=1)]
        target = self._target(modeled=False)
        verdict = check(target, spec, events)
        self.assertEqual((), t.result(verdict, CORE_RESULT_SCHEMA).obligations)
        recorded = HistoryFacts.build(spec, tuple(events), target).action("r")
        self.assertIsNone(recorded.result)
        self.assertEqual(
            (
                "result: undeclared fields ['junk']",
                "size: required field is missing",
            ),
            recorded.problems,
        )

    def test_undecodable_failure_leaves_input_legality_not_evaluated(self) -> None:
        spec = t.spec([t.action("r", "read")])
        events = [t.event(INVOKE, "r", 1.0), t.event(FAIL, "r", 2.0, errno="EIO")]
        verdict = check(self._target(), spec, events)
        (legality,) = t.result(verdict, CORE_INPUT_LEGALITY).obligations
        self.assertEqual(
            (ObligationStatus.NOT_EVALUATED, "core.input-legality.undecoded"),
            (legality.status, legality.not_evaluated_reason),
        )
        self.assertEqual(HARNESS_SUSPECT, verdict.kind)

    def test_scheduler_info_reason_decodes_on_every_schema(self) -> None:
        spec = t.spec([t.action("r", "read")])
        events = [
            t.event(INVOKE, "r", 1.0),
            t.event(INFO, "r", 2.0, reason="operation-timeout"),
        ]
        verdict = check(self._target(), spec, events)
        self.assertEqual(
            [("r", "satisfied")], t.statuses(t.result(verdict, CORE_RESULT_SCHEMA))
        )
