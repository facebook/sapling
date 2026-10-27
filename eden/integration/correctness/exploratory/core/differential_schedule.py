# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Target-neutral differential admission and observation.

Admission is static: a target declares which actions are shared, and
`schedule_problems` admits only schedules where every pair with conflicting
footprints is ordered by a finished edge (actor order or a `finished`
requirement). Each side's result normalizes to one canonical observation
(`observe`); comparing two of them is a direct check that later collectors
own.
"""

from __future__ import annotations

import json
import math
import re
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from typing import Any

from .differential import (
    _check_side,
    PAIRED_SCHEMA_VERSION,
    REFERENCE_RESULT,
    SideResult,
)
from .effects import Footprint, footprints_interact
from .graph_rules import finish_before
from .history import FAIL, INFO, OK
from .spec_types import Action, RunSpec, SpecError


UNORDERED_CONFLICT = "unordered-conflict"


_NAME = re.compile(r"[a-z][a-z0-9-]*")


class MalformedObservation(ValueError):
    """A completed side whose facts cannot form its declared observation."""


@dataclass(frozen=True)
class Applicability:
    """Whether one action runs on both sides, or the stable reason it does not."""

    shared: bool
    reason: str = ""

    def __post_init__(self) -> None:
        if self.shared:
            if self.reason:
                raise ValueError("a shared action carries no not-applicable reason")
            return
        if type(self.reason) is not str or _NAME.fullmatch(self.reason) is None:
            raise ValueError(f"bad not-applicable reason {self.reason!r}")


SHARED = Applicability(True)


def not_applicable(reason: str) -> Applicability:
    """Return the applicability of an action that runs on the primary side only."""
    return Applicability(False, reason)


class Ordering:
    """The finish-before relation of one RunSpec, computed once.

    `before(a, b)` holds when actor order or finished `requires` edges,
    transitively, make `a` finish before `b` starts.
    """

    def __init__(self, spec: RunSpec) -> None:
        self._earlier = {
            action.id: finish_before(spec, action.id) for action in spec.actions
        }

    def before(self, first: str, second: str) -> bool:
        """Return whether `first` must finish before `second` starts."""
        return first in self._earlier[second]

    def ordered(self, first: str, second: str) -> bool:
        """Return whether the two actions are ordered in either direction."""
        return self.before(first, second) or self.before(second, first)


@dataclass(frozen=True)
class ScheduleProblem:
    """Two actions whose footprints conflict and that may run concurrently."""

    first: str
    second: str

    @property
    def reason(self) -> str:
        """Return the stable rejection reason."""
        return UNORDERED_CONFLICT


def schedule_problems(
    spec: RunSpec, footprint: Callable[[RunSpec, Action], Footprint]
) -> tuple[ScheduleProblem, ...]:
    """Return every unordered pair of actions whose footprints conflict.

    A comparable differential schedule has none: every observation is
    ordered after (or before) every conflicting mutation, and no
    concurrent action conflicts with it. A whole-checkout footprint is
    therefore strictly ordered against every action that names any path.
    """
    ordering = Ordering(spec)
    footprints = [(action.id, footprint(spec, action)) for action in spec.actions]
    problems = []
    for index, (first, first_print) in enumerate(footprints):
        for second, second_print in footprints[index + 1 :]:
            if ordering.ordered(first, second):
                continue
            if footprints_interact(first_print, second_print):
                problems.append(ScheduleProblem(first, second))
    return tuple(problems)


def validate_differential(
    spec: RunSpec,
    applicability: Callable[[RunSpec, Action], Applicability],
    footprint: Callable[[RunSpec, Action], Footprint],
) -> None:
    """Admit only shared actions whose conflicting pairs are all ordered; else raise SpecError.

    Every pair with conflicting footprints must be ordered by a finished
    edge. The first action, in spec order, that is not shared is rejected
    with its stable reason; then the first unordered conflicting pair.
    """
    for action in spec.actions:
        verdict = applicability(spec, action)
        if not verdict.shared:
            raise SpecError(
                f"differential action {action.id!r} ({action.kind}) is not "
                f"applicable: {verdict.reason}"
            )
    problems = schedule_problems(spec, footprint)
    if problems:
        first = problems[0]
        raise SpecError(
            f"differential schedule is {first.reason}: {first.first!r} and "
            f"{first.second!r} have conflicting footprints and no ordering"
        )


def reference_facts(reference: SideResult) -> dict[str, Any]:
    """Return the facts that carry the reference side inside the terminal event."""
    _check_side(reference, "reference")
    return {
        REFERENCE_RESULT: {
            "schema_version": PAIRED_SCHEMA_VERSION,
            "outcome": reference.outcome,
            "started": reference.started,
            "completed": reference.completed,
            "facts": dict(reference.facts),
        }
    }


@dataclass(frozen=True)
class SideObservation:
    """One side's canonical observation: its outcome class and canonical value."""

    outcome: str
    value: str


