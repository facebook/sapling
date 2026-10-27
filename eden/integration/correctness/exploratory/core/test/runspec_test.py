# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import json
import math
import re
import unittest
from dataclasses import fields, replace
from typing import Any, cast

from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    DAEMON_EXIT,
    Event,
    event_from_json,
    event_line,
    event_to_json,
    FAIL,
    History,
    history_problems,
    INFO,
    INVOKE,
    is_event_time,
    MAX_EVENT_BYTES,
    MAX_EVENT_TIME,
    OK,
    SKIPPED,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    DEFAULT_BUDGETS,
    Environment,
    from_json as _from_json,
    MAX_ACTIONS,
    MAX_ACTORS,
    MAX_INTEGER,
    MAX_JSON_DEPTH,
    MAX_RUNSPEC_BYTES,
    must_end_before,
    RunSpec,
    spec_problems,
    SpecError,
    STARTED,
    strict_loads,
    to_json,
    validate,
)
from eden.integration.correctness.exploratory.core.test.testing import (
    ev,
    minimal_spec as _minimal_spec,
    TOY_TARGET,
)


_MINIMAL_ACTIONS = {
    "sleep": {"seconds": 1.0},
}


def _minimal_action(kind: str) -> dict:
    return {"kind": kind, "deadline": "operation", **_MINIMAL_ACTIONS[kind]}


def from_json(text: str) -> RunSpec:
    return _from_json(text, TOY_TARGET)


