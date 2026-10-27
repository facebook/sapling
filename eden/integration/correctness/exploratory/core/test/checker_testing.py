# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Checker test fixtures: toy specs, targets, properties, and verdict accessors."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import replace
from typing import Any, cast

from eden.integration.correctness.exploratory.core import effects
from eden.integration.correctness.exploratory.core.checker import (
    CORE_ACTIVITY,
    CORE_DAEMON_EXIT,
    CORE_INPUT_LEGALITY,
    CORE_RESULT_SCHEMA,
    PropertyReport,
    Verdict,
)
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    EffectGroup,
    EffectKind,
    eq,
    Expect,
    Observation,
    Status,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import (
    EMPTY_RESULT,
    ErrnoUse,
    FieldKind,
    FieldSpec,
    HistoryFacts,
    ResultSchema,
)
from eden.integration.correctness.exploratory.core.history import INVOKE, OK
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    PropertyDef,
    RuleDef,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    Environment,
    RunSpec,
)
from eden.integration.correctness.exploratory.core.target import (
    ActionDef,
    Binding,
    Grouping,
    TargetSources,
)

from ..history import Event
from ..target import Target


def spec(actions: list[Action]) -> RunSpec:
    return RunSpec(
        seed=7,
        target="synthetic",
        environment=Environment(target={}),
        budgets=Budgets(30.0, 5.0, 60.0),
        actors=(Actor(id="a", actions=tuple(action.id for action in actions)),),
        actions=tuple(actions),
    )


def action(action_id: str, kind: str, **payload: Any) -> Action:
    return Action(
        id=action_id,
        kind=kind,
        deadline=DEADLINE_OPERATION,
        payload=payload,
    )


INPUT_RESULT = ResultSchema(fail=(FieldSpec("errno", FieldKind.INT, required=False),))


# The core classifies only a modeled input's failure.
UNMODELED_INPUT_RESULT = ResultSchema(
    fail=(FieldSpec("errno", FieldKind.INT, required=False, consumed=False),)
)


def root_footprint(_spec: RunSpec, _action: Action) -> effects.Footprint:
    return effects.Footprint(subtrees_read=frozenset({effects.ROOT}))


def default_result(axis: str, modeled: bool) -> ResultSchema:
    if axis != "input":
        return EMPTY_RESULT
    return INPUT_RESULT if modeled else UNMODELED_INPUT_RESULT


def action_def(
    kind: str,
    axis: str,
    *,
    modeled: bool = False,
    result: ResultSchema | None = None,
) -> ActionDef:
    """A synthetic action; a modeled input reads the whole checkout (an aggregate)."""
    aggregate = modeled and axis == "input"
    return ActionDef(
        kind=kind,
        axis=axis,
        fields=frozenset(),
        required=frozenset(),
        types={},
        validate=lambda _spec, _action: None,
        modeled=modeled,
        spawns_process=False,
        sample=lambda _rng, _context: None,
        result=result or default_result(axis, modeled),
        footprint=root_footprint if aggregate else effects.no_footprint,
        roles=frozenset(),
        binding=Binding.AGGREGATE if aggregate else Binding.NONE,
        grouping=Grouping.NONE,
    )


def event(type: str, action_id: str, time: float, **facts: Any) -> Event:
    return Event(type=type, time=time, data={"action": action_id, **facts})


def never(*_: Any) -> bool:
    return False


PATH = Subject(SubjectKind.PATH, "path:file")


Collect = Callable[[CheckInput, CollectorSink], None]


def rule(rule_id: str, **overrides: Any) -> RuleDef:
    fields: dict[str, Any] = {
        "id": rule_id,
        "statement": "observations agree with the model",
        "subject_kind": SubjectKind.PATH,
    }
    return RuleDef(**{**fields, **overrides})


def prop(pid: str, collect: Collect, **overrides: Any) -> PropertyDef:
    fields: dict[str, Any] = {
        "id": pid,
        "collect": collect,
        "rules": {f"{pid}.bad": rule(f"{pid}.bad")},
        "not_evaluated_reasons": {f"{pid}.no-input": "nothing to judge"},
    }
    return PropertyDef(**{**fields, **overrides})


def violate(
    sink: CollectorSink,
    rule_id: str,
    scope: str,
    attribution: Attribution,
    reason: str,
    subject: Subject = PATH,
    witnesses: tuple[Witness, ...] = (),
) -> None:
    ref = sink.obligation(rule_id, scope, subject=subject)
    sink.violated(ref, attribution, reason, witnesses)


