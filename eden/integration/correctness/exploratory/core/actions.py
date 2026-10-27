# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Target-neutral action schemas for the exploratory correctness harness."""

from __future__ import annotations

import random
from collections.abc import Callable, Mapping
from typing import Any

from .effects import no_footprint
from .facts import EMPTY_RESULT, FieldKind, FieldSpec, ResultSchema
from .runspec import Action, RunSpec, SpecError
from .target import ActionDef, Binding, Grouping


def _sample_none(
    _rng: random.Random, _context: Mapping[str, Any]
) -> Mapping[str, Any] | None:
    return None


def write_sampler(
    sample_path: Callable[[random.Random, Mapping[str, Any]], str],
) -> Callable[[random.Random, Mapping[str, Any]], Mapping[str, Any] | None]:
    """Return the shared write sampler over a target's path sampler.

    Uncontended, each actor writes once (its first input); contended, every
    input may write. The value ID is the action ID, so write values are unique.
    """

    def sample(
        rng: random.Random, context: Mapping[str, Any]
    ) -> Mapping[str, Any] | None:
        if not context.get("contended") and context.get("input_index", 0) > 0:
            return None
        return {"path": sample_path(rng, context), "value_id": context["action_id"]}

    return sample


def _validate_noop(_spec: RunSpec, _action: Action) -> None:
    pass


def _definition(
    kind: str,
    axis: str,
    fields: frozenset[str],
    required: frozenset[str],
    types: Mapping[str, type],
    *,
    result: ResultSchema = EMPTY_RESULT,
    spawns_process: bool = False,
    kills_process: bool = False,
) -> ActionDef:
    return ActionDef(
        kind=kind,
        axis=axis,
        fields=fields,
        required=required,
        types=types,
        validate=_validate_noop,
        modeled=False,
        spawns_process=spawns_process,
        sample=_sample_none,
        result=result,
        footprint=no_footprint,
        roles=frozenset(),
        binding=Binding.NONE,
        grouping=Grouping.NONE,
        kills_process=kills_process,
    )


def _diagnostic(name: str, kind: FieldKind, required: bool = False) -> FieldSpec:
    return FieldSpec(name, kind, required=required, consumed=False)


_FAULT_ID = _diagnostic("fault_id", FieldKind.TEXT, required=True)
_FAULT_INSTALL_RESULT = ResultSchema(
    ok=(
        _FAULT_ID,
        _diagnostic("selector", FieldKind.TEXT),
        _diagnostic("ordinal", FieldKind.INT),
    )
)
_FAULT_AWAIT_RESULT = ResultSchema(
    ok=(
        _FAULT_ID,
        _diagnostic("hit", FieldKind.BOOL),
        _diagnostic("witness", FieldKind.TEXT),
        _diagnostic("target_completed", FieldKind.BOOL),
        _diagnostic("target_outcome", FieldKind.TEXT),
    )
)
_FAULT_REMOVE_RESULT = ResultSchema(
    ok=(_FAULT_ID, _diagnostic("removed", FieldKind.INT, required=True))
)
_KILL_RESULT = ResultSchema(
    ok=(
        _diagnostic("target_action", FieldKind.TEXT, required=True),
        _diagnostic("pgid", FieldKind.INT, required=True),
        _diagnostic("group_empty", FieldKind.BOOL, required=True),
        _diagnostic("already_exited", FieldKind.BOOL),
    )
)
_BLOCKED_RESULT = ResultSchema(ok=(_diagnostic("blocked", FieldKind.JSON),))


CORE_ACTION_DEFS = {
    "sleep": _definition(
        "sleep",
        "input",
        frozenset({"seconds"}),
        frozenset({"seconds"}),
        {"seconds": float},
    ),
    "fault_install": _definition(
        "fault_install",
        "nemesis",
        frozenset(
            {
                "fault_id",
                "site",
                "target_action",
                "behavior",
                "ordinal",
                "delay_ms",
                "key",
            }
        ),
        frozenset({"fault_id", "site", "target_action", "behavior"}),
        {
            "fault_id": str,
            "site": str,
            "target_action": str,
            "behavior": str,
            "ordinal": int,
            "delay_ms": int,
            "key": str,
        },
        result=_FAULT_INSTALL_RESULT,
    ),
    "fault_await": _definition(
        "fault_await",
        "nemesis",
        frozenset({"fault_id"}),
        frozenset({"fault_id"}),
        {"fault_id": str},
        result=_FAULT_AWAIT_RESULT,
    ),
    "fault_remove": _definition(
        "fault_remove",
        "nemesis",
        frozenset({"fault_id"}),
        frozenset({"fault_id"}),
        {"fault_id": str},
        result=_FAULT_REMOVE_RESULT,
    ),
    "kill_process": _definition(
        "kill_process",
        "nemesis",
        frozenset({"target_action", "after_ms"}),
        frozenset({"target_action", "after_ms"}),
        {"target_action": str, "after_ms": int},
        result=_KILL_RESULT,
        kills_process=True,
    ),
    "blocked_faults": _definition(
        "blocked_faults",
        "observation",
        frozenset(),
        frozenset(),
        {},
        result=_BLOCKED_RESULT,
    ),
}


def compose_action_defs(
    *tables: Mapping[str, ActionDef],
) -> dict[str, ActionDef]:
    """Combine action tables, rejecting duplicate kinds."""
    result: dict[str, ActionDef] = {}
    for table in tables:
        collisions = sorted(set(result).intersection(table))
        if collisions:
            raise SpecError(f"action kind collision: {collisions}")
        result.update(table)
    return result
