# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Typed, target-neutral RunSpec input for exploratory correctness.

The vocabulary and dataclasses live in ``spec_types``; the graph and
fault-lifetime rules in ``graph_rules``. This module parses and serializes
specs and re-exports both so callers keep one import path.
"""

from __future__ import annotations

import json
import math
from typing import Any, Mapping

from .graph_rules import (
    finish_before,
    may_overlap,
    must_end_before,
    reachable,
    validate,
)
from .jsonio import loads, MAX_ID_BYTES, MAX_INTEGER, MAX_SCALAR_BYTES
from .spec_types import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    DEADLINE_RUN,
    DEADLINES,
    Environment,
    EVENTS,
    FINISHED,
    MAX_ACTIONS,
    RunSpec,
    SCHEMA,
    SCHEMA_VERSION,
    SpecError,
    STARTED,
)
from .target import Target

__all__ = [
    "Action",
    "Actor",
    "Budgets",
    "DEADLINE_OPERATION",
    "DEADLINE_RUN",
    "DEADLINES",
    "DEFAULT_BUDGETS",
    "DEFAULT_RUNNER_DEADLINE_SECONDS",
    "Environment",
    "EVENTS",
    "FINISHED",
    "MAX_ACTIONS",
    "MAX_ACTORS",
    "MAX_INTEGER",
    "MAX_JSON_DEPTH",
    "MAX_JSON_VALUES",
    "MAX_RUNSPEC_BYTES",
    "RunSpec",
    "SCHEMA",
    "SCHEMA_VERSION",
    "STARTED",
    "SpecError",
    "finish_before",
    "from_json",
    "json_value_problem",
    "may_overlap",
    "must_end_before",
    "reachable",
    "spec_problems",
    "strict_loads",
    "to_json",
    "validate",
]

_COMMON_FIELDS = frozenset({"kind", "deadline", "requires"})
_COMMON_REQUIRED = frozenset({"kind", "deadline"})

_TOP_LEVEL_FIELDS = frozenset(
    {
        "schema",
        "schema_version",
        "seed",
        "target",
        "environment",
        "actors",
        "actions",
    }
)
_TOP_LEVEL_OPTIONAL = frozenset({"metadata", "budgets"})

# The one default whole-run deadline. An omitted operation or cleanup
# deadline is that fixed fraction of the run deadline: 120 s and 30 s of
# the default 600 s.
DEFAULT_RUNNER_DEADLINE_SECONDS = 600
OPERATION_DEADLINE_FRACTION = 5
CLEANUP_DEADLINE_FRACTION = 20

MAX_RUNSPEC_BYTES = 1024 * 1024
MAX_ACTORS = 1000
MAX_JSON_DEPTH = 64
MAX_JSON_VALUES = MAX_RUNSPEC_BYTES
_PATH_KEY_CHARS = 64


def _require_type(value: Any, types: tuple[type, ...], what: str) -> None:
    # type(x) is X (not isinstance) so bools never pass as ints.
    if type(value) not in types:
        names = ", ".join(t.__name__ for t in types)
        raise SpecError(f"{what} must be {names}, got {value!r}")


def _require_exact_keys(
    raw: Any, allowed: frozenset[str], what: str, required: bool = True
) -> dict[str, Any]:
    if type(raw) is not dict:
        raise SpecError(f"{what} must be an object, got {raw!r}")
    unknown = sorted(set(raw) - allowed)
    if unknown:
        raise SpecError(f"{what} has unknown fields: {unknown}")
    if required:
        missing = sorted(allowed - set(raw))
        if missing:
            raise SpecError(f"{what} is missing fields: {missing}")
    return raw


def _parse_action(action_id: str, raw: Any, target: Target) -> Action:
    if type(action_id) is not str or not action_id:
        raise SpecError(f"action id must be a non-empty string, got {action_id!r}")
    if type(raw) is not dict:
        raise SpecError(f"action {action_id!r} must be an object, got {raw!r}")
    kind = raw.get("kind")
    if type(kind) is not str or kind not in target.actions:
        raise SpecError(f"action {action_id!r} has unknown kind {kind!r}")
    action_def = target.actions[kind]
    allowed = _COMMON_FIELDS | action_def.fields
    required = _COMMON_REQUIRED | action_def.required
    body = _require_exact_keys(raw, allowed, f"action {action_id!r}", required=False)
    missing = sorted(required - set(body))
    if missing:
        raise SpecError(f"action {action_id!r} is missing fields: {missing}")
    deadline = body["deadline"]
    if deadline not in DEADLINES:
        raise SpecError(f"action {action_id!r} has bad deadline {deadline!r}")
    requires_raw = body.get("requires", [])
    if type(requires_raw) is not list:
        raise SpecError(f"action {action_id!r} requires must be a list")
    requires: list[tuple[str, str]] = []
    for entry in requires_raw:
        if (
            type(entry) is not list
            or len(entry) != 2
            or type(entry[0]) is not str
            or entry[1] not in EVENTS
        ):
            raise SpecError(
                f"action {action_id!r} has bad requires entry {entry!r}; "
                'expected [action_id, "started"|"finished"]'
            )
        requires.append((entry[0], entry[1]))
    payload = {name: raw[name] for name in action_def.fields if name in raw}
    for name, expected_type in action_def.types.items():
        if name in payload:
            _require_type(
                payload[name], (expected_type,), f"action {action_id!r} field {name!r}"
            )
    return Action(
        id=action_id,
        kind=kind,
        deadline=deadline,
        requires=tuple(requires),
        payload=payload,
    )


def _parse_environment(raw: Any, target: Target) -> Environment:
    body = _require_exact_keys(raw, frozenset({"target"}), "environment")
    target_body = _require_exact_keys(
        body["target"], frozenset(target.environment_fields), "environment.target"
    )
    for name, expected_type in target.environment_fields.items():
        _require_type(target_body[name], (expected_type,), f"environment.target.{name}")
    return Environment(target=target_body)


def _parse_budgets(raw: Any) -> Budgets:
    """Parse the deadlines; each is optional, with one default whole-run deadline.

    An omitted run deadline is `DEFAULT_RUNNER_DEADLINE_SECONDS`; an
    omitted operation or cleanup deadline is the run deadline divided by
    its fixed fraction.
    """
    names = frozenset(
        {
            "operation_deadline_seconds",
            "cleanup_deadline_seconds",
            "runner_deadline_seconds",
        }
    )
    body = dict(_require_exact_keys(raw, names, "budgets", required=False))
    for name, value in body.items():
        _require_type(value, (int, float), f"budgets.{name}")
        if not math.isfinite(value) or value <= 0:
            raise SpecError(f"budgets.{name} must be finite and positive")
    runner = body.setdefault("runner_deadline_seconds", DEFAULT_RUNNER_DEADLINE_SECONDS)
    body.setdefault("operation_deadline_seconds", runner / OPERATION_DEADLINE_FRACTION)
    body.setdefault("cleanup_deadline_seconds", runner / CLEANUP_DEADLINE_FRACTION)
    budgets = Budgets(
        operation_deadline_seconds=body["operation_deadline_seconds"],
        cleanup_deadline_seconds=body["cleanup_deadline_seconds"],
        runner_deadline_seconds=body["runner_deadline_seconds"],
    )
    problems = _budget_problems(budgets)
    if problems:
        raise SpecError(problems[0])
    return budgets


def _utf8_size(value: str, what: str) -> int:
    try:
        return len(value.encode("utf-8"))
    except UnicodeEncodeError as error:
        raise SpecError(f"{what} is not valid UTF-8 text") from error


def _utf8_len_or_none(value: str) -> int | None:
    try:
        return len(value.encode("utf-8"))
    except UnicodeEncodeError:
        return None


def _key_label(key: str) -> str:
    return key if len(key) <= _PATH_KEY_CHARS else key[:_PATH_KEY_CHARS] + "…"


def _scalar_problem(value: Any, where: str) -> str | None:
    """Return why one scalar is outside the RunSpec bounds, or None for a container."""
    kind = type(value)
    if value is None or kind is bool:
        return None
    if kind is int:
        if -MAX_INTEGER <= value <= MAX_INTEGER:
            return None
        return f"{where} is an integer outside ±2**53"
    if kind is float:
        return None if math.isfinite(value) else f"{where} is not a finite number"
    if kind is str:
        size = _utf8_len_or_none(value)
        if size is None:
            return f"{where} is not valid UTF-8 text"
        if size > MAX_SCALAR_BYTES:
            return f"{where} exceeds {MAX_SCALAR_BYTES} UTF-8 bytes"
        return None
    if kind in (list, tuple, dict):
        return None
    return f"{where} is a {kind.__name__}, not a JSON value"


def json_value_problem(value: Any, where: str) -> str | None:
    """Return the first way a value is not bounded RunSpec JSON data, or None.

    Bounded data is null, a boolean, an integer within ±``MAX_INTEGER``, a
    finite float, a string of at most ``MAX_SCALAR_BYTES`` UTF-8 bytes, or a
    list, tuple, or string-keyed dict of bounded data, nested at most
    ``MAX_JSON_DEPTH`` deep with at most ``MAX_JSON_VALUES`` values in all.
    Never raises, so in-memory input is judged rather than trusted.
    """
    pending: list[tuple[Any, str, int]] = [(value, where, 0)]
    seen = 0
    while pending:
        item, path, depth = pending.pop()
        seen += 1
        if seen > MAX_JSON_VALUES:
            return f"{where} holds more than {MAX_JSON_VALUES} values"
        if depth > MAX_JSON_DEPTH:
            return f"{path} nests deeper than {MAX_JSON_DEPTH}"
        problem = _scalar_problem(item, path)
        if problem is not None:
            return problem
        if type(item) is dict:
            for key, child in item.items():
                if type(key) is not str:
                    return f"{path} has a {type(key).__name__} key, not a string"
                key_problem = _scalar_problem(key, f"{path} key")
                if key_problem is not None:
                    return key_problem
                pending.append((child, f"{path}.{_key_label(key)}", depth + 1))
        elif type(item) in (list, tuple):
            pending.extend(
                (child, f"{path}[{index}]", depth + 1)
                for index, child in enumerate(item)
            )
    return None


def strict_loads(text: str) -> Any:
    """Decode strict JSON (`jsonio.loads`, integers within ±2**53); raise SpecError."""
    try:
        return loads(text, bounded=True)
    except ValueError as error:
        raise SpecError(f"RunSpec is not valid JSON: {error}") from error


def _check_id(value: str, what: str) -> None:
    if _utf8_size(value, what) > MAX_ID_BYTES:
        raise SpecError(f"{what} {value[:32]!r}... exceeds {MAX_ID_BYTES} UTF-8 bytes")


def _check_bounds(body: Mapping[str, Any]) -> None:
    """Enforce the decoded RunSpec's bounds before any field is parsed.

    At most MAX_ACTIONS actions and MAX_ACTORS actors; action and actor IDs
    are at most MAX_ID_BYTES and every other string at most MAX_SCALAR_BYTES.
    """
    actions = body.get("actions")
    if type(actions) is dict:
        if len(actions) > MAX_ACTIONS:
            raise SpecError(f"{len(actions)} actions exceed the action limit")
        for action_id in actions:
            _check_id(action_id, "action id")
    actors = body.get("actors")
    if type(actors) is list:
        if len(actors) > MAX_ACTORS:
            raise SpecError(f"{len(actors)} actors exceed the actor limit")
        for actor in actors:
            if type(actor) is dict and type(actor.get("id")) is str:
                _check_id(actor["id"], "actor id")
    problem = json_value_problem(dict(body), "RunSpec")
    if problem is not None:
        raise SpecError(problem)


def from_json(text: str, target: Target) -> RunSpec:
    """Parse and validate a RunSpec; raise SpecError on any violation."""
    if type(text) is not str:
        raise SpecError(f"RunSpec must be text, got {type(text).__name__}")
    if _utf8_size(text, "RunSpec") > MAX_RUNSPEC_BYTES:
        raise SpecError(f"RunSpec exceeds {MAX_RUNSPEC_BYTES} UTF-8 bytes")
    raw = strict_loads(text)
    if type(raw) is dict:
        _check_bounds(raw)
    body = _require_exact_keys(
        raw, _TOP_LEVEL_FIELDS | _TOP_LEVEL_OPTIONAL, "RunSpec", required=False
    )
    missing = sorted(_TOP_LEVEL_FIELDS - set(body))
    if missing:
        raise SpecError(f"RunSpec is missing fields: {missing}")
    metadata = body.get("metadata", {})
    _require_type(metadata, (dict,), "metadata")
    _check_header(body, target)
    environment = _parse_environment(body["environment"], target)
    budgets = _parse_budgets(body.get("budgets", {}))
    actors = _parse_actors(body["actors"])
    if type(body["actions"]) is not dict or not body["actions"]:
        raise SpecError("actions must be a non-empty object")
    actions = tuple(
        _parse_action(action_id, entry, target)
        for action_id, entry in sorted(body["actions"].items())
    )
    spec = RunSpec(
        seed=body["seed"],
        target=body["target"],
        environment=environment,
        budgets=budgets,
        actors=actors,
        actions=actions,
        metadata=metadata,
    )
    validate(spec, target)
    problems = spec_problems(spec)
    if problems:
        raise SpecError(problems[0])
    return spec


DEFAULT_BUDGETS = Budgets(
    operation_deadline_seconds=DEFAULT_RUNNER_DEADLINE_SECONDS
    / OPERATION_DEADLINE_FRACTION,
    cleanup_deadline_seconds=DEFAULT_RUNNER_DEADLINE_SECONDS
    / CLEANUP_DEADLINE_FRACTION,
    runner_deadline_seconds=DEFAULT_RUNNER_DEADLINE_SECONDS,
)


def _check_header(body: Mapping[str, Any], target: Target) -> None:
    """Require the supported schema, an integer seed, and the selected target's name."""
    if body["schema"] != SCHEMA or body["schema_version"] != SCHEMA_VERSION:
        raise SpecError(
            f"RunSpec schema must be {SCHEMA!r} version {SCHEMA_VERSION}, "
            f"got {body['schema']!r} version {body['schema_version']!r}"
        )
    _require_type(body["seed"], (int,), "seed")
    _require_type(body["target"], (str,), "target")
    if body["target"] != target.name:
        raise SpecError(
            f"RunSpec target {body['target']!r} does not match {target.name!r}"
        )


