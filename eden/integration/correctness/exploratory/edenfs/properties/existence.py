# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `existence` property: which paths exist, judged on shared effects.

A presence observer is a `getattr`: `ok` observes the path present,
`ENOENT` observes it absent (its `core.input-legality` observation then
holds in every state, `contracts.checkout.state_observation`). A read's presence is
its bytes observation and its `ENOENT` the `core.input-legality` one; each
piece of evidence is filed under one rule. Every observation is one
engine input over the path's `path` cell inside the observer's window;
the engine decides it over every explanation of the contract effects
(renames as atomic transfers, unlinks, `sl rm`, mkdirs, progressive
checkouts, indeterminate outcomes). Nothing here chooses an order or an
outcome; identity values are read from the observer's own typed facts
only, never from which actions completed before it.

- `existence.missing-after-create`: an absence observation holds at a
  point of its window. Declared for every absence observation.
- `existence.present-after-remove`: a presence observation holds at a
  point of its window. Declared when some contract effect other than a
  write's can bind or unbind the path (otherwise only the initial state
  explains it: not an obligation).
- `existence.rename-atomicity`: observers of an acked rename's source and
  destination inside its window, two of them ordered in real time and
  agreeing on presence, are filed here (and not under the two rules
  above): the rename is one atomic transfer group, so the engine rejects
  an ordered pair no single swap point explains.
- `existence.directory-read`: a FUSE read (`ok` or `ENOENT`) of a path
  some effect can make a directory observes it as no directory at some
  point; a read failing `EISDIR` satisfies it directly.
- `existence.mkdir-type`: a `getattr` of a path some mkdir names observes
  the type FUSE reports, and separately the type Thrift reports, each at
  its own point and under its own obligation (scope `<getattr>` for FUSE,
  `<getattr>:thrift` for Thrift): the two reports are two claims, so two
  channels agreeing against the model corroborate each other through the
  engine's alternative-witness test (one serving group, so
  `undetermined`), while one disagreeing channel is never a co-observer
  of the other's claim.
"""

from __future__ import annotations

import errno
from collections.abc import Iterator, Mapping
from dataclasses import dataclass

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    Atom,
    Cell,
    EffectGroup,
    EffectKind,
    EffectSet,
    eq,
    Expect,
    none_of,
    normalize_path,
    ObligationRef,
    Observation,
    of_class,
    UnknownInitial,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import ActionFacts, FAIL, OK
from eden.integration.correctness.exploratory.core.property import (
    CheckInput,
    checkout_subject,
    CollectorSink,
    CoverageObligation,
    path_subject,
    PropertyDef,
    RuleDef,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    DIR,
    GETATTR_ABSENCE_RULE,
    NEVER,
    path_cell,
)
from eden.integration.correctness.exploratory.edenfs.fixture import DIRECTORY, FILE

EXISTENCE = "existence"
DIRECTORY_READ = f"{EXISTENCE}.directory-read"
MISSING_AFTER_CREATE = GETATTR_ABSENCE_RULE
PRESENT_AFTER_REMOVE = f"{EXISTENCE}.present-after-remove"
MKDIR_TYPE = f"{EXISTENCE}.mkdir-type"
RENAME_ATOMICITY = f"{EXISTENCE}.rename-atomicity"
OBSERVED = "existence.presence-observed"
UNAVAILABLE = "unavailable"
CONSUMES = frozenset({("getattr", "fuse_type"), ("getattr", "thrift_type")})

_DIR = of_class({DIRECTORY})
_PRESENT = of_class({FILE, DIRECTORY})
_TYPES: Mapping[str, Atom] = {"file": of_class({FILE}), "directory": _DIR}


@dataclass(frozen=True)
class _Observer:
    """One completed presence observer: its facts, path, and what it observed."""

    facts: ActionFacts
    path: str
    present: bool

    @property
    def window(self) -> Window:
        return self.facts.window


def _errno(recorded: ActionFacts) -> int | None:
    return None if recorded.result is None else recorded.result.errno


def _is_observer(recorded: ActionFacts) -> bool:
    return recorded.kind == "getattr"


def _observers(check_input: CheckInput) -> list[_Observer]:
    found = []
    for recorded in check_input.facts.actions.values():
        if (
            not _is_observer(recorded)
            or recorded.result is None
            or recorded.invoked is None
            or recorded.completed is None
        ):
            continue
        if recorded.outcome == OK:
            present = True
        elif recorded.outcome == FAIL and _errno(recorded) == errno.ENOENT:
            present = False
        else:
            continue
        path = normalize_path(recorded.payload["path"])
        found.append(_Observer(recorded, path, present))
    return sorted(found, key=lambda item: item.facts.id)


def _changes(effects: EffectSet, cell: Cell, group: EffectGroup) -> tuple[bool, bool]:
    """Return whether a group's effects can remove and can create `cell`'s binding."""
    removes = creates = False
    for effect in group.effects:
        if effect.kind == EffectKind.TRANSFER and effect.source == cell:
            removes = True
        if effect.cell != cell:
            continue
        if effect.kind == EffectKind.TRANSFER or effect.value != ABSENT:
            creates = True
        else:
            removes = True
    return removes, creates


def _rebinds(effects: EffectSet, spec: RunSpec, cell: Cell) -> bool:
    """Return whether some non-write action's contract effects can bind or unbind the cell."""
    return any(
        spec.action(group.action).kind != "write"
        and any(_changes(effects, cell, group))
        for group in effects.writers((cell,))
    )