class RunSpecTest(unittest.TestCase):
    def test_budgets_reject_nonfinite_and_nonpositive_values(self) -> None:
        """Watchdog budgets must be finite and positive.

        test_budget_order_accepts_operation_at_actor_bound is the accept twin.
        """
        raw = json.loads(
            _minimal_spec(
                {"a": _minimal_action("sleep")},
                [{"id": "actor", "actions": ["a"]}],
                target_name=TOY_TARGET.name,
                environment_target={},
            )
        )
        cases = (
            (float("nan"), "NaN is not a JSON number"),
            (float("inf"), "Infinity is not a JSON number"),
            (-float("inf"), "-Infinity is not a JSON number"),
            (0, "finite and positive"),
            (-1, "finite and positive"),
        )
        for name in raw["budgets"]:
            for value, message in cases:
                with self.subTest(name=name, value=value):
                    candidate = {**raw, "budgets": {**raw["budgets"], name: value}}
                    with self.assertRaisesRegex(SpecError, message):
                        from_json(json.dumps(candidate))

    def test_budget_order_rejects_unreserved_cleanup(self) -> None:
        """Cleanup is reserved within the absolute runner budget.

        test_budget_order_accepts_operation_at_actor_bound is the accept twin.
        """
        raw = json.loads(
            _minimal_spec(
                {"a": _minimal_action("sleep")},
                [{"id": "actor", "actions": ["a"]}],
                target_name=TOY_TARGET.name,
                environment_target={},
            )
        )
        for operation, cleanup, runner in ((1, 2, 2), (1, 3, 2), (2, 1, 2)):
            with self.subTest(operation=operation, cleanup=cleanup, runner=runner):
                raw["budgets"] = {
                    "operation_deadline_seconds": operation,
                    "cleanup_deadline_seconds": cleanup,
                    "runner_deadline_seconds": runner,
                }
                with self.assertRaises(SpecError):
                    from_json(json.dumps(raw))

    def test_budget_order_accepts_operation_at_actor_bound(self) -> None:
        """Equality at the reserved actor bound is permitted."""
        raw = json.loads(
            _minimal_spec(
                {"a": _minimal_action("sleep")},
                [{"id": "actor", "actions": ["a"]}],
                target_name=TOY_TARGET.name,
                environment_target={},
            )
        )
        raw["budgets"] = {
            "operation_deadline_seconds": 1,
            "cleanup_deadline_seconds": 1,
            "runner_deadline_seconds": 2,
        }
        self.assertEqual(from_json(json.dumps(raw)).budgets, Budgets(1, 1, 2))

    def test_omitted_budgets_take_the_one_default_run_deadline(self) -> None:
        """One default whole-run deadline; omitted slices are its fixed fractions."""
        raw = json.loads(
            _minimal_spec(
                {"a": _minimal_action("sleep")},
                [{"id": "actor", "actions": ["a"]}],
                target_name=TOY_TARGET.name,
                environment_target={},
            )
        )
        self.assertEqual(Budgets(120, 30, 600), DEFAULT_BUDGETS)
        cases = (
            (None, DEFAULT_BUDGETS),
            ({}, DEFAULT_BUDGETS),
            ({"runner_deadline_seconds": 1200}, Budgets(240, 60, 1200)),
            (
                {"runner_deadline_seconds": 100, "cleanup_deadline_seconds": 10},
                Budgets(20, 10, 100),
            ),
            ({"operation_deadline_seconds": 30}, Budgets(30, 30, 600)),
        )
        for budgets, expected in cases:
            with self.subTest(budgets=budgets):
                candidate = dict(raw)
                del candidate["budgets"]
                if budgets is not None:
                    candidate["budgets"] = budgets
                spec = from_json(json.dumps(candidate))
                self.assertEqual(expected, spec.budgets)
                # The resolved deadlines are written out, so the spec round-trips.
                self.assertEqual(spec, from_json(to_json(spec)))
        candidate = {**raw, "budgets": {"runner_deadline_seconds": 10}}
        candidate["budgets"]["cleanup_deadline_seconds"] = 9
        with self.assertRaisesRegex(SpecError, "operation budget must fit"):
            from_json(json.dumps(candidate))

    def test_action_has_payload_only(self) -> None:
        self.assertEqual(
            [field.name for field in fields(Action)],
            ["id", "kind", "deadline", "requires", "payload"],
        )

    def test_bad_json_rejects(self) -> None:
        with self.assertRaises(SpecError):
            from_json("{not json")

    def test_cycle_error_names_the_path_from_the_first_root(self) -> None:
        actions = {
            "a": _minimal_action("sleep"),
            "b": {
                **_minimal_action("sleep"),
                "requires": [["a", "finished"], ["c", "finished"]],
            },
            "c": _minimal_action("sleep"),
            "d": {**_minimal_action("sleep"), "requires": [["c", "finished"]]},
        }
        spec = _minimal_spec(
            actions,
            [{"id": "p", "actions": ["a"]}, {"id": "q", "actions": ["b", "c", "d"]}],
            target_name=TOY_TARGET.name,
            environment_target={},
        )
        with self.assertRaisesRegex(SpecError, r"^dependency cycle: a -> b -> c -> b$"):
            from_json(spec)

    def test_validate_refuses_an_empty_action_list(self) -> None:
        """A programmatic spec is held to the same rule as a parsed one."""
        spec = from_json(
            _minimal_spec(
                {"a": _minimal_action("sleep")},
                [{"id": "p", "actions": ["a"]}],
                target_name=TOY_TARGET.name,
                environment_target={},
            )
        )
        with self.assertRaisesRegex(SpecError, "^actions must be non-empty$"):
            validate(replace(spec, actions=(), actors=()), TOY_TARGET)

    def test_999_action_chain_validates(self) -> None:
        ids = [f"s{index}" for index in range(999)]
        spec = _minimal_spec(
            {action_id: _minimal_action("sleep") for action_id in ids},
            [{"id": "chain", "actions": ids}],
            target_name=TOY_TARGET.name,
            environment_target={},
        )
        parsed = from_json(spec)
        self.assertEqual(999, len(parsed.actions))
        self.assertEqual(tuple(ids), parsed.actors[0].actions)

    def test_actor_order_ends_the_previous_action_before_the_next_starts(
        self,
    ) -> None:
        def sleep(action_id: str, **kwargs: Any) -> Action:
            return Action(
                id=action_id,
                kind="sleep",
                deadline=DEADLINE_OPERATION,
                payload={},
                **kwargs,
            )

        spec = RunSpec(
            seed=1,
            target=TOY_TARGET.name,
            environment=Environment(target={}),
            budgets=Budgets(1, 1, 3),
            actors=(
                Actor(id="a", actions=("a1", "a2")),
                Actor(id="b", actions=("b1",)),
            ),
            actions=(
                sleep("a1"),
                sleep("a2"),
                sleep("b1", requires=(("a2", STARTED),)),
            ),
        )
        before = must_end_before(spec)
        self.assertEqual(before["a2"], frozenset({"a1"}))
        self.assertEqual(before["b1"], frozenset({"a1"}))
        self.assertEqual(before["a1"], frozenset())


def _three_sleeps() -> RunSpec:
    return RunSpec(
        seed=1,
        target=TOY_TARGET.name,
        environment=Environment(target={}),
        budgets=Budgets(1, 1, 3),
        actors=(Actor(id="a", actions=("a", "b", "c")),),
        actions=tuple(
            Action(id=i, kind="sleep", deadline=DEADLINE_OPERATION, payload={})
            for i in ("a", "b", "c")
        ),
    )


def _scheduler(t: float, status: str = "completed") -> Event:
    return Event(CLEANUP, t, {"scheduler": {"status": status, "reason": None}})


_DAEMON_EXIT = Event(INFO, 3.5, {"reason": DAEMON_EXIT, "returncode": -9})
_RESOLVED = [
    ev(INVOKE, "a", 1.0),
    ev(OK, "a", 2.0),
    ev(INVOKE, "b", 2.0),
    ev(FAIL, "b", 3.0, errno=5),
    _DAEMON_EXIT,
    ev(SKIPPED, "c", 4.0),
]