def _parse_actors(raw: Any) -> tuple[Actor, ...]:
    if type(raw) is not list or not raw:
        raise SpecError("actors must be a non-empty list")
    actors: list[Actor] = []
    seen_actor_ids: set[str] = set()
    for entry in raw:
        actor_body = _require_exact_keys(entry, frozenset({"id", "actions"}), "actor")
        _require_type(actor_body["id"], (str,), "actor id")
        if not actor_body["id"] or actor_body["id"] in seen_actor_ids:
            raise SpecError(f"actor id must be unique and non-empty: {entry!r}")
        seen_actor_ids.add(actor_body["id"])
        if type(actor_body["actions"]) is not list:
            raise SpecError(f"actor {actor_body['id']!r} actions must be a list")
        for action_id in actor_body["actions"]:
            _require_type(action_id, (str,), "actor action id")
        actors.append(Actor(id=actor_body["id"], actions=tuple(actor_body["actions"])))
    return tuple(actors)


def _action_to_json(action: Action) -> dict[str, Any]:
    body: dict[str, Any] = {
        "kind": action.kind,
        "deadline": action.deadline,
    }
    if action.requires:
        body["requires"] = [[dep, event] for dep, event in action.requires]
    for name in sorted(action.payload):
        body[name] = action.payload[name]
    return body


