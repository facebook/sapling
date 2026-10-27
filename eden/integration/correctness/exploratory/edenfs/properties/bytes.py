# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `bytes` property: what a successful read of a path may return.

Each `ok` read (FUSE or Thrift) becomes one observation of its path cell
expecting exactly the bytes it returned; the consistency engine decides
whether some explanation of the run's contract effects (initial state and
dirt, writes, transfers, checkouts, indeterminate outcomes, repeated
producers) shows those bytes at a point inside the read's window. Bytes
that are no candidate value anywhere in the run are decided directly,
because no explanation can produce them (`classify`):

- `bytes.torn-read`: each byte agrees with one of two candidate values
  of the path at its offset (a splice), or they are exactly
  `REGISTER_SIZE` long, the width of every register image (any 64
  non-producible bytes, e.g. a partial write's zero fill);
- `bytes.fabricated-value`: any other such bytes.

A path's candidate values are its initial value, its committed value at
every fixture revision, every write value aimed at it, and, through
renames and `sl mv`, the candidates of every path that can be moved onto
it. Candidates anywhere add every write value of the run and the initial
and committed values of every path the contracts treats as relevant: another
path's value read here is the engine's question (`bytes.stale-read`), never
"torn". A read either direct rule decides has no `bytes.stale-read`
obligation.
"""

from __future__ import annotations

from collections.abc import Iterator, Mapping
from dataclasses import dataclass
from functools import cached_property
from typing import Any

from eden.integration.correctness.exploratory.core.effects import (
    eq,
    Expect,
    normalize_path,
    ObligationRef,
    Observation,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import (
    ActionFacts,
    READ_PREFIX_BYTES,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    CoverageObligation,
    path_subject,
    PropertyDef,
    RuleDef,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.edenfs.actions import repository
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    relevant_paths,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    EdenPinned,
    path_cell,
)
from eden.integration.correctness.exploratory.edenfs.encoding import (
    content_token,
    encode_value,
    REGISTER_SIZE,
)
from eden.integration.correctness.exploratory.edenfs.fixture import FILE
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    eden_pinned,
    judged,
)

BYTES = "bytes"
STALE_READ = f"{BYTES}.stale-read"
TORN_READ = f"{BYTES}.torn-read"
FABRICATED_VALUE = f"{BYTES}.fabricated-value"
TORN_WRITE_CONTROL = "torn-write"
WRITE_OBSERVED = "bytes.write-observed"
READ = "read"
_MOVES = frozenset({"rename", "sl_mv"})

# The read result fields the collector judges: the content digest.
CONSUMES = frozenset((READ, name) for name in ("bytes_prefix", "length", "sha256"))


class Candidates:
    """The values a read of a path could have returned, by content token, lazily.

    Each candidate keeps its whole bytes so a splice can be recognized.
    """

    def __init__(self, spec: RunSpec, pinned: EdenPinned) -> None:
        self._spec = spec
        self._pinned = pinned
        self._revisions = range(repository(spec)["commit_count"])
        self._paths: dict[str, Mapping[str, bytes]] = {}

    def _fixture(self, path: str) -> dict[str, bytes]:
        fixture = self._pinned.fixture
        found: dict[str, bytes] = {}
        entries = [fixture.initial(path)]
        entries += [fixture.committed(path, revision) for revision in self._revisions]
        for entry in entries:
            if entry is not None and entry.kind == FILE:
                found[content_token(entry.data)] = entry.data
        return found

    @cached_property
    def writes(self) -> Mapping[str, Mapping[str, bytes]]:
        """Return every planned write value, by normalized path and token."""
        found: dict[str, dict[str, bytes]] = {}
        for action in self._spec.actions:
            if action.kind != "write":
                continue
            data = encode_value(
                self._spec.seed,
                self._spec.owner(action.id),
                action.payload["value_id"],
            )
            path = normalize_path(action.payload["path"])
            found.setdefault(path, {})[content_token(data)] = data
        return found

    def _sources(self, path: str) -> Iterator[str]:
        """Yield every path whose value a move can carry onto `path`, `path` first."""
        seen = {path}
        pending = [path]
        while pending:
            current = pending.pop()
            yield current
            for action in self._spec.actions:
                if action.kind not in _MOVES:
                    continue
                source = normalize_path(action.payload["source"])
                destination = normalize_path(action.payload["destination"])
                if current == destination:
                    moved = source
                elif current.startswith(destination + "/"):
                    moved = source + current[len(destination) :]
                else:
                    continue
                if moved not in seen:
                    seen.add(moved)
                    pending.append(moved)

    def of_path(self, path: str) -> Mapping[str, bytes]:
        """Return every candidate value of `path`, by token."""
        if path not in self._paths:
            found: dict[str, bytes] = {}
            for source in sorted(self._sources(path)):
                found.update(self._fixture(source))
                found.update(self.writes.get(source, {}))
            self._paths[path] = found
        return self._paths[path]

    @cached_property
    def anywhere(self) -> frozenset[str]:
        """Return every candidate token of the run: all writes, relevant paths' fixture values."""
        tokens = {token for values in self.writes.values() for token in values}
        for path in relevant_paths(self._spec, self._pinned):
            tokens.update(self._fixture(path))
        return frozenset(tokens)


def splice(observed: bytes, candidates: Mapping[str, bytes]) -> tuple[str, str] | None:
    """Return two candidate tokens whose bytes, offset by offset, cover `observed`.

    Each byte of `observed` must equal the first candidate's or the
    second's at its offset; both have the observed length. None when no
    such pair exists.
    """
    same = sorted(
        (token, data)
        for token, data in candidates.items()
        if len(data) == len(observed)
    )
    for index, (first, a) in enumerate(same):
        differs = [i for i in range(len(observed)) if a[i] != observed[i]]
        for second, b in same[index + 1 :]:
            if all(b[i] == observed[i] for i in differs):
                return first, second
    return None


CANDIDATE = "candidate"
TORN = "torn"
FABRICATED = "fabricated"


@dataclass(frozen=True)
class ValueShape:
    """What a read's bytes are: a candidate, torn (with the spliced tokens), or fabricated."""

    kind: str
    spliced: tuple[str, ...] = ()


def classify(
    result: Any, own: Mapping[str, bytes], anywhere: frozenset[str]
) -> ValueShape:
    """Classify a read's digest against the candidates of what it read.

    `own` are the candidate values of what was read and `anywhere` every
    candidate token of the run. A value some producer of the run writes
    is a candidate, whichever path it belongs to: where it may appear is
    the engine's question. Otherwise a splice of two of `own`, or
    register-width bytes, is torn, and anything else fabricated.
    """
    token, length = result.get("sha256"), result.get("length")
    if token in own or token in anywhere:
        return ValueShape(CANDIDATE)
    whole = (
        bytes.fromhex(result.get("bytes_prefix"))
        if length <= READ_PREFIX_BYTES
        else None
    )
    pair = None if whole is None else splice(whole, own)
    if pair is not None or length == REGISTER_SIZE:
        return ValueShape(TORN, pair or ())
    return ValueShape(FABRICATED)


def _read_witnesses(recorded: ActionFacts) -> tuple[Witness, ...]:
    return (Witness("read-action", recorded.id),)


def _judge_read(
    recorded: ActionFacts,
    candidates: Candidates,
    sink: CollectorSink,
) -> Observation | None:
    """Decide a read's direct rules; return its observation when the engine must judge it."""
    result = recorded.result
    assert result is not None
    path = normalize_path(recorded.payload["path"])
    channel = recorded.payload["channel"]
    subject = path_subject(path)
    identity = (Witness("channel", channel),)
    torn = sink.obligation(TORN_READ, recorded.id, subject=subject, witnesses=identity)
    fabricated = sink.obligation(
        FABRICATED_VALUE, recorded.id, subject=subject, witnesses=identity
    )
    length = result.get("length")
    shape = classify(result, candidates.of_path(path), candidates.anywhere)
    if shape.kind == TORN:
        described = (
            "that splice two candidate values and equal neither"
            if shape.spliced
            else "of register width that no producer of the run writes"
        )
        sink.violated(
            torn,
            Attribution.UNDETERMINED,
            f"bytes/{channel}: torn-read: {recorded.id} returned {length} bytes "
            f"on {path} {described}",
            (
                *_read_witnesses(recorded),
                *(Witness("spliced", t) for t in shape.spliced),
            ),
        )
        sink.satisfied(fabricated)
        return None
    sink.satisfied(torn)
    if shape.kind == FABRICATED:
        sink.violated(
            fabricated,
            Attribution.UNDETERMINED,
            f"bytes/{channel}: fabricated-value: {recorded.id} returned "
            f"{length} bytes on {path} that no producer of the run can write",
            _read_witnesses(recorded),
        )
        return None
    sink.satisfied(fabricated)
    stale = sink.obligation(
        STALE_READ, recorded.id, subject=subject, witnesses=identity
    )
    return read_observation(recorded, (stale,))


def read_observation(
    recorded: ActionFacts, obligations: tuple[ObligationRef, ...]
) -> Observation:
    """Return the observation that a read saw its path hold the bytes it returned."""
    assert recorded.result is not None
    assert recorded.invoked is not None and recorded.completed is not None
    path = normalize_path(recorded.payload["path"])
    return Observation(
        id=f"o:{recorded.id}:bytes",
        action=recorded.id,
        cells=(path_cell(path),),
        expect=Expect(((eq(Value(FILE, (recorded.result.get("sha256"),))),),)),
        window=Window(recorded.invoked, recorded.completed),
        channel=recorded.payload["channel"],
        obligations=obligations,
    )


def directly_legal(recorded: ActionFacts, candidates: Candidates) -> bool:
    """Return whether a read's bytes pass the direct rules (neither torn nor fabricated)."""
    assert recorded.result is not None
    own = candidates.of_path(normalize_path(recorded.payload["path"]))
    return classify(recorded.result, own, candidates.anywhere).kind == CANDIDATE


def collect_bytes(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every `ok` primary-side read of the run, in spec order.

    A read whose result did not decode is the core result-schema
    property's to report, so it observes nothing here.
    """
    spec = check_input.spec
    candidates = Candidates(spec, eden_pinned(spec, check_input.pinned))
    written = {token for values in candidates.writes.values() for token in values}
    for recorded in check_input.facts.actions_of(READ):
        if not judged(recorded):
            continue
        observation = _judge_read(recorded, candidates, sink)
        if observation is not None:
            sink.observe(observation)
        assert recorded.result is not None
        if recorded.result.get("sha256") in written:
            sink.covered(WRITE_OBSERVED)


BYTES_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        RuleDef(
            id=STALE_READ,
            statement="A successful read's bytes equal the path's visible value at "
            "some point inside the read's window, whatever produced it (initial "
            "state, dirt, writes, transfers, checkouts, indeterminate effects).",
            subject_kind=SubjectKind.PATH,
            identity_roles=("channel",),
        ),
        RuleDef(
            id=TORN_READ,
            statement="A read returns a whole candidate value of the run, never "
            "a splice of two candidate values of its path or other bytes of "
            "register width.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("channel",),
            red_controls=frozenset({TORN_WRITE_CONTROL}),
        ),
        RuleDef(
            id=FABRICATED_VALUE,
            statement="A read returns only bytes some producer of the run can "
            "write: the initial value or dirt, a committed value of the fixture, "
            "or a write value.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("channel",),
        ),
    )
}

BYTES_PROPERTY = PropertyDef(
    id=BYTES,
    collect=collect_bytes,
    rules=BYTES_RULES,
    coverage=(
        CoverageObligation(
            WRITE_OBSERVED, "Reads that returned a value some write of the run produces"
        ),
    ),
    consumes=CONSUMES,
)