class HistoryGrammarTest(unittest.TestCase):
    def test_valid_complete_histories_and_prefixes(self) -> None:
        """A durable prefix stays judgeable; a scheduler record resolves every action."""
        spec = _three_sleeps()
        valid = {
            "empty prefix": [],
            "invoked only": [ev(INVOKE, "a", 1.0)],
            "killed mid-run": [
                ev(INVOKE, "a", 1.0),
                ev(OK, "a", 2.0),
                ev(INVOKE, "b", 2.5),
            ],
            "resolved": [*_RESOLVED, _scheduler(5.0)],
            "harness failure": [
                ev(INVOKE, "a", 1.0),
                ev(INFO, "a", 2.0, reason="operation-timeout"),
                ev(SKIPPED, "b", 2.1),
                ev(SKIPPED, "c", 2.1),
                _scheduler(3.0, "harness-failure"),
            ],
            "equal invoke and completion": [
                ev(INVOKE, "a", 1.0),
                ev(OK, "a", 1.0),
                ev(SKIPPED, "b", 2.0),
                ev(SKIPPED, "c", 2.0),
                _scheduler(3.0),
            ],
        }
        for name, events in valid.items():
            with self.subTest(history=name):
                self.assertEqual((), history_problems(spec, events))
        self.assertEqual(
            (),
            history_problems(
                spec, [*_RESOLVED, _scheduler(5.0)], execution_status="completed"
            ),
        )
        self.assertEqual(
            (),
            history_problems(
                spec, _RESOLVED[:2], execution_status="containment-timeout"
            ),
        )

    def test_structural_negatives_are_named(self) -> None:
        spec = _three_sleeps()
        cases: dict[str, tuple[list[Event], str]] = {
            "unknown action": (
                [ev(INVOKE, "zz", 1.0)],
                "event 0: invoke names unknown action 'zz'",
            ),
            "action-less invoke": (
                [Event(INVOKE, 1.0, {})],
                "event 0: invoke names unknown action None",
            ),
            "duplicate invocation": (
                [ev(INVOKE, "a", 1.0), ev(INVOKE, "a", 1.5)],
                "a: duplicate invoke",
            ),
            "duplicate terminal": (
                [ev(INVOKE, "a", 1.0), ev(OK, "a", 2.0), ev(FAIL, "a", 3.0)],
                "a: duplicate terminal ['ok', 'fail']",
            ),
            "last-wins terminal": (
                [ev(INVOKE, "a", 1.0), ev(FAIL, "a", 2.0), ev(OK, "a", 3.0)],
                "a: duplicate terminal ['fail', 'ok']",
            ),
            "terminal without invocation": (
                [ev(OK, "a", 2.0)],
                "a: terminal ok without invocation",
            ),
            "terminal before invocation": (
                [ev(OK, "a", 1.0), ev(INVOKE, "a", 2.0)],
                "a: terminal before invocation ['ok', 'invoke']",
            ),
            "completion timestamp before invoke": (
                [ev(INVOKE, "a", 2.0), ev(OK, "a", 1.0)],
                "a: ok at t=1.0 precedes invoke",
            ),
            "skipped plus terminal": (
                [ev(SKIPPED, "a", 1.0), ev(OK, "a", 2.0)],
                "a: skipped and also ['skipped', 'ok']",
            ),
            "invoked then skipped": (
                [ev(INVOKE, "a", 1.0), ev(SKIPPED, "a", 2.0)],
                "a: skipped and also ['invoke', 'skipped']",
            ),
            "oversized event": (
                [ev(INVOKE, "a", 1.0, blob="x" * MAX_EVENT_BYTES)],
                "event 0: 524340 bytes exceeds 524288",
            ),
            "oversized history": (
                [
                    Event(INFO, 0.0, {"reason": "note", "blob": "x" * 500_000})
                    for _ in range(34)
                ],
                "history: 17001802 bytes exceeds 16777216",
            ),
            "action event after the scheduler record": (
                [*_RESOLVED[:5], _scheduler(4.5), ev(SKIPPED, "c", 5.0)],
                "event 6: skipped after the scheduler record",
            ),
            "two scheduler records": (
                [*_RESOLVED, _scheduler(5.0), _scheduler(6.0)],
                "2 scheduler cleanup records",
            ),
            "malformed scheduler record": (
                [*_RESOLVED, Event(CLEANUP, 5.0, {"scheduler": "done"})],
                "scheduler cleanup record has bad status 'done'",
            ),
        }
        for name, (events, problem) in cases.items():
            with self.subTest(history=name):
                self.assertIn(problem, history_problems(spec, events))

    def test_actor_order_and_requires_gate_invocations(self) -> None:
        """Impossible schedules are invalid; valid prefixes stay valid."""

        def spec(actors: tuple[Actor, ...], condition: str = "finished") -> RunSpec:
            return RunSpec(
                seed=1,
                target=TOY_TARGET.name,
                environment=Environment(target={}),
                budgets=Budgets(1, 1, 3),
                actors=actors,
                actions=(
                    Action(
                        id="w", kind="sleep", deadline=DEADLINE_OPERATION, payload={}
                    ),
                    Action(
                        id="r",
                        kind="sleep",
                        deadline=DEADLINE_OPERATION,
                        payload={},
                        requires=(("w", condition),),
                    ),
                ),
            )

        one_actor = spec((Actor(id="a", actions=("w", "r")),))
        genuine = [
            ev(INVOKE, "w", 1.0),
            ev(OK, "w", 2.0),
            ev(INVOKE, "r", 2.0),
            ev(OK, "r", 3.0),
            _scheduler(4.0),
        ]
        for end in range(len(genuine) + 1):
            with self.subTest(prefix=end):
                self.assertEqual((), history_problems(one_actor, genuine[:end]))
        overlap = [
            ev(INVOKE, "w", 1.0),
            ev(INVOKE, "r", 1.001),
            ev(OK, "w", 1.002),
            ev(OK, "r", 1.003),
            _scheduler(2.0),
        ]
        self.assertEqual(
            (
                "r: actor a invoked it before w completed",
                "r: invoked before w finished",
            ),
            history_problems(one_actor, overlap, execution_status="completed"),
        )
        skipped = [
            ev(SKIPPED, "w", 1.0),
            ev(INVOKE, "r", 1.001),
            ev(OK, "r", 1.002),
            _scheduler(2.0),
        ]
        self.assertEqual(
            (
                "r: actor a invoked it before w completed",
                "r: invoked although its prerequisite w was skipped",
            ),
            history_problems(one_actor, skipped, execution_status="completed"),
        )
        two_actors = spec(
            (Actor(id="a", actions=("w",)), Actor(id="b", actions=("r",))), "started"
        )
        started = [ev(INVOKE, "w", 1.0), ev(INVOKE, "r", 1.0), ev(OK, "r", 1.5)]
        self.assertEqual((), history_problems(two_actors, started))
        early = [ev(INVOKE, "r", 1.0), ev(INVOKE, "w", 1.0)]
        self.assertEqual(
            ("r: invoked before w started",), history_problems(two_actors, early)
        )

    def test_execution_status_must_agree_with_the_scheduler_record(self) -> None:
        """A completed execution needs the cleanup record, and it must say completed."""
        spec = _three_sleeps()
        self.assertEqual(
            ("completed execution has no scheduler cleanup record",),
            history_problems(spec, _RESOLVED[:4], execution_status="completed"),
        )
        self.assertEqual(
            ("completed execution has no scheduler cleanup record",),
            history_problems(spec, _RESOLVED, execution_status="completed"),
        )
        self.assertEqual(
            ("completed execution but scheduler reported 'harness-failure'",),
            history_problems(
                spec,
                [*_RESOLVED, _scheduler(5.0, "harness-failure")],
                execution_status="completed",
            ),
        )

    def test_scheduler_record_requires_every_terminal_whatever_the_status(
        self,
    ) -> None:
        """Prefix leniency ends at the scheduler record, for every execution status."""
        spec = _three_sleeps()
        unresolved = [*_RESOLVED[:4], ev(INVOKE, "c", 4.0), _scheduler(5.0)]
        dropped = [*_RESOLVED[:4], _scheduler(5.0)]
        for status in (None, "completed", "harness-failure", "containment-timeout"):
            for name, events in (("invoked only", unresolved), ("dropped", dropped)):
                with self.subTest(status=status, history=name):
                    self.assertIn(
                        "c: no terminal before the scheduler record",
                        history_problems(spec, events, execution_status=status),
                    )
        for status in (None, "harness-failure", "containment-timeout"):
            with self.subTest(prefix_status=status):
                self.assertEqual(
                    (),
                    history_problems(spec, unresolved[:-1], execution_status=status),
                )

    def test_non_finite_and_unencodable_events_are_invalid_not_crashes(
        self,
    ) -> None:
        spec = _three_sleeps()
        cases = {
            "nan time": (
                Event(INVOKE, math.nan, {"action": "a"}),
                "time nan is not a number within ±2**53 seconds",
            ),
            "infinite time": (
                Event(INVOKE, math.inf, {"action": "a"}),
                "time inf is not a number within ±2**53 seconds",
            ),
            "bool time": (Event(INVOKE, cast(Any, True), {"action": "a"}), "time True"),
            "lone surrogate": (
                Event(INFO, 1.0, {"reason": "x\ud800"}),
                "event 0: not encodable as UTF-8 JSON",
            ),
            "nan fact": (
                Event(INFO, 1.0, {"reason": "x", "value": math.nan}),
                "event 0: not encodable as UTF-8 JSON",
            ),
            "unserializable fact": (
                Event(INFO, 1.0, {"reason": "x", "value": object()}),
                "event 0: not encodable as UTF-8 JSON",
            ),
        }
        for name, (event, problem) in cases.items():
            with self.subTest(event=name):
                problems = history_problems(spec, [event])
                self.assertTrue(any(problem in found for found in problems), problems)

    def test_event_times_at_the_range_boundary_are_valid_and_canonical(
        self,
    ) -> None:
        spec = _three_sleeps()
        bounds = [
            (-MAX_EVENT_TIME, MAX_EVENT_TIME),
            (-float(MAX_EVENT_TIME), float(MAX_EVENT_TIME)),
            (0, 1),
            (-0.5, 0.0),
        ]
        for start, end in bounds:
            with self.subTest(start=start, end=end):
                events = [
                    ev(INVOKE, "a", start),
                    ev(OK, "a", end),
                    ev(SKIPPED, "b", end),
                    ev(SKIPPED, "c", end),
                    _scheduler(end),
                ]
                self.assertEqual((), history_problems(spec, events))
                for event in events:
                    line = event_line(event)
                    decoded = event_from_json(json.loads(line))
                    self.assertEqual(event, decoded)
                    self.assertIs(type(event.time), type(decoded.time))
                    self.assertEqual(line, event_line(decoded))

    def test_event_times_outside_the_range_are_invalid_not_crashes(self) -> None:
        """No time value escapes the grammar, whatever the int-to-text digit limit."""
        spec = _three_sleeps()
        above = math.nextafter(float(MAX_EVENT_TIME), math.inf)
        cases: dict[str, tuple[Any, str]] = {
            "int above": (MAX_EVENT_TIME + 1, "9007199254740993"),
            "int below": (-MAX_EVENT_TIME - 1, "-9007199254740993"),
            "float above": (above, repr(above)),
            "float below": (-above, repr(-above)),
            "huge float": (1e300, "1e+300"),
            "10**1000": (10**1000, "<int of 3322 bits>"),
            "-10**1000": (-(10**1000), "<int of 3322 bits>"),
            "past the digit limit": (10**5000, "<int of 16610 bits>"),
            "text": ("1.0", "<str>"),
            "none": (None, "<NoneType>"),
        }
        for name, (time, label) in cases.items():
            with self.subTest(time=name):
                self.assertFalse(is_event_time(time))
                histories = [
                    [Event(INVOKE, time, {"action": "a"})],
                    [ev(INVOKE, "a", 1.0), Event(OK, time, {"action": "a"})],
                    [Event(INVOKE, time, {"action": "a"}), ev(OK, "a", 1.0)],
                ]
                for events in histories:
                    problems = history_problems(spec, events)
                    expected = f"time {label} is not a number within ±2**53 seconds"
                    self.assertTrue(
                        any(expected in problem for problem in problems), problems
                    )
                    self.assertFalse(any("precedes" in p for p in problems), problems)
                    self.assertLess(max(len(p) for p in problems), 512, problems)

    def test_huge_integer_time_from_a_json_line_is_invalid(self) -> None:
        spec = _three_sleeps()
        for digits in ("1" + "0" * 400, "-1" + "0" * 1000):
            with self.subTest(digits=len(digits)):
                line = '{"type":"invoke","time":' + digits + ',"action":"a"}'
                event = event_from_json(json.loads(line))
                problems = history_problems(spec, [event, ev(OK, "a", 2.0)])
                self.assertIn(
                    f"event 0: time <int of {abs(int(digits)).bit_length()} bits> "
                    "is not a number within ±2**53 seconds",
                    problems,
                )

    def test_history_index_keeps_the_last_completion(self) -> None:
        """The index stays tolerant; only `history_problems` judges structure."""
        spec = _three_sleeps()
        events = [ev(INVOKE, "a", 1.0), ev(FAIL, "a", 2.0), ev(OK, "a", 3.0)]
        history = History(spec, events, TOY_TARGET.actions)
        self.assertEqual(OK, history.outcome("a"))
        self.assertNotEqual((), history_problems(spec, events))