def _encode(spec: RunSpec) -> str:
    raw: dict[str, Any] = {
        "schema": SCHEMA,
        "schema_version": SCHEMA_VERSION,
        "seed": spec.seed,
        "target": spec.target,
        "environment": {"target": dict(spec.environment.target)},
        "budgets": {
            "operation_deadline_seconds": spec.budgets.operation_deadline_seconds,
            "cleanup_deadline_seconds": spec.budgets.cleanup_deadline_seconds,
            "runner_deadline_seconds": spec.budgets.runner_deadline_seconds,
        },
        "actors": [
            {"id": actor.id, "actions": list(actor.actions)} for actor in spec.actors
        ],
        "actions": {action.id: _action_to_json(action) for action in spec.actions},
    }
    if spec.metadata:
        raw["metadata"] = dict(spec.metadata)
    return json.dumps(raw, sort_keys=True, indent=2, allow_nan=False) + "\n"


def _sequence(value: Any) -> bool:
    return type(value) in (list, tuple)


def _id_problem(value: Any, what: str) -> str | None:
    if type(value) is not str or not value:
        return f"{what} must be a non-empty string"
    size = _utf8_len_or_none(value)
    if size is None:
        return f"{what} is not valid UTF-8 text"
    if size > MAX_ID_BYTES:
        return f"{what} exceeds {MAX_ID_BYTES} UTF-8 bytes"
    return None


