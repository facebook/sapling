# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Durable history events and sinks for the exploratory harness.

The runner writes these events as JSONL; the supervisor appends them verbatim to
history.jsonl. Every event carries a monotonic timestamp. invoke publishes
``started``; ok/fail/info publishes ``finished`` for the scheduler.
``started`` records scheduler admission: the action's backend call is about
to begin. It is not evidence that the backend engaged the daemon or that a
process-kill target was running.
"""

from __future__ import annotations

import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from typing import Any, Protocol

from .spec_types import Action, FINISHED, RunSpec, STARTED
from .target import ActionDef


INVOKE = "invoke"
OK = "ok"
FAIL = "fail"
INFO = "info"
SKIPPED = "skipped"
CLEANUP = "cleanup"
CLEANUP_REMOVED = "cleanup-removed"
DAEMON_EXIT = "daemon-exit"

COMPLETIONS = frozenset({OK, FAIL, INFO})
TERMINALS = COMPLETIONS | {SKIPPED}
ACTION_EVENTS = TERMINALS | {INVOKE}

SCHEDULER_COMPLETED = "completed"
SCHEDULER_STATUSES = frozenset({SCHEDULER_COMPLETED, "harness-failure"})

MAX_EVENT_BYTES = 512 * 1024
MAX_HISTORY_BYTES = 16 * 1024 * 1024
MAX_EVENT_TIME = 2**53

_NEMESIS = "nemesis"
_FAULT_INSTALL = "fault_install"
_FAULT_REMOVE = "fault_remove"


@dataclass(frozen=True)
class Event:
    """One history record with a monotonic timestamp and string type."""

    type: str
    time: float
    data: dict[str, Any] = field(default_factory=dict, compare=True)


@dataclass
class _ActionRecord:
    invocation: Event | None = None
    completion: Event | None = None
    outcome: str | None = None
    facts: dict[str, Any] = field(default_factory=dict)


class History:
    """Index action events and derive their temporal relationships."""

    def __init__(
        self,
        spec: RunSpec,
        events: Sequence[Event],
        action_defs: Mapping[str, ActionDef],
    ) -> None:
        self._spec = spec
        self._action_defs = action_defs
        self._events = tuple(events)
        self._records = {action.id: _ActionRecord() for action in spec.actions}
        for event in events:
            action_id = event.data.get("action")
            if action_id not in self._records:
                continue
            record = self._records[action_id]
            if event.type == INVOKE:
                record.invocation = event
            elif event.type in COMPLETIONS:
                record.completion = event
                record.outcome = event.type
                record.facts = {
                    key: value
                    for key, value in event.data.items()
                    if key not in ("action", "actor")
                }

    @property
    def events(self) -> tuple[Event, ...]:
        """Return every event in recorded order."""
        return self._events

    def invoke(self, action_id: str) -> Event | None:
        """Return the invocation event for an action, if one was recorded."""
        return self._records[action_id].invocation

    def completion(self, action_id: str) -> Event | None:
        """Return the completion event for an action, if one was recorded."""
        return self._records[action_id].completion

    def outcome(self, action_id: str) -> str | None:
        """Return the completion outcome for an action, if one was recorded."""
        return self._records[action_id].outcome

    def facts(self, action_id: str) -> dict[str, Any]:
        """Return the completion facts for an action."""
        return dict(self._records[action_id].facts)

    def fault_installs(self) -> list[Action]:
        """Return the nemesis fault installs whose completion was ``ok``."""
        return [
            action
            for action in self._spec.actions
            if action.kind == _FAULT_INSTALL
            and self._is_nemesis(action)
            and self.outcome(action.id) == OK
        ]

    def interval(self, action_id: str) -> tuple[float, float] | None:
        """Return the closed invocation-to-completion interval for an action."""
        record = self._records[action_id]
        if record.invocation is None or record.completion is None:
            return None
        return (record.invocation.time, record.completion.time)

    def completed_before(self, first_id: str, second_id: str) -> bool:
        """Return whether first completed strictly before second was invoked."""
        first = self._records[first_id]
        second = self._records[second_id]
        return (
            first.completion is not None
            and second.invocation is not None
            and first.completion.time < second.invocation.time
        )

    def overlaps(self, first_id: str, second_id: str) -> bool:
        """Return whether two complete action intervals overlap."""
        if self.interval(first_id) is None or self.interval(second_id) is None:
            return False
        return not self.completed_before(
            first_id, second_id
        ) and not self.completed_before(second_id, first_id)

    def nemesis_windows(self) -> list[tuple[float, float]]:
        """Return kill, process-lease, and installed-fault lifetime windows.

        A process kill or a spawned process opens a window at its invoke that
        closes at its ok/fail completion, or never.
        """
        windows: list[tuple[float, float]] = []
        for action in self._spec.actions:
            definition = self._action_defs[action.kind]
            if not self._is_nemesis(action) or not (
                definition.kills_process or definition.spawns_process
            ):
                continue
            invocation = self.invoke(action.id)
            completion = self.completion(action.id)
            if invocation is not None:
                end = (
                    completion.time
                    if completion is not None and completion.type != INFO
                    else float("inf")
                )
                windows.append((invocation.time, end))
        windows.extend(window for _install, window in self.fault_windows())
        return sorted(windows)

    def fault_windows(self) -> list[tuple[Action, tuple[float, float]]]:
        """Return each fault install that completed ``ok`` with its lifetime window.

        An install that completed ``fail`` or ``info`` opens no window. A
        window opens at the install's invoke (the earliest moment the fault
        can be armed) and closes at the first later completion of its
        fault_remove, or never.
        """
        windows: list[tuple[Action, tuple[float, float]]] = []
        for install in self._spec.actions:
            if not self._is_nemesis(install) or install.kind != _FAULT_INSTALL:
                continue
            completion = self.completion(install.id)
            invoked = self.invoke(install.id)
            if completion is None or completion.type != OK or invoked is None:
                continue
            start = invoked.time
            end = self._fault_window_end(install, start)
            windows.append((install, (start, end)))
        return windows

    def _is_nemesis(self, action: Action) -> bool:
        return self._action_defs[action.kind].axis == _NEMESIS

    def _fault_window_end(self, install: Action, start: float) -> float:
        """Return the earliest completed event that closes an installed fault."""
        candidates: list[float] = []
        fault_id = install.payload.get("fault_id")
        for action in self._spec.actions:
            if not self._is_nemesis(action):
                continue
            closes_fault = (
                action.kind == _FAULT_REMOVE
                and action.payload.get("fault_id") == fault_id
            )
            completion = self.completion(action.id)
            if closes_fault and completion is not None and completion.time > start:
                candidates.append(completion.time)
        return min(candidates, default=float("inf"))


class HistorySink(Protocol):
    """Append-only receiver for history events."""

    def emit(self, event: Event) -> None:
        """Append one event; must not raise on well-formed input."""
        ...  # pragma: no cover


class InMemorySink:
    """Test sink that retains every emitted event in order."""

    def __init__(self) -> None:
        self.events: list[Event] = []

    def emit(self, event: Event) -> None:
        self.events.append(event)

    def of_type(self, type: str) -> list[Event]:
        """Return emitted events of one type, in emission order."""
        return [event for event in self.events if event.type == type]


def _action_sequences(
    spec: RunSpec, events: Sequence[Event], problems: list[str]
) -> dict[str, list[Event]]:
    """Group action events by action ID, reporting events that name no known action.

    An ``info`` event without an ``action`` key is a run-level report, such
    as a daemon exit, not an action event.
    """
    known = {action.id for action in spec.actions}
    sequences: dict[str, list[Event]] = {action_id: [] for action_id in known}
    for index, event in enumerate(events):
        if event.type not in ACTION_EVENTS:
            continue
        if event.type == INFO and "action" not in event.data:
            continue
        action_id = event.data.get("action")
        if type(action_id) is not str or action_id not in known:
            problems.append(
                f"event {index}: {event.type} names unknown action {action_id!r}"
            )
            continue
        sequences[action_id].append(event)
    return sequences


def is_event_time(value: Any) -> bool:
    """Return whether a value is a supported event time, without raising for any input.

    A supported time is an ``int`` or ``float`` (not ``bool``) in
    ``[-MAX_EVENT_TIME, MAX_EVENT_TIME]`` seconds. Every value in that range
    is finite and compares and round-trips exactly as an IEEE 754 double, so
    times mean the same on every platform and JSON decoder.
    """
    return type(value) in (int, float) and -MAX_EVENT_TIME <= value <= MAX_EVENT_TIME


def time_label(value: Any) -> str:
    """Return a bounded label for a time value; an int wider than 64 bits is named by its size.

    Never raises: ``repr`` of an enormous int can exceed the interpreter's
    int-to-text digit limit, and its text would be unbounded.
    """
    if type(value) is int and value.bit_length() > 64:
        return f"<int of {value.bit_length()} bits>"
    if type(value) in (bool, int, float):
        return repr(value)
    return f"<{type(value).__name__}>"


RESERVED_DATA_KEYS = frozenset({"type", "time"})


def _shape_problems(events: Sequence[Event]) -> list[str]:
    """Report records that are not events with a string type and a dict of data.

    Data may not carry ``type`` or ``time``: those keys are the event's own
    fields in its canonical line, so data holding them would forge another
    event's identity.
    """
    problems = []
    for index, event in enumerate(events):
        if type(event) is not Event:
            problems.append(f"event {index}: not an Event")
        elif type(event.type) is not str or type(event.data) is not dict:
            problems.append(f"event {index}: type must be text and data a dict")
        elif not RESERVED_DATA_KEYS.isdisjoint(event.data):
            reserved = sorted(RESERVED_DATA_KEYS.intersection(event.data))
            problems.append(f"event {index}: data carries reserved keys {reserved}")
    return problems


def _size_problems(events: Sequence[Event]) -> list[str]:
    """Report events without a supported time, unencodable, or beyond the size bounds."""
    problems: list[str] = []
    total = 0
    for index, event in enumerate(events):
        if not is_event_time(event.time):
            problems.append(
                f"event {index}: time {time_label(event.time)} is not a number "
                "within ±2**53 seconds"
            )
        try:
            size = len(event_line(event))
        except (TypeError, ValueError) as error:
            problems.append(f"event {index}: not encodable as UTF-8 JSON: {error}")
            continue
        total += size
        if size > MAX_EVENT_BYTES:
            problems.append(f"event {index}: {size} bytes exceeds {MAX_EVENT_BYTES}")
    if total > MAX_HISTORY_BYTES:
        problems.append(f"history: {total} bytes exceeds {MAX_HISTORY_BYTES}")
    return problems


def _sequence_problems(action_id: str, sequence: Sequence[Event]) -> list[str]:
    """Return how one action's events break invoke-then-one-terminal order."""
    types = [event.type for event in sequence]
    if types in ([], [SKIPPED], [INVOKE]):
        return []
    if len(types) == 2 and types[0] == INVOKE and types[1] in COMPLETIONS:
        invoked, completed = sequence[0].time, sequence[1].time
        if is_event_time(invoked) and is_event_time(completed) and completed < invoked:
            return [f"{action_id}: {types[1]} at t={completed!r} precedes invoke"]
        return []
    problems = []
    if types.count(INVOKE) > 1:
        problems.append(f"{action_id}: duplicate invoke")
    terminals = [kind for kind in types if kind in TERMINALS]
    if len(terminals) > 1:
        problems.append(f"{action_id}: duplicate terminal {terminals}")
    if SKIPPED in types and len(types) > 1:
        problems.append(f"{action_id}: skipped and also {types}")
    elif (
        terminals
        and INVOKE in types
        and types.index(terminals[0]) < types.index(INVOKE)
    ):
        problems.append(f"{action_id}: terminal before invocation {types}")
    elif terminals and INVOKE not in types:
        problems.append(f"{action_id}: terminal {terminals[0]} without invocation")
    return problems or [f"{action_id}: illegal event order {types}"]


