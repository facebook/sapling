# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Strict action result schemas and immutable history facts.

Every action kind declares, per terminal outcome, the exact fields its
result carries. `decode_result` turns a recorded completion into a typed
result or a problem; nothing outside a declaration is ever read. A
`HistoryFacts` is built once from a structurally valid history and is
the only history view properties receive.
"""

from __future__ import annotations

import errno
import functools
import hashlib
import re
from collections.abc import Mapping
from dataclasses import dataclass, field, replace
from enum import StrEnum
from types import MappingProxyType
from typing import Any, TYPE_CHECKING

from .differential import read_side, REFERENCE_RESULT
from .effects import Window
from .history import (
    ACTION_EVENTS,
    CLEANUP,
    Event,
    FAIL,
    History,
    history_problems,
    INFO,
    OK,
)
from .jsonio import MAX_ID_BYTES, require as _require

if TYPE_CHECKING:
    from .spec_types import Action, RunSpec
    from .target import Target


ERRNO_FIELD = "errno"
# The `errnos` key of a failure that carries no errno (a process exit
# status, an RPC error with its own code), as `checker.errno_name` names it.
NO_ERRNO = "none"
ERROR_TYPE_FIELD = "error_type"
_ERROR_TYPE = re.compile(r"[A-Z][A-Z0-9_]*")
INFO_REASON_FIELD = "reason"
ENVELOPE_KEYS = frozenset({"action", "actor"})
READ_PREFIX_BYTES = 128

_FIELD_NAME = re.compile(r"[a-z][a-z0-9_]*")
_HEX = re.compile(r"(?:[0-9a-f]{2})*")
_SHA256 = re.compile(r"[0-9a-f]{64}")
_ERRNO_NAMES = frozenset(errno.errorcode.values())
_MAX_DEPTH = 8
_OUTCOMES = frozenset({"ok", "fail", "info"})
SIDES = frozenset({"primary", "reference"})


class Outcome(StrEnum):
    """The terminal outcome of one invoked action."""

    OK = "ok"
    FAIL = "fail"
    INFO = "info"


class FieldKind(StrEnum):
    """The exact JSON shape of one result field.

    `DIGEST` is the content-digest triple `{bytes_prefix, length, sha256}`,
    checked the same way when an object declares the three as flat fields;
    `JSON` is an opaque diagnostic value nobody may judge.
    """

    TEXT = "text"
    INT = "int"
    BOOL = "bool"
    HEX = "hex"
    DIGEST = "digest"
    TEXT_LIST = "text-list"
    INT_LIST = "int-list"
    OBJECT = "object"
    OBJECT_LIST = "object-list"
    JSON = "json"


_NESTED = frozenset({FieldKind.OBJECT, FieldKind.OBJECT_LIST})


class ErrnoUse(StrEnum):
    """Why a modeled input may fail with one errno.

    `STATE` errnos are legal in some states; the target turns them into
    observations. `FAULT` errnos are legal only when a nemesis caused them.
    `TIMEOUT` is ETIMEDOUT, legal only under the timeout excuse.
    """

    STATE = "state"
    FAULT = "fault"
    TIMEOUT = "timeout"


@dataclass(frozen=True)
class FieldSpec:
    """One declared result field.

    `consumed=False` marks a diagnostic field no property judges. An
    `OBJECT` or `OBJECT_LIST` field declares its members in `schema`, and
    objects nest at most `_MAX_DEPTH` levels, so every declared schema can
    decode.
    """

    name: str
    kind: FieldKind
    required: bool = True
    consumed: bool = True
    schema: tuple[FieldSpec, ...] | None = None

    def __post_init__(self) -> None:
        _require(
            type(self.name) is str
            and _FIELD_NAME.fullmatch(self.name) is not None
            and len(self.name) <= MAX_ID_BYTES,
            f"bad result field name {self.name!r:.200}",
        )
        _require(self.kind in frozenset(FieldKind), f"bad field kind {self.kind!r}")
        _require(
            type(self.required) is bool and type(self.consumed) is bool,
            f"{self.name}: required and consumed are bools",
        )
        _require(
            self.kind != FieldKind.JSON or not self.consumed,
            f"{self.name}: a json field is diagnostic and never consumed",
        )
        if self.kind in _NESTED:
            _require(
                type(self.schema) is tuple and bool(self.schema),
                f"{self.name}: an object field declares its members",
            )
            _check_fields(self.schema, f"{self.name} members")
            _require(
                self.depth <= _MAX_DEPTH,
                f"{self.name}: objects nest more than {_MAX_DEPTH} levels",
            )
        else:
            _require(self.schema is None, f"{self.name}: only objects have members")

    @property
    def depth(self) -> int:
        """Return how many object levels this field nests: 0 for a non-object."""
        if self.schema is None:
            return 0
        return 1 + max(member.depth for member in self.schema)

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this field declaration."""
        return {
            "name": self.name,
            "kind": str(self.kind),
            "required": self.required,
            "consumed": self.consumed,
            "schema": None
            if self.schema is None
            else [member.document() for member in self.schema],
        }