def _can_be_directory(effects: EffectSet, cell: Cell) -> bool:
    try:
        if effects.initial(cell).cls == DIRECTORY:
            return True
    except UnknownInitial:
        return True
    for group in effects.writers((cell,)):
        for effect in group.effects:
            if effect.cell == cell and (
                effect.kind == EffectKind.TRANSFER
                or (effect.value is not None and effect.value.cls == DIRECTORY)
            ):
                return True
    return False


def _observation(
    observer: _Observer, part: str, atom: Atom, ref: ObligationRef, channel: str
) -> Observation:
    return Observation(
        id=f"o:{observer.facts.id}:{part}",
        action=observer.facts.id,
        cells=(path_cell(observer.path),),
        expect=Expect(((atom,),)),
        window=observer.window,
        channel=channel,
        obligations=(ref,),
    )


def _presence_atom(observer: _Observer) -> Atom:
    return _PRESENT if observer.present else eq(ABSENT)


def _agreeing_pairs(
    sources: list[_Observer], destinations: list[_Observer]
) -> Iterator[tuple[_Observer, _Observer]]:
    """Yield (earlier, later) observer pairs across the two paths that agree on presence."""
    for first in sources:
        for second in destinations:
            if first.present != second.present:
                continue
            if first.window.before(second.window):
                yield first, second
            elif second.window.before(first.window):
                yield second, first


def _judge_renames(
    check_input: CheckInput, observers: list[_Observer], sink: CollectorSink
) -> set[str]:
    """File observers inside acked renames under rename-atomicity; return their IDs."""
    filed: set[str] = set()
    for recorded in check_input.facts.actions_of("rename"):
        if recorded.outcome != OK or recorded.completed is None:
            continue
        assert recorded.invoked is not None
        span = Window(recorded.invoked, recorded.completed)
        source = normalize_path(recorded.payload["source"])
        destination = normalize_path(recorded.payload["destination"])
        inside = [o for o in observers if span.contains(o.window)]
        sources = [o for o in inside if o.path == source]
        destinations = [o for o in inside if o.path == destination]
        pairs = sorted(
            _agreeing_pairs(sources, destinations),
            key=lambda pair: (pair[0].window.hi, pair[1].window.lo, pair[0].facts.id),
        )
        if not pairs:
            continue
        state = "both-present" if pairs[0][0].present else "both-absent"
        ref = sink.obligation(
            RENAME_ATOMICITY,
            recorded.id,
            subject=checkout_subject(recorded.id),
            witnesses=(Witness("state", state),),
        )
        for observer in (*sources, *destinations):
            sink.observe(
                _observation(
                    observer,
                    f"atomic:{recorded.id}",
                    _presence_atom(observer),
                    ref,
                    "fuse",
                )
            )
            filed.add(observer.facts.id)
    return filed


def _judge_presence(
    spec: RunSpec, effects: EffectSet, observer: _Observer, sink: CollectorSink
) -> None:
    cell = path_cell(observer.path)
    subject = path_subject(observer.path)
    if observer.present:
        if not _rebinds(effects, spec, cell):
            return
        ref = sink.obligation(
            PRESENT_AFTER_REMOVE,
            observer.facts.id,
            subject=subject,
            witnesses=(Witness("observation-kind", observer.facts.kind),),
        )
    else:
        ref = sink.obligation(MISSING_AFTER_CREATE, observer.facts.id, subject=subject)
    sink.observe(
        _observation(observer, "presence", _presence_atom(observer), ref, "fuse")
    )
    sink.covered(OBSERVED)