def _scheduler_problems(
    spec: RunSpec,
    events: Sequence[Event],
    sequences: Mapping[str, Sequence[Event]],
    execution_status: str | None,
) -> list[str]:
    """Check the scheduler's cleanup record against the action events and execution.

    The record is the ``cleanup`` event carrying ``scheduler``; at most one
    exists, no action event follows it, and every action has a terminal
    before it. A completed execution requires the record, and the record
    must agree that it completed.
    """
    records = [
        (index, event)
        for index, event in enumerate(events)
        if event.type == CLEANUP and "scheduler" in event.data
    ]
    problems: list[str] = []
    if len(records) > 1:
        problems.append(f"{len(records)} scheduler cleanup records")
    if not records:
        if execution_status == SCHEDULER_COMPLETED:
            problems.append("completed execution has no scheduler cleanup record")
        return problems
    index, record = records[0]
    scheduler = record.data["scheduler"]
    status = scheduler.get("status") if type(scheduler) is dict else None
    if type(status) is not str or status not in SCHEDULER_STATUSES:
        problems.append(f"scheduler cleanup record has bad status {scheduler!r}")
    if execution_status == SCHEDULER_COMPLETED and status != SCHEDULER_COMPLETED:
        problems.append(f"completed execution but scheduler reported {status!r}")
    for action in spec.actions:
        sequence = sequences.get(action.id, ())
        if not sequence or sequence[-1].type not in TERMINALS:
            problems.append(f"{action.id}: no terminal before the scheduler record")
    problems.extend(
        f"event {later}: {event.type} after the scheduler record"
        for later, event in enumerate(events)
        if later > index and event.type in ACTION_EVENTS and "action" in event.data
    )
    return problems


