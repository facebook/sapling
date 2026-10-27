# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Offline in-memory target for exercising the complete run pipeline.

Its one property judges reads on the shared engine: the contracts hook turns
each `ok` write into an acked group and each `info` write into an
indeterminate one (resolved at the end of the run), the pinned facts give
every register its `init` value, and each `ok` read is an observation of
its register expecting exactly the bytes it returned. Torn and fabricated
bytes are decided directly.
"""

from __future__ import annotations

import hashlib
import math
import random
from collections.abc import Mapping
from dataclasses import dataclass, field
from typing import Any

from eden.integration.correctness.exploratory.core.actions import (
    compose_action_defs,
    CORE_ACTION_DEFS,
    write_sampler,
)
from eden.integration.correctness.exploratory.core.effects import (
    Cell,
    ContractSink,
    Effect,
    EffectGroup,
    EffectKind,
    eq,
    Expect,
    Footprint,
    Observation,
    PinnedFacts,
    PRIMARY,
    Status,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import (
    ActionFacts,
    EMPTY_RESULT,
    FieldKind,
    FieldSpec,
    HistoryFacts,
    INFO,
    OK,
    ResultSchema,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    path_subject,
    PropertyDef,
    RuleDef,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.core.scheduler import BackendResult
from eden.integration.correctness.exploratory.core.target import (
    ActionDef,
    Binding,
    Environment,
    Grouping,
    ProcessRegistry,
    Role,
    Target,
    TargetSources,
)

WRITE = "write"
READ = "read"
GETATTR = "getattr"
_VALUE_SIZE = 64

BYTES = "bytes"
CHANNEL = "fake"
TORN_WRITE = "torn-write"
SEMANTICS_PROFILE = "fake-memory-v1"


def encode_value(seed: int, writer: str, value_id: str) -> bytes:
    """Return the 64 ASCII bytes the fake backend stores for one register value."""
    text = f"REG1 seed={seed:08d} writer={writer} action={value_id}"
    encoded = text.encode("ascii")
    if len(encoded) > _VALUE_SIZE:
        raise ValueError(f"encoded register value too long: {text!r}")
    return encoded + b"." * (_VALUE_SIZE - len(encoded))


def sample_environment(_rng: random.Random) -> Mapping[str, Any]:
    """Sample the fake target's empty environment."""
    return {}


def _sample_path(rng: random.Random, context: Mapping[str, Any]) -> str:
    return "register-0" if context.get("contended") else f"register-{rng.randrange(4)}"


def _sample_readlike(
    rng: random.Random, context: Mapping[str, Any]
) -> Mapping[str, Any] | None:
    if context.get("contended") and not context.get("forced_kind"):
        return None
    return {"path": _sample_path(rng, context)}


def _require_payload(*names: str):
    def validate(_spec: RunSpec, action: Action) -> None:
        missing = [name for name in names if not action.payload.get(name)]
        if missing:
            raise SpecError(f"action {action.id!r} needs {' and '.join(missing)}")

    return validate


def _validate_write(spec: RunSpec, action: Action) -> None:
    """Require a path and a value whose register encoding fits `_VALUE_SIZE`."""
    _require_payload("path", "value_id")(spec, action)
    try:
        encode_value(spec.seed, spec.owner(action.id), action.payload["value_id"])
    except ValueError as error:
        raise SpecError(f"action {action.id!r}: {error}") from error


def _reads_path(_spec: RunSpec, action: Action) -> Footprint:
    return Footprint(
        reads=frozenset({action.payload["path"]}), channels=frozenset({CHANNEL})
    )


def _writes_path(_spec: RunSpec, action: Action) -> Footprint:
    return Footprint(
        writes=frozenset({action.payload["path"]}), channels=frozenset({CHANNEL})
    )


BYTES_FIELD = "bytes"
READ_RESULT = ResultSchema(ok=(FieldSpec(BYTES_FIELD, FieldKind.HEX),))
GETATTR_RESULT = ResultSchema(ok=(FieldSpec("exists", FieldKind.BOOL, consumed=False),))

FAKE_ACTION_DEFS = {
    WRITE: ActionDef(
        WRITE,
        "input",
        frozenset({"path", "value_id"}),
        frozenset({"path", "value_id"}),
        {"path": str, "value_id": str},
        _validate_write,
        True,
        False,
        write_sampler(_sample_path),
        result=EMPTY_RESULT,
        footprint=_writes_path,
        roles=frozenset({Role.PRODUCE, Role.CAN_SUPERSEDE}),
        binding=Binding.PATH,
        grouping=Grouping.SINGLE,
    ),
    READ: ActionDef(
        READ,
        "input",
        frozenset({"path"}),
        frozenset({"path"}),
        {"path": str},
        _require_payload("path"),
        True,
        False,
        _sample_readlike,
        result=READ_RESULT,
        footprint=_reads_path,
        roles=frozenset({Role.OBSERVE}),
        binding=Binding.PATH,
        grouping=Grouping.NONE,
    ),
    GETATTR: ActionDef(
        GETATTR,
        "input",
        frozenset({"path"}),
        frozenset({"path"}),
        {"path": str},
        _require_payload("path"),
        False,
        False,
        _sample_readlike,
        result=GETATTR_RESULT,
        footprint=_reads_path,
        roles=frozenset({Role.OBSERVE}),
        binding=Binding.PATH,
        grouping=Grouping.NONE,
    ),
}

