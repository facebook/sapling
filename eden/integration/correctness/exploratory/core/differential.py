# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Target-neutral differential paired results.

A differential run executes each shared action once on the primary target
and once on a reference implementation. The reference side's outcome is a
fact inside the action's single terminal event, keyed by the original
action ID, so the history grammar is unchanged. Admission and observation
are in `differential_schedule`.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

from .history import COMPLETIONS, is_event_time, time_label

REFERENCE_RESULT = "reference_result"
PAIRED_SCHEMA_VERSION = 1

_SIDE_KEYS = frozenset({"schema_version", "outcome", "started", "completed", "facts"})


@dataclass(frozen=True)
class SideResult:
    """One side's outcome, facts, and monotonic invocation and completion times."""

    outcome: str
    facts: Mapping[str, Any]
    started: float
    completed: float


def _time(raw: Any, what: str) -> float:
    if not is_event_time(raw):
        raise ValueError(
            f"{what} must be a number within ±2**53 seconds, got {time_label(raw)}"
        )
    return float(raw)


def _check_side(side: SideResult, what: str) -> None:
    if type(side.outcome) is not str or side.outcome not in COMPLETIONS:
        raise ValueError(f"{what} outcome must be one of {sorted(COMPLETIONS)}")
    if not isinstance(side.facts, Mapping):
        raise ValueError(f"{what} facts must be an object")
    if any(type(key) is not str for key in side.facts):
        raise ValueError(f"{what} fact keys must be strings")
    if REFERENCE_RESULT in side.facts or "action" in side.facts:
        raise ValueError(f"{what} facts cannot nest a side or rename the action")
    started = _time(side.started, f"{what} start")
    if _time(side.completed, f"{what} completion") < started:
        raise ValueError(f"{what} completes before it starts")


def read_side(raw: Any) -> SideResult:
    """Return the reference side a terminal event carries; raise ValueError if malformed.

    The side has exactly the paired keys, schema version 1, an outcome
    among the completions, an object of facts, and bounded times that do
    not run backwards.
    """
    if type(raw) is not dict or set(raw) != _SIDE_KEYS:
        found = sorted(raw) if type(raw) is dict else type(raw).__name__
        raise ValueError(
            f"reference result must have exactly {sorted(_SIDE_KEYS)}, got {found}"
        )
    version = raw["schema_version"]
    if type(version) is not int or version != PAIRED_SCHEMA_VERSION:
        raise ValueError(f"reference result schema_version must be 1, got {version!r}")
    if type(raw["facts"]) is not dict:
        raise ValueError("reference facts must be an object")
    side = SideResult(
        raw["outcome"],
        dict(raw["facts"]),
        _time(raw["started"], "reference start"),
        _time(raw["completed"], "reference completion"),
    )
    _check_side(side, "reference")
    return side