def _indices(
    events: Sequence[Event],
) -> tuple[dict[str, int], dict[str, int], set[str]]:
    """Return each action's first invoke and completion index, and the skipped actions."""
    invoked: dict[str, int] = {}
    completed: dict[str, int] = {}
    skipped: set[str] = set()
    for index, event in enumerate(events):
        action_id = event.data.get("action")
        if type(action_id) is not str:
            continue
        if event.type == INVOKE:
            invoked.setdefault(action_id, index)
        elif event.type == SKIPPED:
            skipped.add(action_id)
        elif event.type in COMPLETIONS:
            completed.setdefault(action_id, index)
    return invoked, completed, skipped


def _gate(
    condition: str,
    dependency: str,
    invoked: Mapping[str, int],
    completed: Mapping[str, int],
) -> int | None:
    if condition == STARTED:
        return invoked.get(dependency)
    return completed.get(dependency) if condition == FINISHED else None


def _order_problems(spec: RunSpec, events: Sequence[Event]) -> list[str]:
    """Return how invocations break actor order or `requires`, by durable event order.

    The scheduler runs each actor's actions in its listed order, invoking the
    next only after the previous one's completion is recorded, and invokes a
    dependent only after its prerequisite's invocation (`started`) or
    completion (`finished`) is recorded; a never-invoked action is swept as
    skipped and nothing that depends on it runs. So an invoked action needs
    every earlier action of its actor invoked and completed, and each
    prerequisite invoked or completed, at earlier event indices. Only
    invoked actions are checked, so every durable prefix of a valid history
    stays valid.
    """
    invoked, completed, skipped = _indices(events)
    problems: list[str] = []
    for actor in spec.actors:
        for position, action_id in enumerate(actor.actions):
            at = invoked.get(action_id)
            problems += [
                f"{action_id}: actor {actor.id} invoked it before {earlier} completed"
                for earlier in actor.actions[:position]
                if at is not None and completed.get(earlier, at + 1) > at
            ]
    for action in spec.actions:
        at = invoked.get(action.id)
        if at is None:
            continue
        for dependency, condition in action.requires:
            gate = _gate(condition, dependency, invoked, completed)
            if dependency in skipped:
                problems.append(
                    f"{action.id}: invoked although its prerequisite {dependency} "
                    "was skipped"
                )
            elif gate is None or gate > at:
                problems.append(f"{action.id}: invoked before {dependency} {condition}")
    return problems