def finding_property(
    pid: str, *findings: tuple[Attribution, str], satisfied: int = 1
) -> PropertyDef:
    """A property that satisfies `satisfied` obligations and violates one per finding."""

    def collect(_input: CheckInput, sink: CollectorSink) -> None:
        for index in range(satisfied):
            sink.satisfied(sink.obligation(f"{pid}.bad", f"ok{index}", subject=PATH))
        for index, (attribution, reason) in enumerate(findings):
            violate(sink, f"{pid}.bad", f"v{index}", attribution, reason)

    return prop(pid, collect)


def quiet(pid: str = "quiet") -> PropertyDef:
    return finding_property(pid)


def target(*properties: PropertyDef, **overrides: Any) -> Target:
    fields: dict[str, Any] = {
        "name": "synthetic",
        "actions": {"read": action_def("read", "input", modeled=True)},
        "sites": {},
        "properties": properties,
        "install_witnessed": never,
        "environment_fields": {},
        "make_environment": cast(Any, lambda _spec: None),
        "make_backend": cast(Any, lambda _environment, _spec, _red_control: None),
        "red_controls": frozenset(),
        "sources": TargetSources(implementation=__name__),
    }
    return Target(**{**fields, **overrides})


def check_input(spec: RunSpec, events: list[Event], target: Target) -> CheckInput:
    facts = HistoryFacts.build(spec, tuple(events), target)
    return CheckInput(spec, facts, None, effects.EffectSet())


def input_target(
    *,
    errnos: dict[str, ErrnoUse] | None = None,
    witnessed: Any = never,
    timeout_capable: Any = None,
) -> Target:
    result = ResultSchema(
        fail=(FieldSpec("errno", FieldKind.INT, required=False),),
        errnos=errnos or {},
    )
    return target(
        quiet(),
        actions={
            "write": action_def("write", "input", modeled=True, result=result),
            "read": action_def("read", "input", modeled=True, result=result),
            "sleep": action_def("sleep", "input"),
            "switch": action_def("switch", "input"),
            "fault_install": action_def("fault_install", "nemesis"),
            "fault_remove": action_def("fault_remove", "nemesis"),
            "kill_process": action_def("kill_process", "nemesis"),
            "background_read": replace(
                action_def("background_read", "nemesis"), spawns_process=True
            ),
        },
        install_witnessed=witnessed,
        timeout_capable=timeout_capable,
    )


def reads_path(_spec: RunSpec, action: Action) -> effects.Footprint:
    return effects.Footprint(reads=frozenset({action.payload["path"]}))


def path_input_target(**overrides: Any) -> Target:
    """`_input_target` whose reads are bound to their payload path."""
    base = input_target(**overrides)
    read = replace(base.actions["read"], footprint=reads_path, binding=Binding.PATH)
    return replace(base, actions={**base.actions, "read": read})


def report(verdict: Verdict) -> PropertyReport:
    assert verdict.report is not None
    return verdict.report


def result(verdict: Verdict, pid: str) -> Any:
    return next(r for r in report(verdict).results if r.property_id == pid)


def statuses(result: Any) -> list[tuple[str, str]]:
    return [(o.scope, str(o.status)) for o in result.obligations]


READ_OK = [event(INVOKE, "r", 1.0), event(OK, "r", 2.0)]


CORE_IDS = [CORE_DAEMON_EXIT, CORE_ACTIVITY, CORE_INPUT_LEGALITY, CORE_RESULT_SCHEMA]


FILE_CELL = effects.Cell("primary", "path", "file")


class Pinned:
    """Pinned facts over a finite mapping; every other cell is unknown."""

    def __init__(self, values: dict[effects.Cell, Value]) -> None:
        self._values = values

    def initial(self, cell: effects.Cell) -> Value:
        if cell not in self._values:
            raise effects.UnknownInitial(str(cell))
        return self._values[cell]


def pinned(value: Value) -> Callable[[RunSpec], Pinned]:
    return lambda _spec: Pinned({FILE_CELL: value})


def group(group_id: str = "g1", **overrides: Any) -> EffectGroup:
    fields: dict[str, Any] = {
        "id": group_id,
        "action": "r",
        "status": Status.ACKED,
        "window": Window(1.0, 2.0),
        "effects": (
            effects.Effect(FILE_CELL, EffectKind.PRODUCE, Value("file"), None, "r"),
        ),
    }
    return EffectGroup(**{**fields, **overrides})


def observation(
    ref: effects.ObligationRef, node: str = "o1", **overrides: Any
) -> Observation:
    fields: dict[str, Any] = {
        "id": node,
        "action": "r",
        "cells": (FILE_CELL,),
        "expect": Expect(((eq(ABSENT),),)),
        "window": Window(1.0, 2.0),
        "channel": "fuse",
        "obligations": (ref,),
    }
    return Observation(**{**fields, **overrides})
