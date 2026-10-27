# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import unittest
from collections.abc import Mapping
from typing import Any

from eden.integration.correctness.exploratory.core.differential import (
    read_side,
    REFERENCE_RESULT,
    SideResult,
)
from eden.integration.correctness.exploratory.core.differential_schedule import (
    Applicability,
    canonical_value,
    MalformedObservation,
    not_applicable,
    ObservationDef,
    observe,
    Ordering,
    reference_facts,
    schedule_problems,
    ScheduleProblem,
    SHARED,
    SideObservation,
    UNORDERED_CONFLICT,
    validate_differential,
)
from eden.integration.correctness.exploratory.core.effects import (
    Footprint,
    footprints_interact,
    ROOT,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    Event,
    event_line,
    history_problems,
    MAX_EVENT_BYTES,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    from_json,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.core.test.testing import (
    ev,
    minimal_spec,
    TOY_TARGET,
)

_REFERENCE = SideResult("ok", {"entries": ["b", "a"]}, 1.2, 1.8)
_WHOLE = Footprint(subtrees_written=frozenset({ROOT}))


def _sleep(**extra: Any) -> dict[str, Any]:
    return {"kind": "sleep", "deadline": "operation", "seconds": 0.0, **extra}


def _spec(*action_ids: str) -> RunSpec:
    return from_json(
        minimal_spec(
            {action: _sleep() for action in action_ids},
            [{"id": "a", "actions": list(action_ids)}],
            target_name="toy",
            environment_target={},
        ),
        TOY_TARGET,
    )


def _schedule(
    actors: Mapping[str, list[str]], requires: Mapping[str, list[list[str]]]
) -> RunSpec:
    """Return a spec of sleeps on the given actors with the given requires edges."""
    action_ids = [action for chain in actors.values() for action in chain]
    actions = {
        action: _sleep(**({"requires": requires[action]} if action in requires else {}))
        for action in action_ids
    }
    return from_json(
        minimal_spec(
            actions,
            [{"id": actor, "actions": chain} for actor, chain in actors.items()],
            target_name="toy",
            environment_target={},
        ),
        TOY_TARGET,
    )


def _prints(table: Mapping[str, Footprint]) -> Any:
    def footprint(_spec: RunSpec, action: Action) -> Footprint:
        return table.get(action.id, Footprint())

    return footprint


def _paired(
    action: str = "s", reference: SideResult = _REFERENCE, **primary: object
) -> list[Event]:
    return [
        ev("invoke", action, 1.0),
        ev("ok", action, 2.0, **primary, **reference_facts(reference)),
    ]


def _scheduler(t: float) -> Event:
    return Event(type=CLEANUP, time=t, data={"scheduler": {"status": "completed"}})


class ApplicabilityTest(unittest.TestCase):
    def test_shared_and_not_applicable_reasons(self) -> None:
        self.assertTrue(SHARED.shared)
        self.assertEqual("primary-only", not_applicable("primary-only").reason)
        for bad in ("", "Primary", "primary only", "1st"):
            with self.subTest(reason=bad), self.assertRaises(ValueError):
                not_applicable(bad)
        with self.assertRaises(ValueError):
            Applicability(True, "primary-only")


class ReferenceSideTest(unittest.TestCase):
    def test_pairing_keeps_the_history_grammar(self) -> None:
        events = [*_paired(entries=[]), _scheduler(5.0)]
        self.assertEqual(
            (), history_problems(_spec("s"), events, execution_status="completed")
        )

    def test_a_result_never_spans_two_terminal_events(self) -> None:
        split = [
            ev("invoke", "s", 1.0),
            ev("ok", "s", 2.0),
            ev("info", "s", 3.0, **reference_facts(_REFERENCE)),
        ]
        self.assertTrue(history_problems(_spec("s"), split))
        orphan = [ev("ok", "s", 2.0, **reference_facts(_REFERENCE))]
        self.assertTrue(history_problems(_spec("s"), orphan))

    def test_malformed_reference_sides_raise(self) -> None:
        good = reference_facts(_REFERENCE)[REFERENCE_RESULT]
        self.assertEqual(_REFERENCE, read_side(good))
        cases = {
            "missing key": {k: v for k, v in good.items() if k != "started"},
            "extra key": {**good, "time": 1.0},
            "schema": {**good, "schema_version": 2},
            "outcome": {**good, "outcome": "skipped"},
            "backwards": {**good, "completed": 1.1},
            "nonfinite": {**good, "started": float("nan")},
            "huge start": {**good, "started": 10**1000},
            "huge negative start": {**good, "started": -(10**1000)},
            "completion past the digit limit": {**good, "completed": 10**5000},
            "one past the range": {**good, "completed": 2**53 + 1},
            "boolean": {**good, "started": True},
            "facts": {**good, "facts": ["entries"]},
            "nested": {**good, "facts": {REFERENCE_RESULT: {}}},
        }
        for name, raw in cases.items():
            with self.subTest(case=name), self.assertRaises(ValueError):
                read_side(raw)
        with self.assertRaises(ValueError):
            reference_facts(SideResult("skipped", {}, 1.0, 2.0))
        for started, completed in ((10**1000, 10**1000), (0.0, -(10**5000))):
            with self.assertRaisesRegex(ValueError, "within ±2\\*\\*53 seconds"):
                reference_facts(SideResult("ok", {}, started, completed))
        bound = reference_facts(SideResult("ok", {}, -(2**53), 2**53))
        self.assertEqual(2**53, bound[REFERENCE_RESULT]["completed"])

    def test_reference_side_counts_against_the_event_bound(self) -> None:
        names = [f"{index:06d}-{'n' * 70}.dat" for index in range(4000)]
        half = SideResult("ok", {"entries": names[:2000]}, 1.0, 2.0)
        fits = _paired(entries=names[:2000], reference=half)
        self.assertLess(len(event_line(fits[1])), MAX_EVENT_BYTES)
        self.assertEqual((), history_problems(_spec("s"), fits))
        whole = SideResult("ok", {"entries": names}, 1.0, 2.0)
        oversized = _paired(entries=names, reference=whole)
        self.assertGreater(len(event_line(oversized[1])), MAX_EVENT_BYTES)
        (problem,) = history_problems(_spec("s"), oversized)
        self.assertIn(f"exceeds {MAX_EVENT_BYTES}", problem)


class ObservationTest(unittest.TestCase):
    LISTING = ObservationDef(("entries",), frozenset({"entries"}))

    def test_canonical_observations(self) -> None:
        self.assertEqual(
            observe(SideResult("ok", {"entries": ["b", "a"]}, 0, 1), self.LISTING),
            observe(
                SideResult("ok", {"entries": ["a", "b"], "x": 1}, 0, 1), self.LISTING
            ),
        )
        status = ObservationDef(("status",))
        self.assertEqual(
            observe(SideResult("ok", {"status": {"a": "M", "b": "?"}}, 0, 1), status),
            observe(SideResult("ok", {"status": {"b": "?", "a": "M"}}, 0, 1), status),
        )
        ordered = ObservationDef(("entries",))
        self.assertNotEqual(
            observe(SideResult("ok", {"entries": ["b", "a"]}, 0, 1), ordered),
            observe(SideResult("ok", {"entries": ["a", "b"]}, 0, 1), ordered),
        )
        self.assertEqual(
            SideObservation("fail", "2"),
            observe(
                SideResult("fail", {"errno": 2, "reason": "x"}, 0, 1), self.LISTING
            ),
        )

    def test_numbers_are_normalized_and_non_finite_values_are_malformed(self) -> None:
        self.assertEqual(canonical_value({"a": 1}), canonical_value({"a": 1.0}))
        self.assertNotEqual(canonical_value(1), canonical_value(1.5))
        self.assertNotEqual(canonical_value(True), canonical_value(1))
        self.assertEqual(canonical_value([1, 2]), canonical_value((1, 2)))
        for bad in (float("nan"), float("inf"), {"x": [float("-inf")]}, {1: "a"}, b"x"):
            with self.subTest(value=bad), self.assertRaises(MalformedObservation):
                canonical_value(bad)

    def test_conditional_keys_follow_their_guard(self) -> None:
        stat = ObservationDef(
            ("size", "type"), conditional={"size": ("type", frozenset({"file"}))}
        )
        directory = [
            observe(SideResult("ok", {"type": "directory", "size": size}, 0, 1), stat)
            for size in (4096, 80)
        ]
        self.assertEqual(directory[0], directory[1])
        self.assertEqual(
            observe(SideResult("ok", {"type": "directory"}, 0, 1), stat), directory[0]
        )
        files = [
            observe(SideResult("ok", {"type": "file", "size": size}, 0, 1), stat)
            for size in (64, 0)
        ]
        self.assertNotEqual(files[0], files[1])
        with self.assertRaises(MalformedObservation):
            observe(SideResult("ok", {"type": "file"}, 0, 1), stat)
        for bad in (
            {"size": ("type", frozenset({"file"}))},
            {"type": ("type", frozenset())},
        ):
            with self.subTest(conditional=bad), self.assertRaises(ValueError):
                ObservationDef(("size",), conditional=bad)

    def test_only_unresolved_sides_observe_nothing(self) -> None:
        self.assertIsNone(
            observe(SideResult("info", {"entries": []}, 0, 1), self.LISTING)
        )
        malformed = (
            SideResult("fail", {"reason": "rpc"}, 0, 1),
            SideResult("fail", {"errno": "ENOENT"}, 0, 1),
            SideResult("fail", {"errno": True}, 0, 1),
            SideResult("fail", {"returncode": "255"}, 0, 1),
            SideResult("fail", {"returncode": True}, 0, 1),
            SideResult("ok", {}, 0, 1),
            SideResult("ok", {"entries": "ab"}, 0, 1),
            SideResult("ok", {"entries": [float("nan")]}, 0, 1),
            SideResult("skipped", {}, 0, 1),
        )
        for side in malformed:
            with self.subTest(side=side), self.assertRaises(MalformedObservation):
                observe(side, self.LISTING)
        with self.assertRaises(ValueError):
            ObservationDef(("status",), frozenset({"entries"}))

    def test_failed_processes_are_observed_by_return_code(self) -> None:
        """A failure without an errno is observed by its process return code."""
        checkout = ObservationDef()

        def failed(**facts: object) -> SideObservation | None:
            return observe(SideResult("fail", dict(facts), 0, 1), checkout)

        self.assertEqual(SideObservation("fail", "5"), failed(errno=5, returncode=1))
        self.assertEqual(
            SideObservation("fail", '{"returncode":255}'), failed(returncode=255)
        )
        self.assertEqual(failed(returncode=255), failed(returncode=255, reason="x"))
        self.assertNotEqual(failed(returncode=255), failed(returncode=1))
        self.assertNotEqual(failed(errno=1), failed(returncode=1))
        self.assertEqual(
            SideObservation("ok", "{}"),
            observe(SideResult("ok", {}, 0, 1), checkout),
        )


class FootprintConflictTest(unittest.TestCase):
    def test_conflicts_follow_writes_exact_paths_and_subtrees(self) -> None:
        read = Footprint(reads=frozenset({"d/f"}))
        cases = (
            (_WHOLE, read, True),
            (read, read, False),
            (Footprint(writes=frozenset({"d/f"})), read, True),
            (Footprint(writes=frozenset({"d"})), read, False),
            (Footprint(subtrees_written=frozenset({"d"})), read, True),
            (Footprint(writes=frozenset({"d/f/g"})), read, False),
            (
                Footprint(writes=frozenset({"d/f/g"})),
                Footprint(subtrees_read=frozenset({"d/f"})),
                True,
            ),
            (Footprint(writes=frozenset({"d/g"})), read, False),
            (
                Footprint(subtrees_written=frozenset({"d/ff"})),
                Footprint(subtrees_read=frozenset({"d/f"})),
                False,
            ),
            (
                Footprint(subtrees_read=frozenset({ROOT})),
                Footprint(writes=frozenset({"x"})),
                True,
            ),
            (
                Footprint(writes=frozenset({"x"})),
                Footprint(writes=frozenset({"x"})),
                True,
            ),
            (
                Footprint(reads=frozenset({"x"}), channels=frozenset({"fuse"})),
                Footprint(reads=frozenset({"x"}), channels=frozenset({"thrift"})),
                False,
            ),
        )
        for left, right, expected in cases:
            with self.subTest(left=left, right=right):
                self.assertEqual(expected, footprints_interact(left, right))
                self.assertEqual(expected, footprints_interact(right, left))


class ScheduleTest(unittest.TestCase):
    def test_ordering_follows_actor_order_and_finished_requires(self) -> None:
        spec = _schedule(
            {"a": ["w", "r"], "b": ["x"], "c": ["y"]},
            {"x": [["w", "finished"]], "y": [["w", "started"]]},
        )
        ordering = Ordering(spec)
        self.assertTrue(ordering.before("w", "r"))
        self.assertTrue(ordering.before("w", "x"))
        self.assertFalse(ordering.before("r", "w"))
        self.assertTrue(ordering.ordered("r", "w"))
        self.assertFalse(ordering.ordered("w", "y"))
        self.assertFalse(ordering.ordered("r", "x"))

    def test_ordered_conflicts_are_admitted(self) -> None:
        spec = _schedule({"a": ["w", "r"], "b": ["s"]}, {"s": [["r", "finished"]]})
        table = {
            "w": Footprint(writes=frozenset({"f"})),
            "r": Footprint(reads=frozenset({"f"})),
            "s": Footprint(subtrees_read=frozenset({ROOT})),
        }
        self.assertEqual((), schedule_problems(spec, _prints(table)))
        validate_differential(spec, lambda _spec, _action: SHARED, _prints(table))

    def test_concurrent_conflicts_are_rejected(self) -> None:
        spec = _schedule({"a": ["w"], "b": ["r"], "c": ["o"]}, {})
        table = {
            "w": Footprint(writes=frozenset({"f"})),
            "r": Footprint(reads=frozenset({"f"})),
            "o": Footprint(reads=frozenset({"g"})),
        }
        self.assertEqual(
            (ScheduleProblem("r", "w"),), schedule_problems(spec, _prints(table))
        )
        self.assertEqual(UNORDERED_CONFLICT, ScheduleProblem("r", "w").reason)
        with self.assertRaisesRegex(SpecError, "unordered-conflict: 'r' and 'w'"):
            validate_differential(spec, lambda _spec, _action: SHARED, _prints(table))

    def test_a_started_edge_does_not_order(self) -> None:
        spec = _schedule({"a": ["w"], "b": ["r"]}, {"r": [["w", "started"]]})
        table = {"w": _WHOLE, "r": Footprint(reads=frozenset({"f"}))}
        self.assertEqual(1, len(schedule_problems(spec, _prints(table))))

    def test_unshared_actions_are_rejected_with_their_reason(self) -> None:
        spec = _schedule({"a": ["w", "r"]}, {})

        def applicability(_spec: RunSpec, action: Action) -> Applicability:
            return not_applicable("primary-only") if action.id == "w" else SHARED

        with self.assertRaisesRegex(
            SpecError, "'w' \\(sleep\\) is not applicable: primary-only"
        ):
            validate_differential(spec, applicability, _prints({}))