def _check_fields(fields: Any, what: str) -> None:
    _require(type(fields) is tuple, f"{what} must be a tuple of FieldSpec")
    for spec in fields:
        _require(
            isinstance(spec, FieldSpec), f"{what}: {spec!r:.200} is not a FieldSpec"
        )
    names = [spec.name for spec in fields]
    _require(len(set(names)) == len(names), f"{what} repeat a field name")


@dataclass(frozen=True)
class ResultSchema:
    """The exact fields of each terminal outcome, and the errno vocabulary.

    Every `info` result may also carry the scheduler's diagnostic
    `reason`, so `info` fields never declare it, and every `info` field is
    optional: a scheduler-stopped action carries only that reason.
    `errnos` maps errno names
    to their use; a schema with errnos declares an `errno` int on `fail`.
    The key `NO_ERRNO` declares failures without an errno: it is STATE
    (the target decides it with an observation) or FAULT, and needs no
    errno field. `error_types` classifies errno-less failures that name
    a typed error in the `error_type` text field (which the schema then
    declares on `fail`): each declared type is STATE or FAULT, and an
    undeclared type is judged like an undeclared errno.
    """

    ok: tuple[FieldSpec, ...] = ()
    fail: tuple[FieldSpec, ...] = ()
    info: tuple[FieldSpec, ...] = ()
    errnos: Mapping[str, ErrnoUse] = field(default_factory=dict)
    error_types: Mapping[str, ErrnoUse] = field(default_factory=dict)

    def __post_init__(self) -> None:
        for outcome in Outcome:
            _check_fields(self.fields(outcome), f"{outcome} fields")
            _require(
                all(
                    spec == REFERENCE_SIDE and outcome != INFO
                    for spec in self.fields(outcome)
                    if spec.name == REFERENCE_RESULT
                ),
                f"{REFERENCE_RESULT} is declared only by `paired` on ok and fail",
            )
        _require(
            all(spec.name != INFO_REASON_FIELD for spec in self.info),
            "info fields never declare the scheduler's reason",
        )
        _require(
            not any(spec.required for spec in self.info),
            "info fields are optional: a scheduler stop carries only its reason",
        )
        errnos = {}
        for name, use in dict(self.errnos).items():
            _require(
                name in _ERRNO_NAMES or name == NO_ERRNO,
                f"unknown errno {name!r:.200}",
            )
            _require(use in frozenset(ErrnoUse), f"{name}: bad errno use {use!r:.200}")
            errnos[name] = ErrnoUse(use)
        object.__setattr__(self, "errnos", MappingProxyType(errnos))
        for name, use in self.errnos.items():
            _require(
                (use == ErrnoUse.TIMEOUT) == (name == "ETIMEDOUT"),
                f"{name}: only ETIMEDOUT is a timeout errno, and it always is",
            )
        errno_fields = [spec for spec in self.fail if spec.name == ERRNO_FIELD]
        _require(
            all(spec.kind == FieldKind.INT for spec in errno_fields),
            "a fail errno field is an int",
        )
        _require(
            set(self.errnos) <= {NO_ERRNO} or bool(errno_fields),
            "a schema with errnos declares a fail errno field",
        )
        types = {}
        for name, use in dict(self.error_types).items():
            _require(
                type(name) is str and _ERROR_TYPE.fullmatch(name) is not None,
                f"bad error type {name!r:.200}",
            )
            _require(
                use in (ErrnoUse.STATE, ErrnoUse.FAULT),
                f"{name}: an error type is state or fault, not {use!r:.200}",
            )
            types[name] = ErrnoUse(use)
        object.__setattr__(self, "error_types", MappingProxyType(types))
        _require(
            not types
            or any(
                spec.name == ERROR_TYPE_FIELD and spec.kind == FieldKind.TEXT
                for spec in self.fail
            ),
            "a schema with error types declares a fail error_type text field",
        )

    def fields(self, outcome: Outcome | str) -> tuple[FieldSpec, ...]:
        """Return the declared fields of one outcome."""
        return {OK: self.ok, FAIL: self.fail, INFO: self.info}[outcome]

    def failure_fields(self) -> frozenset[str]:
        """Return the `fail` fields the core reads to classify a failure."""
        if self.error_types:
            return frozenset({ERRNO_FIELD, ERROR_TYPE_FIELD})
        return frozenset({ERRNO_FIELD})

    def document(self) -> dict[str, Any]:
        """Return the canonical JSON document of this schema.

        `error_types` appears only when the schema declares some.
        """
        document: dict[str, Any] = {
            "ok": [spec.document() for spec in self.ok],
            "fail": [spec.document() for spec in self.fail],
            "info": [spec.document() for spec in self.info],
            "errnos": {name: str(use) for name, use in sorted(self.errnos.items())},
        }
        if self.error_types:
            document["error_types"] = {
                name: str(use) for name, use in sorted(self.error_types.items())
            }
        return document