def _budget_problems(budgets: Any) -> list[str]:
    if type(budgets) is not Budgets:
        return ["budgets must be Budgets"]
    problems = []
    for name in (
        "operation_deadline_seconds",
        "cleanup_deadline_seconds",
        "runner_deadline_seconds",
    ):
        value = getattr(budgets, name)
        if type(value) not in (int, float):
            problems.append(f"budgets.{name} must be a number")
        elif _scalar_problem(value, f"budgets.{name}") or not value > 0:
            problems.append(f"budgets.{name} must be finite, positive, and ±2**53")
    if problems:
        return problems
    cleanup = budgets.cleanup_deadline_seconds
    if cleanup >= budgets.runner_deadline_seconds:
        problems.append("cleanup budget must be less than runner budget")
    elif budgets.operation_deadline_seconds > budgets.runner_deadline_seconds - cleanup:
        problems.append("operation budget must fit within runner minus cleanup")
    return problems


def _actor_problems(actors: Any) -> list[str]:
    if not _sequence(actors) or len(actors) > MAX_ACTORS:
        return [f"actors must be a sequence of at most {MAX_ACTORS}"]
    problems = []
    for index, actor in enumerate(actors):
        if type(actor) is not Actor:
            problems.append(f"actors[{index}] must be an Actor")
            continue
        problems += filter(None, [_id_problem(actor.id, f"actors[{index}].id")])
        if not _sequence(actor.actions) or len(actor.actions) > MAX_ACTIONS:
            problems.append(f"actors[{index}].actions must be a bounded sequence")
            continue
        problems += filter(
            None,
            (
                _id_problem(item, f"actors[{index}].actions[{position}]")
                for position, item in enumerate(actor.actions)
            ),
        )
    return problems