def history_problems(
    spec: RunSpec,
    events: Sequence[Event],
    *,
    execution_status: str | None = None,
) -> tuple[str, ...]:
    """Return every way a history breaks the action-event grammar of its spec.

    Every action event names a known action. Each action has no events, one
    ``skipped``, one ``invoke``, or one ``invoke`` followed by exactly one
    ``ok``/``fail``/``info`` at or after it. Invocations keep actor order
    and every ``requires`` gate in durable event order, and nothing that
    requires a skipped action is invoked (`_order_problems`). Only a durable prefix with no
    scheduler cleanup record, from an execution that did not complete, may
    leave actions unresolved, so a killed or failed run stays judgeable.
    At most one scheduler record exists; every action has a terminal
    before it and no action event follows it. ``execution_status`` is the
    containment status, when known: a completed execution requires a
    completed scheduler record. Every record is an `Event` with a text
    type and dict data without the reserved keys. Every event time satisfies
    ``is_event_time``, every event encodes as strict UTF-8 JSON of at most
    ``MAX_EVENT_BYTES``, and the history is at most ``MAX_HISTORY_BYTES``.
    Problems are structural: they make the history invalid rather than a
    property finding. No event time value raises.
    """
    problems = _shape_problems(events)
    if problems:
        return tuple(problems)
    problems = _size_problems(events)
    sequences = _action_sequences(spec, events, problems)
    for action in spec.actions:
        problems.extend(_sequence_problems(action.id, sequences[action.id]))
    problems.extend(_order_problems(spec, events))
    problems.extend(_scheduler_problems(spec, events, sequences, execution_status))
    return tuple(problems)


def event_to_json(event: Event) -> dict[str, Any]:
    """Return the JSON-serializable form of one event.

    Raises ValueError when the data carries a reserved key, which would
    replace the event's own type or time.
    """
    if not RESERVED_DATA_KEYS.isdisjoint(event.data):
        raise ValueError("event data carries a reserved key: type or time")
    return {"type": event.type, "time": event.time, **event.data}


def event_line(event: Event) -> bytes:
    """Return one event's canonical JSONL encoding: sorted keys, compact, strict UTF-8.

    Raises ValueError for text that is not valid UTF-8, a non-finite
    number, or reserved data keys, and TypeError for a value JSON cannot
    represent.
    """
    text = json.dumps(
        event_to_json(event),
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    )
    return text.encode("utf-8") + b"\n"


def event_from_json(raw: Any) -> Event:
    """Parse one JSON-decoded object into an Event; raise ValueError."""
    if type(raw) is not dict:
        raise ValueError(f"history line must be an object, got {raw!r}")
    if type(raw.get("type")) is not str or type(raw.get("time")) not in (int, float):
        raise ValueError(f"history event needs type and time: {raw!r}")
    data = {key: value for key, value in raw.items() if key not in ("type", "time")}
    return Event(type=raw["type"], time=raw["time"], data=data)