ACTIONS = compose_action_defs(CORE_ACTION_DEFS, FAKE_ACTION_DEFS)


@dataclass
class FakeProcesses(ProcessRegistry):
    """Record fake process identifiers."""

    pids: tuple[int, ...] = (1,)

    def __post_init__(self) -> None:
        super().__init__()


@dataclass
class FakeEnvironment:
    """In-memory files and deterministic fake process lifecycle."""

    files: dict[str, bytes] = field(default_factory=dict)
    processes: FakeProcesses = field(default_factory=FakeProcesses)

    def setup(self) -> None:
        """Prepare the in-memory target."""

    def close(self) -> str | None:
        """Close the in-memory target without cleanup errors."""
        return None


class FakeBackend:
    """Execute fake actions against one environment's byte map."""

    def __init__(
        self, environment: FakeEnvironment, spec: RunSpec, red_control: str | None
    ) -> None:
        self._environment = environment
        self._spec = spec
        self._red_control = red_control

    def __call__(self, action: Action) -> BackendResult:
        """Execute one in-memory action."""
        path = action.payload["path"]
        if action.kind == WRITE:
            value_id = action.payload["value_id"]
            value = encode_value(self._spec.seed, self._spec.owner(action.id), value_id)
            if self._red_control == TORN_WRITE:
                # Half the new image over the old one, as a short write would.
                old = self._environment.files.get(
                    path, encode_value(self._spec.seed, "init", path)
                )
                value = value[: _VALUE_SIZE // 2] + old[_VALUE_SIZE // 2 :]
            self._environment.files[path] = value
            return BackendResult(OK)
        if action.kind == READ:
            value = self._environment.files.get(
                path, encode_value(self._spec.seed, "init", path)
            )
            return BackendResult(OK, {BYTES_FIELD: value.hex()})
        return BackendResult(OK, {"exists": path in self._environment.files})

    def unwind(self, sink: Any, clock: Any) -> dict[str, Any]:
        """Return empty cleanup facts because fake actions hold no resources."""
        del sink, clock
        return {}


STALE_READ = f"{BYTES}.stale-read"
TORN_READ = f"{BYTES}.torn-read"
FABRICATED_VALUE = f"{BYTES}.fabricated-value"
REGISTER_SPACE = "path"
FILE = "file"


def bytes_value(data: bytes) -> Value:
    """Return the register value holding `data`: its sha256 content token."""
    return Value(FILE, (hashlib.sha256(data).hexdigest(),))


def register_cell(path: str) -> Cell:
    """Return the cell of one fake register."""
    return Cell(PRIMARY, REGISTER_SPACE, path)


class FakePinned:
    """The fake fixture: every register starts with its `init` value."""

    def __init__(self, spec: RunSpec) -> None:
        self._seed = spec.seed

    def initial(self, cell: Cell) -> Value:
        """Return a register's `init` value."""
        return bytes_value(encode_value(self._seed, "init", cell.key))


def pinned_facts(spec: RunSpec) -> FakePinned:
    """Return the fake target's pinned facts."""
    return FakePinned(spec)


def contracts(
    spec: RunSpec, facts: HistoryFacts, _pinned: PinnedFacts | None, sink: ContractSink
) -> None:
    """Record one group per write that may have applied.

    An `ok` write applied inside its interval; an `info` write, or one
    invoked with no terminal event (a prefix history), may have applied at
    most once from its invocation to the end of the run; a failed write
    applied nothing.
    """
    for recorded in facts.actions_of(WRITE):
        if recorded.invoked is None or recorded.outcome not in (OK, INFO, None):
            continue
        acked = recorded.outcome == OK and recorded.completed is not None
        end = recorded.completed if acked else None
        data = encode_value(spec.seed, recorded.actor, recorded.payload["value_id"])
        sink.group(
            EffectGroup(
                id=f"g:{recorded.id}",
                action=recorded.id,
                status=Status.ACKED if acked else Status.INDETERMINATE,
                window=Window(recorded.invoked, math.inf if end is None else end),
                effects=(
                    Effect(
                        register_cell(recorded.payload["path"]),
                        EffectKind.PRODUCE,
                        bytes_value(data),
                        None,
                        recorded.id,
                    ),
                ),
            )
        )


_CHANNEL_WITNESS = (Witness("channel", CHANNEL),)


def _candidates(spec: RunSpec) -> dict[str, dict[str, bytes]]:
    """Return every register's candidate values: its `init` value and its writes."""
    found: dict[str, dict[str, bytes]] = {}
    for action in spec.actions:
        if action.kind not in (WRITE, READ):
            continue
        path = action.payload["path"]
        values = found.setdefault(path, {})
        init = encode_value(spec.seed, "init", path)
        values[hashlib.sha256(init).hexdigest()] = init
        if action.kind == WRITE:
            data = encode_value(
                spec.seed, spec.owner(action.id), action.payload["value_id"]
            )
            values[hashlib.sha256(data).hexdigest()] = data
    return found


def _splices(observed: bytes, candidates: Mapping[str, bytes]) -> bool:
    """Return whether two candidates cover `observed`, each byte from one of them.

    O(k² · 64) for the k same-length candidates of one path; k is at most
    the path's writes plus its initial value.
    """
    same = sorted(data for data in candidates.values() if len(data) == len(observed))
    for index, first in enumerate(same):
        differs = [i for i, byte in enumerate(observed) if first[i] != byte]
        if any(
            all(second[i] == observed[i] for i in differs)
            for second in same[index + 1 :]
        ):
            return True
    return False


def _judge(
    recorded: ActionFacts,
    observed: bytes,
    candidates: Mapping[str, Mapping[str, bytes]],
    sink: CollectorSink,
) -> bool:
    """Decide the read's direct rules; return whether the engine must judge it."""
    path = recorded.payload["path"]
    subject = path_subject(path)
    refs = {
        rule: sink.obligation(
            rule, recorded.id, subject=subject, witnesses=_CHANNEL_WITNESS
        )
        for rule in (TORN_READ, FABRICATED_VALUE)
    }
    token = hashlib.sha256(observed).hexdigest()
    anywhere = {t for values in candidates.values() for t in values}
    if token in candidates.get(path, {}):
        verdict = None
    elif _splices(observed, candidates.get(path, {})):
        verdict = TORN_READ
    elif token not in anywhere:
        verdict = FABRICATED_VALUE
    else:
        verdict = None
    for rule, ref in refs.items():
        if rule == verdict:
            sink.violated(
                ref,
                Attribution.UNDETERMINED,
                f"bytes/{CHANNEL}: {rule.rsplit('.', 1)[1]}: {recorded.id} returned "
                f"bytes no single producer of {path} writes",
                (Witness("read-action", recorded.id),),
            )
        else:
            sink.satisfied(ref)
    return verdict is None


def collect_bytes(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every completed `ok` fake read, in spec order.

    A read whose result did not decode is the core result-schema
    property's to report, so it observes nothing here.
    """
    spec, facts = check_input.spec, check_input.facts
    candidates = _candidates(spec)
    for recorded in facts.actions_of(READ):
        interval = recorded.interval
        if interval is None or recorded.outcome != OK or recorded.result is None:
            continue
        observed = bytes.fromhex(recorded.result.get(BYTES_FIELD))
        if not _judge(recorded, observed, candidates, sink):
            continue
        path = recorded.payload["path"]
        stale = sink.obligation(
            STALE_READ,
            recorded.id,
            subject=path_subject(path),
            witnesses=_CHANNEL_WITNESS,
        )
        sink.observe(
            Observation(
                id=f"o:{recorded.id}",
                action=recorded.id,
                cells=(register_cell(path),),
                expect=Expect(((eq(bytes_value(observed)),),)),
                window=Window(*interval),
                channel=CHANNEL,
                obligations=(stale,),
            )
        )


def _byte_rule(rule_id: str, statement: str, controls: frozenset[str]) -> RuleDef:
    return RuleDef(
        id=rule_id,
        statement=statement,
        subject_kind=SubjectKind.PATH,
        identity_roles=("channel",),
        red_controls=controls,
    )


BYTES_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        _byte_rule(
            STALE_READ,
            "A read's bytes are the register's visible value at a point inside "
            "the read's window.",
            frozenset(),
        ),
        _byte_rule(
            TORN_READ,
            "A read returns one whole value that the initial state or a single "
            "write produced, never a splice of two values.",
            frozenset({TORN_WRITE}),
        ),
        _byte_rule(
            FABRICATED_VALUE,
            "A successful read returns only bytes that the initial state or "
            "some write could produce.",
            frozenset(),
        ),
    )
}

BYTES_PROPERTY = PropertyDef(
    id=BYTES,
    collect=collect_bytes,
    rules=BYTES_RULES,
    consumes=frozenset({(READ, BYTES_FIELD)}),
)


def _never(_spec: RunSpec, _facts: HistoryFacts, _action: Action) -> bool:
    """The fake target has no fault sites and no timeout-capable channel."""
    return False


def _make_environment(_spec: RunSpec, _sink: Any) -> FakeEnvironment:
    return FakeEnvironment()


def _make_backend(
    environment: Environment, spec: RunSpec, red_control: str | None
) -> FakeBackend:
    if not isinstance(environment, FakeEnvironment):
        raise TypeError("fake target needs FakeEnvironment")
    return FakeBackend(environment, spec, red_control)


FAKE = Target(
    name="fake",
    actions=ACTIONS,
    sites={},
    properties=(BYTES_PROPERTY,),
    install_witnessed=_never,
    environment_fields={},
    make_environment=_make_environment,
    make_backend=_make_backend,
    red_controls=frozenset({TORN_WRITE}),
    semantics_profile=SEMANTICS_PROFILE,
    serving_paths={CHANNEL: "memory"},
    sources=TargetSources(implementation=__name__),
    contracts=contracts,
    pinned=pinned_facts,
)