class EventIdentityTest(unittest.TestCase):
    """Data cannot replace an event's own type or time in its canonical line."""

    def test_reserved_data_keys_are_invalid_and_never_encode(self) -> None:
        spec = _three_sleeps()
        for data in (
            {"action": "a", "type": "fail"},
            {"action": "a", "time": 0.5},
            {"action": "a", "type": "fail", "time": 0.5, "errno": 5},
        ):
            with self.subTest(data=sorted(data)):
                event = Event(OK, 2.0, data)
                problems = history_problems(spec, [ev(INVOKE, "a", 1.0), event])
                self.assertEqual(1, len(problems))
                self.assertIn("event 1: data carries reserved keys", problems[0])
                with self.assertRaisesRegex(ValueError, "reserved key"):
                    event_line(event)
                with self.assertRaisesRegex(ValueError, "reserved key"):
                    event_to_json(event)

    def test_a_shadowed_event_is_not_the_other_history(self) -> None:
        """An `ok` whose data says `fail` at t=0.5 passes as neither."""
        spec = _three_sleeps()
        shadow = Event(OK, 2.0, {"action": "a", "type": "fail", "time": 0.5})
        forged = Event(FAIL, 0.5, {"action": "a"})
        self.assertEqual(
            ("event 1: data carries reserved keys ['time', 'type']",),
            history_problems(spec, [ev(INVOKE, "a", 1.0), shadow]),
        )
        self.assertEqual(
            ("a: fail at t=0.5 precedes invoke",),
            history_problems(spec, [ev(INVOKE, "a", 1.0), forged]),
        )

    def test_malformed_records_are_invalid_not_crashes(self) -> None:
        spec = _three_sleeps()
        cases: dict[str, Any] = {
            "not an event": {"type": INVOKE, "time": 1.0, "action": "a"},
            "unhashable type": Event(cast(Any, ["invoke"]), 1.0, {"action": "a"}),
            "list data": Event(INVOKE, 1.0, cast(Any, [("action", "a")])),
        }
        for name, record in cases.items():
            with self.subTest(record=name):
                (problem,) = history_problems(spec, [cast(Any, record)])
                self.assertTrue(problem.startswith("event 0: "), problem)