def _judge_directory_reads(check_input: CheckInput, sink: CollectorSink) -> None:
    for recorded in check_input.facts.actions_of("read"):
        if (
            recorded.payload["channel"] != "fuse"
            or recorded.result is None
            or recorded.invoked is None
            or recorded.completed is None
        ):
            continue
        code = _errno(recorded)
        if recorded.outcome == OK:
            observed = "ok"
        elif recorded.outcome == FAIL and code in (errno.ENOENT, errno.EISDIR):
            observed = "ENOENT" if code == errno.ENOENT else "EISDIR"
        else:
            continue
        path = normalize_path(recorded.payload["path"])
        if code != errno.EISDIR and not _can_be_directory(
            check_input.effects, path_cell(path)
        ):
            continue
        ref = sink.obligation(
            DIRECTORY_READ,
            recorded.id,
            subject=path_subject(path),
            witnesses=(Witness("observed", observed),),
        )
        if code == errno.EISDIR:
            sink.satisfied(ref)
            continue
        observer = _Observer(recorded, path, recorded.outcome == OK)
        sink.observe(_observation(observer, "not-dir", none_of({DIR}), ref, "fuse"))


def _made_by_mkdir(effects: EffectSet, spec: RunSpec, path: str) -> bool:
    return any(
        spec.action(group.action).kind == "mkdir"
        for group in effects.writers((path_cell(path),))
    )


def _judge_mkdir_types(
    check_input: CheckInput, observers: list[_Observer], sink: CollectorSink
) -> None:
    for observer in observers:
        recorded = observer.facts
        if recorded.kind != "getattr" or not observer.present:
            continue
        if not _made_by_mkdir(check_input.effects, check_input.spec, observer.path):
            continue
        result = recorded.result
        assert result is not None
        fuse_type = result.get("fuse_type")
        thrift_type = result.get("thrift_type")
        witnesses = (
            Witness("fuse-type", fuse_type),
            Witness("thrift-type", thrift_type or UNAVAILABLE),
        )
        reported = [("fuse-type", fuse_type, "fuse", recorded.id)]
        if thrift_type is not None:
            reported.append(
                ("thrift-type", thrift_type, "thrift", f"{recorded.id}:thrift")
            )
        for part, kind, channel, scope in reported:
            ref = sink.obligation(
                MKDIR_TYPE,
                scope,
                subject=path_subject(observer.path),
                witnesses=witnesses,
            )
            atom = _TYPES.get(kind, NEVER)
            sink.observe(_observation(observer, part, atom, ref, channel))


def collect_existence(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every presence, directory-read, mkdir-type, and rename-atomicity obligation."""
    observers = _observers(check_input)
    filed = _judge_renames(check_input, observers, sink)
    for observer in observers:
        if observer.facts.id not in filed:
            _judge_presence(check_input.spec, check_input.effects, observer, sink)
    _judge_directory_reads(check_input, sink)
    _judge_mkdir_types(check_input, observers, sink)


EXISTENCE_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        RuleDef(
            id=DIRECTORY_READ,
            statement="A by-path FUSE read of a path that is a directory at every "
            "point of its window fails EISDIR.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("observed",),
        ),
        RuleDef(
            id=MISSING_AFTER_CREATE,
            statement="A getattr failing ENOENT observes the path absent at a point "
            "inside its window: a path a rename, mkdir, or checkout created and "
            "nothing removed is present.",
            subject_kind=SubjectKind.PATH,
            identity_roles=(),
        ),
        RuleDef(
            id=PRESENT_AFTER_REMOVE,
            statement="A successful getattr observes the path present at a point "
            "inside its window: a path an unlink, sl rm, rename source, or checkout "
            "removed and nothing re-created is absent.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("observation-kind",),
        ),
        RuleDef(
            id=MKDIR_TYPE,
            statement="A directory a mkdir made is a directory on FUSE and on Thrift, "
            "each observed at its own point of the getattr window.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("fuse-type", "thrift-type"),
        ),
        RuleDef(
            id=RENAME_ATOMICITY,
            statement="A rename moves the source entry to the destination in one "
            "step (atomic transfer group). Once two observers ordered in real time "
            "inside an acked rename agree on presence, every observer of the source "
            "or destination inside it is explained by that one transfer; any "
            "contradiction among them violates this rule.",
            subject_kind=SubjectKind.CHECKOUT,
            identity_roles=("state",),
        ),
    )
}

EXISTENCE_PROPERTY = PropertyDef(
    id=EXISTENCE,
    collect=collect_existence,
    rules=EXISTENCE_RULES,
    coverage=(
        CoverageObligation(OBSERVED, "Presence observations judged by the engine"),
    ),
    consumes=CONSUMES,
)
