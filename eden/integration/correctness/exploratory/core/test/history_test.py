# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The history `check` judges: action intervals, order and nemesis windows come
from the events (a process nemesis window lasts until its observed exit), and a
malformed execution document, an out-of-bounds spec, or any other gate problem
stops the history before property evaluation, kept whole however large it is.
"""

from __future__ import annotations

import math
import unittest
from dataclasses import replace
from typing import Any

from eden.integration.correctness.exploratory.core import history as history_module
from eden.integration.correctness.exploratory.core.checker import (
    bind,
    check,
    decode_verdict,
    encode_verdict,
    INVALID_HISTORY,
    INVALID_RUNSPEC,
    NOT_EVALUATED,
    Verdict,
    verdict_document,
    VERDICT_OPERATIONAL_BYTES,
)
from eden.integration.correctness.exploratory.core.history import (
    Event,
    FAIL,
    INFO,
    INVOKE,
    OK,
)
from eden.integration.correctness.exploratory.core.test import checker_testing as t


_KILL = replace(t.action_def("kill_process", "nemesis"), kills_process=True)


class HistoryTest(unittest.TestCase):
    def test_process_nemesis_windows_follow_metadata(self) -> None:
        """A running process lease remains disruptive until its observed exit."""
        actions = [
            t.action("load", "load"),
            t.action("input", "input"),
            t.action("idle", "idle"),
        ]
        definitions = {
            kind: replace(t.action_def(kind, axis), spawns_process=spawns)
            for kind, axis, spawns in (
                ("load", "nemesis", True),
                ("input", "input", True),
                ("idle", "nemesis", False),
            )
        }
        prefix = [t.event(INVOKE, action.id, 1.0) for action in actions]
        for outcome in (OK, FAIL, INFO, None):
            with self.subTest(outcome=outcome):
                events = prefix + (
                    []
                    if outcome is None
                    else [t.event(outcome, action.id, 2.0) for action in actions]
                )
                history = history_module.History(t.spec(actions), events, definitions)
                end = 2.0 if outcome in (OK, FAIL) else float("inf")
                self.assertEqual([(1.0, end)], history.nemesis_windows())
        self.assertEqual(
            [],
            history_module.History(t.spec(actions), [], definitions).nemesis_windows(),
        )

    def test_history_intervals_and_order(self) -> None:
        actions = [
            t.action("a", "write"),
            t.action("b", "read"),
            t.action("c", "read"),
        ]
        events = [
            t.event(INVOKE, "a", 1.0),
            t.event(OK, "a", 3.0, bytes=7),
            t.event(INVOKE, "b", 3.0),
            t.event(OK, "b", 5.0),
            t.event(INVOKE, "c", 6.0),
            t.event(OK, "c", 8.0),
        ]
        action_defs = {
            "write": t.action_def("write", "input"),
            "read": t.action_def("read", "input"),
        }

        history = history_module.History(t.spec(actions), events, action_defs)

        self.assertEqual((1.0, 3.0), history.interval("a"))
        self.assertEqual(OK, history.outcome("a"))
        self.assertEqual({"bytes": 7}, history.facts("a"))
        self.assertFalse(history.completed_before("a", "b"))
        self.assertTrue(history.overlaps("a", "b"))
        self.assertTrue(history.completed_before("a", "c"))
        self.assertFalse(history.overlaps("a", "c"))
        self.assertEqual(tuple(events), history.events)

    def test_history_nemesis_windows(self) -> None:
        actions = [
            t.action("kill", "kill_process"),
            t.action("install", "fault_install", fault_id="f"),
            t.action("remove", "fault_remove", fault_id="f"),
            t.action("input", "kill_target_input"),
        ]
        events = [
            t.event(INVOKE, "kill", 1.0),
            t.event(OK, "kill", 4.0),
            t.event(INVOKE, "install", 5.0),
            t.event(OK, "install", 6.0),
            t.event(INVOKE, "input", 6.5),
            t.event(OK, "input", 7.0),
            t.event(INVOKE, "remove", 8.0),
            t.event(OK, "remove", 9.0),
        ]
        action_defs = {
            "kill_process": _KILL,
            "fault_install": t.action_def("fault_install", "nemesis"),
            "fault_remove": t.action_def("fault_remove", "nemesis"),
            "kill_target_input": t.action_def("kill_target_input", "input"),
        }

        history = history_module.History(t.spec(actions), events, action_defs)

        self.assertEqual([(1.0, 4.0), (5.0, 9.0)], history.nemesis_windows())

    def test_kill_window_follows_the_declaration_and_stays_open(self) -> None:
        """A declared kill that never completed ok/fail keeps its window open;
        a nemesis whose kind merely starts with `kill_` opens none."""
        actions = [
            t.action("invoked", "kill_process"),
            t.action("swept", "kill_process"),
            t.action("named", "kill_lookalike"),
        ]
        events = [
            t.event(INVOKE, "invoked", 1.0),
            t.event(INVOKE, "swept", 2.0),
            t.event(INFO, "swept", 3.0),
            t.event(INVOKE, "named", 4.0),
            t.event(OK, "named", 5.0),
        ]
        action_defs = {
            "kill_process": _KILL,
            "kill_lookalike": t.action_def("kill_lookalike", "nemesis"),
        }

        history = history_module.History(t.spec(actions), events, action_defs)

        self.assertEqual([(1.0, math.inf), (2.0, math.inf)], history.nemesis_windows())


class HistoryGateTest(unittest.TestCase):
    _SPEC = t.spec([t.action("r", "read")])

    def _round_trip(self, verdict: Verdict, limit: int = 256 * 1024) -> bytes:
        data = encode_verdict(verdict)
        self.assertLess(len(data), limit)
        self.assertEqual(
            verdict_document(verdict), verdict_document(decode_verdict(data))
        )
        return data

    def test_malformed_execution_documents_gate_the_history(self) -> None:
        partial = [t.event(INVOKE, "r", 1.0)]
        cases: dict[str, Any] = {
            "empty": {},
            "integer status": {"status": 5},
            "null status": {"status": None},
            "list status": {"status": ["completed"]},
            "huge status": {"status": 10**5000},
            "empty status": {"status": ""},
            "long status": {"status": "x" * 129},
            "not an object": ["completed"],
        }
        for name, execution in cases.items():
            with self.subTest(execution=name):
                verdict = check(
                    t.target(t.quiet()), self._SPEC, partial, execution=execution
                )
                self.assertEqual(
                    (NOT_EVALUATED, INVALID_HISTORY),
                    (verdict.kind, verdict.not_evaluated_cause),
                )
                self.assertIn("execution", verdict.reason)
                self._round_trip(verdict)
        unknown = check(t.target(t.quiet()), self._SPEC, partial, execution=None)
        self.assertNotEqual(INVALID_HISTORY, unknown.not_evaluated_cause)

    def test_every_gate_problem_is_kept_whole_past_the_operational_threshold(
        self,
    ) -> None:
        """20,000 unknown 700-byte action IDs keep all 20,000 problems."""
        events = [
            Event(INVOKE, 1.0, {"action": "x" * 700 + str(index)})
            for index in range(20000)
        ]
        problems = history_module.history_problems(self._SPEC, events)
        verdict = check(t.target(t.quiet()), self._SPEC, events)
        report = t.report(verdict)
        self.assertEqual(tuple(problems), report.gate)
        self.assertGreaterEqual(len(problems), 20000)
        self.assertEqual(
            f"invalid-history/history≠spec: {problems[0]} (+{len(problems) - 1} more)",
            verdict.reason,
        )
        data = encode_verdict(verdict)
        self.assertGreater(len(data), 14 * 1024 * 1024)
        self.assertGreater(len(data), VERDICT_OPERATIONAL_BYTES)
        decoded = decode_verdict(data)
        self.assertEqual(verdict_document(verdict), verdict_document(decoded))
        self.assertEqual(report.gate, t.report(decoded).gate)
        self.assertEqual(data, encode_verdict(decoded))

    def test_an_oversized_problem_is_kept_whole(self) -> None:
        action = "y" * (400 * 1024)
        verdict = check(
            t.target(t.quiet()), self._SPEC, [Event(INVOKE, 1.0, {"action": action})]
        )
        (problem,) = t.report(verdict).gate
        self.assertIn(action, problem)
        self.assertIn(action, verdict.reason)
        self._round_trip(verdict, limit=2 * 1024 * 1024)

    def test_out_of_bounds_specs_gate_as_invalid_runspec(self) -> None:
        """`bind` gates an in-memory huge seed as an invalid RunSpec."""
        action = self._SPEC.actions[0]
        specs = {
            "huge seed": replace(self._SPEC, seed=10**5000),
            "huge budget": replace(
                self._SPEC,
                budgets=replace(
                    self._SPEC.budgets, runner_deadline_seconds=-(10**5000)
                ),
            ),
            "huge payload": replace(
                self._SPEC, actions=(replace(action, payload={"n": 10**5000}),)
            ),
            "nan metadata": replace(self._SPEC, metadata={"x": math.nan}),
        }
        digests = set()
        for name, spec in specs.items():
            with self.subTest(spec=name):
                verdict = check(t.target(t.quiet()), spec, t.READ_OK)
                self.assertEqual(
                    (NOT_EVALUATED, INVALID_RUNSPEC),
                    (verdict.kind, verdict.not_evaluated_cause),
                )
                runspec = bind(spec, t.READ_OK, None).runspec
                self.assertEqual(runspec, t.report(verdict).bindings.runspec)
                digests.add(runspec)
                self._round_trip(verdict)
        self.assertEqual(len(specs), len(digests))
        self.assertNotIn(bind(self._SPEC, t.READ_OK, None).runspec, digests)

    def test_a_non_dict_execution_never_shares_a_valid_execution_digest(
        self,
    ) -> None:
        not_a_dict: Any = ["completed"]
        invalid = bind(self._SPEC, t.READ_OK, not_a_dict).execution
        self.assertIsNotNone(invalid)
        for execution in ({"status": "<list>"}, {"status": "completed"}, {}):
            with self.subTest(execution=execution):
                self.assertNotEqual(
                    invalid, bind(self._SPEC, t.READ_OK, execution).execution
                )