def _bounds_doc(**changes: Any) -> dict[str, Any]:
    doc = json.loads(
        _minimal_spec(
            {"a": _minimal_action("sleep")},
            [{"id": "actor", "actions": ["a"]}],
            target_name=TOY_TARGET.name,
            environment_target={},
        )
    )
    doc.update(changes)
    return doc


class StrictRunSpecJsonTest(unittest.TestCase):
    """RunSpec JSON is strict: unique keys, finite numbers, integers within ±2**53."""

    _BASE = json.dumps(_bounds_doc(), sort_keys=True)

    def test_strict_documents_are_rejected_before_conversion(self) -> None:
        base = self._BASE
        self.assertIn('"seed": 1,', base)
        self.assertIn('"seconds": 1.0', base)
        cases = {
            "duplicate top-level key": (
                base.replace('"seed": 1,', '"seed": 1, "seed": 2,'),
                "duplicate key 'seed'",
            ),
            "duplicate nested key": (
                base.replace('"seconds": 1.0', '"seconds": 1.0, "seconds": 2.0'),
                "duplicate key 'seconds'",
            ),
            "NaN": (base.replace('"seconds": 1.0', '"seconds": NaN'), "NaN is not"),
            "Infinity": (
                base.replace('"seconds": 1.0', '"seconds": Infinity'),
                "Infinity is not",
            ),
            "-Infinity": (
                base.replace('"seconds": 1.0', '"seconds": -Infinity'),
                "-Infinity is not",
            ),
            "overflowing float": (
                base.replace('"seconds": 1.0', '"seconds": 1e999'),
                "number 1e999 is not finite",
            ),
            "overflowing negative float": (
                base.replace('"seed": 1,', '"seed": 1, "metadata": {"x": -1e999},'),
                "number -1e999 is not finite",
            ),
            "integer one past": (
                base.replace('"seed": 1,', f'"seed": {MAX_INTEGER + 1},'),
                "integer 9007199254740993 is outside ±2**53",
            ),
            "integer one below": (
                base.replace('"seed": 1,', f'"seed": {-MAX_INTEGER - 1},'),
                "integer -9007199254740993 is outside ±2**53",
            ),
            "17-digit integer": (
                base.replace('"seed": 1,', '"seed": 10000000000000000,'),
                "integer of 17 digits is outside ±2**53",
            ),
            "5000-digit seed": (
                base.replace('"seed": 1,', '"seed": 1' + "0" * 5000 + ","),
                "integer of 5001 digits is outside ±2**53",
            ),
            "5000-digit metadata": (
                base.replace(
                    '"seed": 1,', '"seed": 1, "metadata": {"x": 1' + "0" * 5000 + "},"
                ),
                "integer of 5001 digits is outside ±2**53",
            ),
        }
        for name, (text, message) in cases.items():
            with self.subTest(document=name):
                with self.assertRaisesRegex(SpecError, re.escape(message)):
                    from_json(text)

    def test_boundary_numbers_are_accepted_and_canonical(self) -> None:
        metadata = {
            "max": MAX_INTEGER,
            "min": -MAX_INTEGER,
            "largest": 1.7976931348623157e308,
            "tiny": 5e-324,
        }
        doc = _bounds_doc(seed=MAX_INTEGER, metadata=metadata)
        spec = from_json(json.dumps(doc))
        self.assertEqual(MAX_INTEGER, spec.seed)
        self.assertEqual(metadata, dict(spec.metadata))
        text = to_json(spec)
        self.assertNotIn("Infinity", text)
        self.assertEqual(spec, from_json(text))
        self.assertEqual(text, to_json(from_json(text)))

    def test_strict_loads_accepts_standard_json(self) -> None:
        self.assertEqual(
            {"a": [1, -2.5, None, True, "x"]},
            strict_loads('{"a": [1, -2.5, null, true, "x"]}'),
        )

    def test_strictly_malformed_specs_are_refused(self) -> None:
        """Strictly malformed RunSpec documents: none is accepted or escapes."""
        base = json.dumps(
            json.loads(
                _minimal_spec(
                    {"a": {"kind": "sleep", "seconds": 0.0, "deadline": "operation"}},
                    [{"id": "x", "actions": ["a"]}],
                    target_name=TOY_TARGET.name,
                    environment_target={},
                )
            )
        )
        accepted = {
            "ok": base,
            "seed 4000 digits": base.replace('"seed": 1', '"seed": 1' + "0" * 4000),
        }
        rejected = {
            "seed 5000 digits": base.replace('"seed": 1', '"seed": 1' + "0" * 5000),
            "seconds NaN": base.replace('"seconds": 0.0', '"seconds": NaN'),
            "seconds 1e999": base.replace('"seconds": 0.0', '"seconds": 1e999'),
            "dup key": base.replace('"seed": 1', '"seed": 1, "seed": 2'),
            "metadata NaN": base.replace(
                '"seed": 1', '"seed": 1, "metadata": {"x": NaN}'
            ),
            "metadata 5000-digit": base.replace(
                '"seed": 1', '"seed": 1, "metadata": {"x": 1' + "0" * 5000 + "}"
            ),
        }
        self.assertIsInstance(from_json(accepted["ok"]), RunSpec)
        with self.assertRaisesRegex(SpecError, "integer of 4001 digits"):
            from_json(accepted["seed 4000 digits"])
        unknown_budget_field = base.replace(
            '"budgets": {', '"budgets": {"unknown_seconds": 1, '
        )
        with self.assertRaisesRegex(
            SpecError,
            re.escape("budgets has unknown fields: ['unknown_seconds']"),
        ):
            from_json(unknown_budget_field)
        for name, text in rejected.items():
            with self.subTest(document=name):
                with self.assertRaises(SpecError):
                    from_json(text)