EMPTY_RESULT = ResultSchema()
# A differential terminal's reference side: declared, never judged as a
# field, and decoded by `HistoryFacts.build` into `ActionFacts.reference`.
REFERENCE_SIDE = FieldSpec(REFERENCE_RESULT, FieldKind.JSON, False, False)


def paired(schema: ResultSchema) -> ResultSchema:
    """Return `schema` whose `ok` and `fail` results may carry a reference side."""
    return replace(
        schema, ok=(*schema.ok, REFERENCE_SIDE), fail=(*schema.fail, REFERENCE_SIDE)
    )


_INFO_REASON = FieldSpec(INFO_REASON_FIELD, FieldKind.TEXT, False, False)


@dataclass(frozen=True)
class ContentDigest:
    """Content facts: the hex prefix, the exact length, and the sha256."""

    bytes_prefix: str
    length: int
    sha256: str

    def key(self) -> bytes:
        """Return what two contents compare by: bytes, or sha256 past the prefix."""
        if self.length > READ_PREFIX_BYTES:
            return bytes.fromhex(self.sha256)
        return bytes.fromhex(self.bytes_prefix)


@dataclass(frozen=True)
class TypedResult:
    """One decoded result: its outcome and exactly its declared fields."""

    outcome: Outcome
    fields: Mapping[str, Any]

    def get(self, name: str) -> Any:
        """Return one decoded field, or None when an optional field is absent."""
        return self.fields.get(name)

    @property
    def errno(self) -> int | None:
        """Return the failure errno, when the result carries one."""
        value = self.fields.get(ERRNO_FIELD)
        return value if type(value) is int else None


@dataclass(frozen=True)
class ResultProblem:
    """Why one recorded result does not match its declaration: a recorder failure."""

    outcome: str
    problems: tuple[str, ...]


def _label(value: Any) -> str:
    if type(value) is str and len(value) <= 64:
        return repr(value)
    if type(value) in (bool, float) or value is None:
        return repr(value)
    if type(value) is int:
        return "<int>" if value.bit_length() > 64 else str(value)
    return f"<{type(value).__name__}>"