@dataclass(frozen=True)
class ObservationDef:
    """Which facts form a shared action's observation.

    `keys` name the compared facts of a successful side; `unordered`
    names list-valued keys compared as sorted multisets. `conditional`
    maps a key to the compared guard key and the guard values for which it
    is compared at all, such as a size compared only for regular files. A
    failed side is observed by its errno, else by its process return code.
    """

    keys: tuple[str, ...] = ()
    unordered: frozenset[str] = field(default_factory=frozenset)
    conditional: Mapping[str, tuple[str, frozenset[Any]]] = field(default_factory=dict)

    def __post_init__(self) -> None:
        if not set(self.unordered) <= set(self.keys):
            raise ValueError("unordered keys must be compared keys")
        for key, (guard, _values) in self.conditional.items():
            if key not in self.keys or guard not in self.keys or guard == key:
                raise ValueError(f"conditional key {key!r} needs a compared guard")
            if guard in self.conditional:
                raise ValueError(f"guard {guard!r} cannot itself be conditional")


def _normalized(value: Any) -> Any:
    """Return a JSON value with integral floats as ints; reject what JSON cannot hold."""
    if value is None or type(value) in (bool, str, int):
        return value
    if type(value) is float:
        if not math.isfinite(value):
            raise MalformedObservation(f"non-finite number {value!r}")
        return int(value) if value.is_integer() else value
    if type(value) in (list, tuple):
        return [_normalized(item) for item in value]
    if type(value) is dict:
        if any(type(key) is not str for key in value):
            raise MalformedObservation("object keys must be strings")
        return {key: _normalized(item) for key, item in value.items()}
    raise MalformedObservation(f"{type(value).__name__} is not a JSON value")


def canonical_value(value: Any) -> str:
    """Return the canonical JSON text of one observed value.

    Integral floats compare equal to ints; NaN, infinities, and non-JSON
    values raise MalformedObservation.
    """
    return json.dumps(
        _normalized(value),
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    )


def _ordered(value: Any, key: str) -> list[Any]:
    if type(value) not in (list, tuple):
        raise MalformedObservation(f"unordered fact {key!r} must be a list")
    return sorted(value, key=canonical_value)


def _compared(side: SideResult, definition: ObservationDef) -> dict[str, Any]:
    compared: dict[str, Any] = {}
    for key in definition.keys:
        guard = definition.conditional.get(key)
        if guard is not None and side.facts.get(guard[0]) not in guard[1]:
            continue
        if key not in side.facts:
            raise MalformedObservation(f"ok side lacks compared fact {key!r}")
        value = side.facts[key]
        compared[key] = _ordered(value, key) if key in definition.unordered else value
    return compared


def observe(side: SideResult, definition: ObservationDef) -> SideObservation | None:
    """Return one side's canonical observation, or None when it is unresolved.

    Only an unresolved (`info`) side observes nothing. A failed side is
    observed by its stable failure class: its integer errno, else (a process
    that exited nonzero) its integer return code as `{"returncode": code}`.
    A successful side missing a compared fact, a failed side with neither,
    or a fact JSON cannot hold raises MalformedObservation.
    """
    if side.outcome == INFO:
        return None
    if side.outcome == FAIL:
        code = side.facts.get("errno")
        if type(code) is int:
            return SideObservation(FAIL, canonical_value(code))
        returncode = side.facts.get("returncode")
        if type(returncode) is int:
            return SideObservation(FAIL, canonical_value({"returncode": returncode}))
        raise MalformedObservation(
            f"failed side has errno {code!r} and return code {returncode!r}"
        )
    if side.outcome != OK:
        raise MalformedObservation(f"unknown outcome {side.outcome!r}")
    return SideObservation(OK, canonical_value(_compared(side, definition)))