class InMemorySpecBoundsTest(unittest.TestCase):
    """`spec_problems` judges in-memory specs; `to_json` refuses what it rejects."""

    def _assert_rejected(self, spec: Any, message: str) -> None:
        problems = spec_problems(spec)
        self.assertTrue(any(message in problem for problem in problems), problems)
        with self.assertRaisesRegex(SpecError, re.escape(problems[0])):
            to_json(spec)

    def test_valid_specs_have_no_problems(self) -> None:
        parsed = from_json(json.dumps(_bounds_doc()))
        self.assertEqual((), spec_problems(parsed))
        self.assertEqual(parsed, from_json(to_json(parsed)))
        spec = _three_sleeps()
        self.assertEqual((), spec_problems(spec))
        listed = replace(
            spec,
            actors=cast(Any, list(spec.actors)),
            actions=cast(Any, list(spec.actions)),
        )
        self.assertEqual((), spec_problems(listed))

    def test_out_of_bounds_values_are_problems_not_crashes(self) -> None:
        spec = _three_sleeps()
        action = spec.actions[0]
        cyclic: dict[str, Any] = {}
        cyclic["self"] = cyclic
        deep: Any = 0
        for _ in range(MAX_JSON_DEPTH + 1):
            deep = [deep]
        cases: dict[str, tuple[Any, str]] = {
            "huge seed": (replace(spec, seed=10**5000), "seed is an integer outside"),
            "seed past": (replace(spec, seed=MAX_INTEGER + 1), "seed is an integer"),
            "boolean seed": (replace(spec, seed=True), "seed must be an integer"),
            "text target": (replace(spec, target=cast(Any, 7)), "target must be"),
            "huge budget": (
                replace(
                    spec,
                    budgets=replace(spec.budgets, runner_deadline_seconds=10**5000),
                ),
                "budgets.runner_deadline_seconds must be finite",
            ),
            "nan budget": (
                replace(
                    spec,
                    budgets=replace(spec.budgets, operation_deadline_seconds=math.nan),
                ),
                "budgets.operation_deadline_seconds must be finite",
            ),
            "budget order": (
                replace(
                    spec,
                    budgets=replace(
                        spec.budgets,
                        operation_deadline_seconds=120,
                        cleanup_deadline_seconds=30,
                        runner_deadline_seconds=140,
                    ),
                ),
                "operation budget must fit within runner minus cleanup",
            ),
            "unreserved cleanup": (
                replace(
                    spec,
                    budgets=replace(
                        spec.budgets,
                        cleanup_deadline_seconds=30,
                        runner_deadline_seconds=30,
                    ),
                ),
                "cleanup budget must be less than runner budget",
            ),
            "duplicate action id": (
                replace(spec, actions=(action, action)),
                f"actions[1].id {action.id!r} is a duplicate",
            ),
            "bad deadline": (
                replace(spec, actions=(replace(action, deadline="forever"),)),
                "actions[0].deadline must be one of",
            ),
            "huge payload int": (
                replace(spec, actions=(replace(action, payload={"n": -(10**5000)}),)),
                "actions[0].payload.n is an integer outside ±2**53",
            ),
            "nan payload": (
                replace(spec, actions=(replace(action, payload={"x": [math.inf]}),)),
                "actions[0].payload.x[0] is not a finite number",
            ),
            "object payload": (
                replace(spec, actions=(replace(action, payload={"x": object()}),)),
                "is a object, not a JSON value",
            ),
            "non-text key": (
                replace(spec, metadata=cast(Any, {1: "a"})),
                "has a int key",
            ),
            "cyclic metadata": (replace(spec, metadata=cyclic), "nests deeper"),
            "deep metadata": (replace(spec, metadata={"d": deep}), "nests deeper"),
            "huge id": (
                replace(spec, actions=(replace(action, id="x" * 129),)),
                "actions[0].id exceeds 128 UTF-8 bytes",
            ),
            "not a spec": (object(), "RunSpec must be a RunSpec"),
        }
        for name, (candidate, message) in cases.items():
            with self.subTest(spec=name):
                self._assert_rejected(candidate, message)