def _is_int(value: Any) -> bool:
    return type(value) is int


_DIGEST_KEYS = frozenset({"bytes_prefix", "length", "sha256"})


def _decode_digest(value: Any, path: str, problems: list[str]) -> Any:
    if type(value) is not dict or set(value) != _DIGEST_KEYS:
        problems.append(f"{path}: digest must have exactly {sorted(_DIGEST_KEYS)}")
        return None
    prefix, length, sha = value["bytes_prefix"], value["length"], value["sha256"]
    if not _is_int(length) or length < 0:
        problems.append(f"{path}: digest length {_label(length)} is not a count")
        return None
    if type(prefix) is not str or _HEX.fullmatch(prefix) is None:
        problems.append(f"{path}: digest prefix is not lowercase hex")
        return None
    if len(prefix) != 2 * min(length, READ_PREFIX_BYTES):
        problems.append(f"{path}: digest prefix does not cover the first bytes")
        return None
    if type(sha) is not str or _SHA256.fullmatch(sha) is None:
        problems.append(f"{path}: digest sha256 is not 64 lowercase hex digits")
        return None
    if (
        length <= READ_PREFIX_BYTES
        and hashlib.sha256(bytes.fromhex(prefix)).hexdigest() != sha
    ):
        problems.append(f"{path}: digest sha256 does not match its whole bytes")
        return None
    return ContentDigest(prefix, length, sha)


def _decode_list(item_ok: Any, value: Any, path: str, problems: list[str]) -> Any:
    if type(value) is not list or not all(item_ok(item) for item in value):
        problems.append(f"{path}: {_label(value)} is not the declared list")
        return None
    return tuple(value)


_SCALARS: Mapping[FieldKind, Any] = {
    FieldKind.TEXT: lambda value: type(value) is str,
    FieldKind.INT: _is_int,
    FieldKind.BOOL: lambda value: type(value) is bool,
    FieldKind.HEX: lambda value: type(value) is str
    and _HEX.fullmatch(value) is not None,
}


def _decode_value(
    spec: FieldSpec, value: Any, path: str, problems: list[str], depth: int
) -> Any:
    """Decode one field value of a declared kind; record a problem on mismatch."""
    kind = spec.kind
    if kind in _SCALARS:
        if not _SCALARS[kind](value):
            problems.append(f"{path}: {_label(value)} is not {kind}")
        return value
    if kind == FieldKind.DIGEST:
        return _decode_digest(value, path, problems)
    if kind == FieldKind.TEXT_LIST:
        return _decode_list(lambda item: type(item) is str, value, path, problems)
    if kind == FieldKind.INT_LIST:
        return _decode_list(_is_int, value, path, problems)
    if kind == FieldKind.JSON:
        return freeze(value)
    assert spec.schema is not None
    if kind == FieldKind.OBJECT:
        return _decode_object(spec.schema, value, path, problems, depth + 1)
    if type(value) is not list:
        problems.append(f"{path}: {_label(value)} is not a list of objects")
        return None
    return tuple(
        _decode_object(spec.schema, item, f"{path}[{index}]", problems, depth + 1)
        for index, item in enumerate(value)
    )


def _decode_object(
    fields: tuple[FieldSpec, ...],
    raw: Any,
    path: str,
    problems: list[str],
    depth: int = 0,
) -> Mapping[str, Any]:
    """Decode an object with exactly the declared keys: required ⊆ keys ⊆ declared."""
    if depth > _MAX_DEPTH or type(raw) is not dict:
        problems.append(f"{path or 'result'}: {_label(raw)} is not an object")
        return MappingProxyType({})
    declared = {spec.name: spec for spec in fields}
    unknown = sorted(_label(key) for key in raw if key not in declared)
    if unknown:
        shown = ", ".join(unknown[:8])
        problems.append(f"{path or 'result'}: undeclared fields [{shown}]")
    decoded: dict[str, Any] = {}
    for name, spec in declared.items():
        where = f"{path}.{name}" if path else name
        if name not in raw:
            if spec.required:
                problems.append(f"{where}: required field is missing")
            continue
        decoded[name] = _decode_value(spec, raw[name], where, problems, depth)
    if _DIGEST_KEYS <= decoded.keys():
        _decode_digest(
            {key: raw[key] for key in _DIGEST_KEYS}, path or "result", problems
        )
    return MappingProxyType(decoded)