def _action_problems(actions: Any) -> list[str]:
    if not _sequence(actions) or len(actions) > MAX_ACTIONS:
        return [f"actions must be a sequence of at most {MAX_ACTIONS}"]
    problems = []
    seen: set[str] = set()
    for index, action in enumerate(actions):
        where = f"actions[{index}]"
        if type(action) is not Action:
            problems.append(f"{where} must be an Action")
            continue
        if type(action.id) is str:
            if action.id in seen:
                problems.append(f"{where}.id {action.id!r} is a duplicate")
            seen.add(action.id)
        problems += filter(
            None,
            [
                _id_problem(action.id, f"{where}.id"),
                _id_problem(action.kind, f"{where}.kind"),
            ],
        )
        if type(action.deadline) is not str or action.deadline not in DEADLINES:
            problems.append(f"{where}.deadline must be one of {sorted(DEADLINES)}")
        if not _sequence(action.requires) or len(action.requires) > MAX_ACTIONS:
            problems.append(f"{where}.requires must be a bounded sequence")
        else:
            for entry in action.requires:
                if (
                    not _sequence(entry)
                    or len(entry) != 2
                    or _id_problem(entry[0], "requires")
                    or type(entry[1]) is not str
                    or entry[1] not in EVENTS
                ):
                    problems.append(f"{where}.requires has a malformed entry")
                    break
        if type(action.payload) is not dict:
            problems.append(f"{where}.payload must be a dict")
        else:
            problem = json_value_problem(action.payload, f"{where}.payload")
            problems += [problem] if problem else []
    return problems


def spec_problems(spec: Any) -> tuple[str, ...]:
    """Return every way an in-memory spec breaks the RunSpec type and value bounds.

    This is structural: field types, bounded identifiers and counts,
    numbers within ±2**53 and finite, bounded JSON payload and metadata,
    and a canonical encoding of at most ``MAX_RUNSPEC_BYTES``. It never
    raises, and a spec without problems always encodes with ``to_json``.
    Target-specific graph rules are ``validate``'s.
    """
    if type(spec) is not RunSpec:
        return (f"RunSpec must be a RunSpec, got {type(spec).__name__}",)
    problems: list[str] = []
    if type(spec.seed) is not int:
        problems.append("seed must be an integer")
    elif _scalar_problem(spec.seed, "seed"):
        problems.append("seed is an integer outside ±2**53")
    problems += filter(None, [_id_problem(spec.target, "target")])
    if (
        type(spec.environment) is not Environment
        or type(spec.environment.target) is not dict
    ):
        problems.append("environment.target must be a dict")
    else:
        problem = json_value_problem(spec.environment.target, "environment.target")
        problems += [problem] if problem else []
    problems += _budget_problems(spec.budgets)
    problems += _actor_problems(spec.actors)
    problems += _action_problems(spec.actions)
    if type(spec.metadata) is not dict:
        problems.append("metadata must be a dict")
    else:
        problem = json_value_problem(spec.metadata, "metadata")
        problems += [problem] if problem else []
    if problems:
        return tuple(problems)
    size = _utf8_len_or_none(_encode(spec))
    if size is None or size > MAX_RUNSPEC_BYTES:
        return (f"canonical RunSpec exceeds {MAX_RUNSPEC_BYTES} UTF-8 bytes",)
    return ()


def to_json(spec: RunSpec) -> str:
    """Serialize a spec with stable key order.

    The text round-trips through from_json for a spec that is valid for its
    target; target rules (payload fields, action ownership) are checked
    only when parsing.

    Raises SpecError, naming the first problem, for a spec that
    ``spec_problems`` rejects; the encoding is never non-standard JSON.
    """
    problems = spec_problems(spec)
    if problems:
        raise SpecError(problems[0])
    return _encode(spec)