class RunSpecBoundsTest(unittest.TestCase):
    def _raw(self, count: int = 1) -> dict[str, Any]:
        ids = [f"a{index}" for index in range(count)]
        return json.loads(
            _minimal_spec(
                {action_id: _minimal_action("sleep") for action_id in ids},
                [{"id": "actor", "actions": ids}],
                target_name=TOY_TARGET.name,
                environment_target={},
            )
        )

    def test_limits_accept_at_the_boundary(self) -> None:
        raw = self._raw(MAX_ACTIONS)
        raw["actions"]["é" * 64] = raw["actions"].pop("a0")
        raw["actors"][0]["actions"][0] = "é" * 64
        raw["metadata"] = {"note": "é" * 2048, "k" * 4096: 1}
        self.assertEqual(MAX_ACTIONS, len(from_json(json.dumps(raw)).actions))
        many_actors = self._raw(MAX_ACTORS)
        many_actors["actors"] = [
            {"id": f"p{index}", "actions": [f"a{index}"]} for index in range(MAX_ACTORS)
        ]
        self.assertEqual(MAX_ACTORS, len(from_json(json.dumps(many_actors)).actors))

    def test_limits_reject_past_the_boundary(self) -> None:
        def mutated(change: Any, count: int = 1) -> str:
            raw = self._raw(count)
            change(raw)
            return json.dumps(raw)

        def rename_action(new: str) -> Any:
            def change(raw: dict[str, Any]) -> None:
                raw["actions"][new] = raw["actions"].pop("a0")
                raw["actors"][0]["actions"] = [new]

            return change

        def extra_actors(raw: dict[str, Any]) -> None:
            raw["actors"].extend(
                {"id": f"p{index}", "actions": []} for index in range(MAX_ACTORS)
            )

        cases = {
            "actions": (mutated(lambda raw: None, MAX_ACTIONS + 1), "action limit"),
            "actors": (mutated(extra_actors), "actor limit"),
            "action id": (mutated(rename_action("é" * 65)), "exceeds 128 UTF-8 bytes"),
            "actor id": (
                mutated(lambda raw: raw["actors"][0].update(id="x" * 129)),
                "actor id",
            ),
            "metadata string": (
                mutated(lambda raw: raw.update(metadata={"note": "é" * 2049})),
                "RunSpec.metadata.note exceeds 4096 UTF-8 bytes",
            ),
            "metadata key": (
                mutated(lambda raw: raw.update(metadata={"k" * 4097: 1})),
                "RunSpec.metadata key exceeds 4096 UTF-8 bytes",
            ),
            "nested payload string": (
                mutated(lambda raw: raw.update(metadata={"a": [{"b": "x" * 4097}]})),
                "RunSpec.metadata.a[0].b exceeds",
            ),
            "document": (
                mutated(
                    lambda raw: raw.update(
                        metadata={f"k{i}": "x" * 4000 for i in range(300)}
                    )
                ),
                f"RunSpec exceeds {MAX_RUNSPEC_BYTES} UTF-8 bytes",
            ),
            "non-object document": (
                "[" + ",".join(["0"] * (MAX_RUNSPEC_BYTES // 2 + 1)) + "]",
                f"RunSpec exceeds {MAX_RUNSPEC_BYTES} UTF-8 bytes",
            ),
            "lone surrogate": (
                mutated(lambda raw: raw.update(metadata={"note": "\ud800"})),
                "is not valid UTF-8 text",
            ),
        }
        for name, (text, message) in cases.items():
            with self.subTest(limit=name):
                with self.assertRaisesRegex(SpecError, re.escape(message)):
                    from_json(text)