def decode_result(
    schema: ResultSchema, outcome: str, raw: Mapping[str, Any]
) -> TypedResult | ResultProblem:
    """Decode one recorded result against its schema; never raises for any outcome or raw.

    `raw` is the completion's data without the scheduler's `action` and
    `actor`. The key set must satisfy required ⊆ keys ⊆ declared, every
    value must have exactly its declared kind (a bool is not an int), and
    nested objects decode recursively. Anything else, including an outcome
    that is not one of the three or a `raw` that is not a dict, is a
    problem. Decoded JSON values are read-only copies.
    """
    if type(outcome) is not str or outcome not in _OUTCOMES:
        return ResultProblem(_label(outcome), (f"{_label(outcome)} is not an outcome",))
    if type(raw) is not dict:
        return ResultProblem(outcome, (f"result {_label(raw)} is not an object",))
    fields = schema.fields(outcome)
    if outcome == INFO:
        fields = (*fields, _INFO_REASON)
    problems: list[str] = []
    decoded = _decode_object(fields, raw, "", problems)
    if problems:
        return ResultProblem(outcome, tuple(problems))
    return TypedResult(Outcome(outcome), decoded)


@dataclass(frozen=True)
class ActionFacts:
    """Everything recorded about one action, decoded against its schema.

    `result` is None when the action has no completion or its result did
    not decode; `problems` then says why. `side` is the side the action
    ran on, and `reference` is the paired reference side's facts of the
    same action ID: when the kind's schema is `paired`, `HistoryFacts.build`
    decodes the `reference_result` a differential terminal event carries
    against the same schema, with the side's own start and completion
    times. An undeclared `reference_result` is an undeclared primary field.
    A malformed or out-of-span reference side keeps its problems on the
    reference facts, never the primary's. Presence follows the target's
    `expects_reference`: an `ok`/`fail` action that owes a side and
    carries none, or one carrying a side it does not owe, gets reference
    facts holding only `MISSING_REFERENCE` or `UNEXPECTED_REFERENCE`;
    otherwise `reference` is None when the terminal carries no side.
    """

    id: str
    kind: str
    actor: str
    payload: Mapping[str, Any]
    side: str = "primary"
    invoked: float | None = None
    completed: float | None = None
    outcome: Outcome | None = None
    result: TypedResult | None = None
    problems: tuple[str, ...] = ()
    reference: ActionFacts | None = None

    def __post_init__(self) -> None:
        _require(self.side in SIDES, f"{self.id}: bad side {self.side!r:.200}")
        reference = self.reference
        _require(
            reference is None
            or (
                isinstance(reference, ActionFacts)
                and reference.side == "reference"
                and reference.id == self.id
                and reference.reference is None
                and self.side == "primary"
            ),
            f"{self.id}: a reference pairs a primary action with its reference side",
        )

    @property
    def interval(self) -> tuple[float, float] | None:
        """Return the closed invocation-to-completion interval, when complete."""
        if self.invoked is None or self.completed is None:
            return None
        return (self.invoked, self.completed)

    @property
    def window(self) -> Window:
        """Return the invocation-to-completion Window of a completed action."""
        assert self.invoked is not None and self.completed is not None
        return Window(self.invoked, self.completed)


def freeze(value: Any) -> Any:
    """Return a read-only copy of decoded JSON: mappings become proxies, lists tuples."""
    if type(value) is dict:
        return MappingProxyType({key: freeze(item) for key, item in value.items()})
    if type(value) in (list, tuple):
        return tuple(freeze(item) for item in value)
    return value


@dataclass(frozen=True)
class RunRecord:
    """One run-level history record, such as a daemon exit or cleanup, read-only."""

    type: str
    time: float
    data: Mapping[str, Any]


def _actor_of(spec: RunSpec) -> dict[str, str]:
    return {action_id: actor.id for actor in spec.actors for action_id in actor.actions}


def _decode(
    schema: ResultSchema | None, kind: str, outcome: str, raw: Mapping[str, Any]
) -> TypedResult | ResultProblem:
    if schema is None:
        return ResultProblem(outcome, (f"unknown action kind {kind!r:.200}",))
    return decode_result(schema, outcome, raw)


def _reference_facts(
    base: ActionFacts,
    schema: ResultSchema | None,
    raw: Any,
    span: tuple[float | None, float],
) -> ActionFacts:
    """Decode one terminal event's reference side into reference-side facts.

    `base` names the action on the reference side. The side must be well
    formed and run inside its primary's span; otherwise the reference
    facts carry the problem and no result.
    """
    try:
        side = read_side(raw)
    except ValueError as error:
        return replace(base, problems=(f"reference result: {error}"[:1024],))
    timed = replace(
        base,
        invoked=side.started,
        completed=side.completed,
        outcome=Outcome(side.outcome),
    )
    invoked, completed = span
    if invoked is None or not invoked <= side.started <= side.completed <= completed:
        return replace(
            timed,
            problems=(
                f"reference side [{side.started}, {side.completed}] is outside "
                f"the primary span [{invoked}, {completed}]",
            ),
        )
    decoded = _decode(schema, base.kind, side.outcome, dict(side.facts))
    if isinstance(decoded, ResultProblem):
        return replace(timed, problems=decoded.problems)
    return replace(timed, result=decoded)


MISSING_REFERENCE = "missing reference side"
UNEXPECTED_REFERENCE = "reference side on an action that owes none"


def owes_reference(target: Target, spec: RunSpec, action: Action) -> bool:
    """Return whether the target says the action's ok/fail terminal carries a side."""
    predicate = target.expects_reference
    return predicate is not None and predicate(spec, action)


def _presence_facts(
    base: ActionFacts, outcome: str, carried: bool, owed: bool
) -> ActionFacts | None:
    """Return reference facts for a side that is missing or unexpected, else None.

    A completed `ok`/`fail` action that owes a side and carries none, and
    an action carrying a side it does not owe, each get reference facts
    whose only content is the problem; `core.result-schema` attributes
    the first to the reference and the second to the harness.
    """
    if carried and not owed:
        return replace(base, problems=(UNEXPECTED_REFERENCE,))
    if owed and not carried and outcome in (OK, FAIL):
        return replace(base, problems=(MISSING_REFERENCE,))
    return None


def _action_facts(
    action: Action,
    actor: str,
    schema: ResultSchema | None,
    history: History,
    owed: bool,
) -> ActionFacts:
    invocation = history.invoke(action.id)
    completion = history.completion(action.id)
    common = {
        "id": action.id,
        "kind": action.kind,
        "actor": actor,
        "payload": freeze(dict(action.payload)),
        "side": "primary",
        "invoked": None if invocation is None else invocation.time,
    }
    if completion is None:
        return ActionFacts(**common)
    raw = {k: v for k, v in completion.data.items() if k not in ENVELOPE_KEYS}
    declared = schema is not None and REFERENCE_SIDE in schema.fields(completion.type)
    carried = declared and REFERENCE_RESULT in raw
    side = raw.pop(REFERENCE_RESULT) if carried else None
    base = ActionFacts(
        action.id, action.kind, actor, common["payload"], side="reference"
    )
    reference = _presence_facts(base, completion.type, carried, owed)
    if reference is None and carried:
        reference = _reference_facts(
            base,
            schema,
            side,
            (None if invocation is None else invocation.time, completion.time),
        )
    decoded = _decode(schema, action.kind, completion.type, raw)
    outcome = Outcome(completion.type)
    if isinstance(decoded, ResultProblem):
        return ActionFacts(
            **common,
            completed=completion.time,
            outcome=outcome,
            problems=decoded.problems,
            reference=reference,
        )
    return ActionFacts(
        **common,
        completed=completion.time,
        outcome=outcome,
        result=decoded,
        reference=reference,
    )


def _schema(target: Target, action: Action) -> ResultSchema | None:
    definition = target.actions.get(action.kind)
    return None if definition is None else definition.result


class HistoryFacts:
    """Immutable decoded facts about one structurally valid history.

    Per-action facts keep spec order. Run-level records (daemon exits,
    cleanup) stay read-only events. Indexes are built on first use and
    cached. Temporal nemesis windows come from the same validated events.
    """

    def __init__(
        self,
        spec: RunSpec,
        actions: Mapping[str, ActionFacts],
        run_events: tuple[RunRecord, ...],
        history: History,
    ) -> None:
        self._spec = spec
        self._actions = MappingProxyType(dict(actions))
        self._run_events = run_events
        self._history = history

    @classmethod
    def build(
        cls,
        spec: RunSpec,
        events: tuple[Event, ...] | list[Event],
        target: Target,
        *,
        execution_status: str | None = None,
    ) -> HistoryFacts:
        """Decode every action of a history whose `history_problems` are empty.

        Raises ValueError, naming the first problem, for a history that
        breaks its spec's grammar under `execution_status`.
        """
        problems = history_problems(spec, events, execution_status=execution_status)
        _require(
            not problems,
            f"history facts need a valid history: {problems[0] if problems else ''}"
            + (f" (+{len(problems) - 1} more)" if len(problems) > 1 else ""),
        )
        history = History(spec, events, target.actions)
        actors = _actor_of(spec)
        actions = {
            action.id: _action_facts(
                action,
                actors.get(action.id, ""),
                _schema(target, action),
                history,
                owes_reference(target, spec, action),
            )
            for action in spec.actions
        }
        run_events = tuple(
            RunRecord(event.type, event.time, freeze(event.data))
            for event in events
            if event.type not in ACTION_EVENTS or "action" not in event.data
        )
        return cls(spec, actions, run_events, history)

    @property
    def spec(self) -> RunSpec:
        """Return the RunSpec these facts describe."""
        return self._spec

    @property
    def actions(self) -> Mapping[str, ActionFacts]:
        """Return every action's facts by ID, in spec order."""
        return self._actions

    @property
    def run_events(self) -> tuple[RunRecord, ...]:
        """Return the run-level records: events that belong to no action's grammar."""
        return self._run_events

    def action(self, action_id: str) -> ActionFacts:
        """Return one action's facts."""
        return self._actions[action_id]

    @functools.cached_property
    def _by_kind(self) -> Mapping[str, tuple[ActionFacts, ...]]:
        index: dict[str, list[ActionFacts]] = {}
        for facts in self._actions.values():
            index.setdefault(facts.kind, []).append(facts)
        return MappingProxyType({kind: tuple(items) for kind, items in index.items()})

    def actions_of(self, kind: str) -> tuple[ActionFacts, ...]:
        """Return the facts of every action of one kind, in spec order."""
        return self._by_kind.get(kind, ())

    @functools.cached_property
    def cleanup_time(self) -> float:
        """Return when the runner's final cleanup record was written, or infinity.

        The final record carries the environment close result (`close_error`)
        or the scheduler's status; an earlier cleanup record, such as the
        list of calls still active at teardown, is written before the
        environment closes and does not end the run.
        """
        return min(
            (
                event.time
                for event in self._run_events
                if event.type == CLEANUP
                and ("close_error" in event.data or "scheduler" in event.data)
            ),
            default=float("inf"),
        )

    @functools.cached_property
    def _fault_windows(self) -> tuple[tuple[Action, tuple[float, float]], ...]:
        return tuple(self._history.fault_windows())

    def fault_windows(self) -> list[tuple[Action, tuple[float, float]]]:
        """Return each fault install that completed ok with its lifetime window."""
        return list(self._fault_windows)
