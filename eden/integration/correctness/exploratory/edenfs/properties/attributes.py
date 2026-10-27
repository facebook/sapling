# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `attributes` property: what one `getattr` may report, on shared effects.

A FUSE `getattr` is one stat reply: its type, mode, and size describe one
inode at one point inside the action's window. Each judgment is an engine
input over the path's contract cells, so repeated metadata values (a mode
set back to an earlier one, a path re-created with old bytes) are the
engine's finite choice, never a shortcut. The Thrift type the
same action records comes from a second call at another point; the two
are compared only when nothing can change the path's binding in between.

- `attributes.type-mismatch`: the FUSE type is the class of the `path`
  cell's value at a point inside the window (no claim where the engine
  places no binding: that is the existence rules'). Declared for every
  path no mkdir names (`existence.mkdir-type` owns those), so an untouched
  path is judged against its pinned initial and revision values. When the
  window is `disturbed`, so the Thrift type is not compared with FUSE's,
  the Thrift type is a second such observation (scope `<getattr>:thrift`).
- `attributes.mode-revert`: a file's FUSE executable bit equals the `mode`
  cell at a point where the `path` cell holds a file. Only when an inode
  can leave the path (a move from it, an open of it, or a checkout) may
  the stat describe a detached inode, so only then is a point with a
  directory or no binding no claim. Declared for every file getattr,
  untouched paths included.
- `attributes.size-mismatch`: a file's size is the length of a candidate
  value the `path` cell holds at a point inside the window. When an inode
  can leave the path and some value of a path the spec's moves can carry
  it to has that size, the cell may instead hold a directory or no
  binding (those are the type and existence rules' claims, never filed
  twice here).

The three are observations (`type_observation`, `mode_observation`,
`size_observation`), which the differential property reuses as the
EdenFS side's legality evidence for a getattr. Their identity names only
what the getattr reported.
- `attributes.type-disagreement`: FUSE and Thrift report one type, decided
  directly, when the window is not `disturbed` for the path cell and no
  fault targets the getattr (`common.faulted`).
- `attributes.mtime-decreased`: between two getattrs of a path, the later
  one's `mtime_ns` is not lower, decided directly, when their joint span
  is not `disturbed` for the path's `path` and `mode` cells. One
  obligation per later getattr; no mtime value is predicted.
"""

from __future__ import annotations

from collections.abc import Mapping

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    ANY,
    Atom,
    eq,
    Expect,
    normalize_path,
    ObligationRef,
    Observation,
    of_class,
    one_of,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import ActionFacts, OK
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
from eden.integration.correctness.exploratory.edenfs.actions import MOVE_KINDS
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    file_value,
    mode_cell,
    mode_value,
    NEVER,
    path_cell,
)
from eden.integration.correctness.exploratory.edenfs.fixture import DIRECTORY, FILE
from eden.integration.correctness.exploratory.edenfs.properties.bytes import Candidates
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    disturbed,
    eden_pinned,
    faulted,
)

ATTRIBUTES = "attributes"
TYPE_DISAGREEMENT = f"{ATTRIBUTES}.type-disagreement"
MODE_REVERT = f"{ATTRIBUTES}.mode-revert"
TYPE_MISMATCH = f"{ATTRIBUTES}.type-mismatch"
SIZE_MISMATCH = f"{ATTRIBUTES}.size-mismatch"
MTIME_DECREASED = f"{ATTRIBUTES}.mtime-decreased"
TYPES_COMPARED = "attributes.types-compared"
MTIMES_COMPARED = "attributes.mtimes-compared"
GETATTR = "getattr"
EXECUTABLE_BITS = 0o111
CONSUMES = frozenset(
    {
        (GETATTR, "size"),
        (GETATTR, "mode"),
        (GETATTR, "mtime_ns"),
        (GETATTR, "fuse_type"),
        (GETATTR, "thrift_type"),
    }
)


def _stats(check_input: CheckInput) -> list[ActionFacts]:
    return [
        recorded
        for recorded in check_input.facts.actions_of(GETATTR)
        if recorded.outcome == OK
        and recorded.result is not None
        and recorded.invoked is not None
        and recorded.completed is not None
    ]


def _judge_type(
    check_input: CheckInput, recorded: ActionFacts, sink: CollectorSink
) -> None:
    assert recorded.result is not None
    fuse_type = recorded.result.get("fuse_type")
    thrift_type = recorded.result.get("thrift_type")
    path = normalize_path(recorded.payload["path"])
    if thrift_type is None or faulted(check_input.facts, recorded):
        return
    if disturbed(check_input.effects, (path_cell(path),), recorded.window):
        if not _made_by_mkdir(check_input.spec, path):
            ref = sink.obligation(
                TYPE_MISMATCH,
                f"{recorded.id}:thrift",
                subject=path_subject(path),
                witnesses=(Witness("fuse-type", thrift_type),),
            )
            sink.observe(type_observation(recorded, (ref,), "thrift"))
        return
    ref = sink.obligation(
        TYPE_DISAGREEMENT,
        recorded.id,
        subject=path_subject(path),
        witnesses=(
            Witness("fuse-type", fuse_type),
            Witness("thrift-type", thrift_type),
        ),
    )
    sink.covered(TYPES_COMPARED)
    if fuse_type == thrift_type:
        sink.satisfied(ref)
        return
    sink.violated(
        ref,
        Attribution.PRODUCT,
        f"attributes/fuse≠thrift: {recorded.id} reports {fuse_type} over FUSE and "
        f"{thrift_type} via Thrift on {path}, which nothing could change in between",
        (Witness("stat-action", recorded.id),),
    )


_TYPES: Mapping[str, Atom] = {
    "file": of_class({FILE}),
    "directory": of_class({DIRECTORY}),
}
_UNBOUND = eq(ABSENT)


def type_observation(
    recorded: ActionFacts,
    obligations: tuple[ObligationRef, ...],
    channel: str = "fuse",
) -> Observation:
    """Return the observation that the path's binding is of the channel's type, or absent."""
    assert recorded.result is not None
    reported = _TYPES.get(recorded.result.get(f"{channel}_type"), NEVER)
    return Observation(
        id=f"o:{recorded.id}:type" + ("" if channel == "fuse" else f":{channel}"),
        action=recorded.id,
        cells=(path_cell(normalize_path(recorded.payload["path"])),),
        expect=Expect(((reported,), (_UNBOUND,))),
        window=recorded.window,
        channel=channel,
        obligations=obligations,
    )


def mode_observation(
    recorded: ActionFacts,
    obligations: tuple[ObligationRef, ...],
    detachable: bool = True,
) -> Observation | None:
    """Return the observation of a file's executable bit, or None for a non-file.

    A `detachable` path (the stat may describe an inode that left it) also
    allows a point where the path holds a directory or no binding.
    """
    assert recorded.result is not None
    if recorded.result.get("fuse_type") != "file":
        return None
    path = normalize_path(recorded.payload["path"])
    executable = bool(recorded.result.get("mode") & EXECUTABLE_BITS)
    return Observation(
        id=f"o:{recorded.id}:mode",
        action=recorded.id,
        cells=(path_cell(path), mode_cell(path)),
        expect=Expect(
            (
                (of_class({FILE}), eq(mode_value(executable))),
                *([(of_class({ABSENT.cls, DIRECTORY}), ANY)] if detachable else []),
            )
        ),
        window=recorded.window,
        channel="fuse",
        obligations=obligations,
    )


def size_observation(
    recorded: ActionFacts,
    candidates: Candidates,
    obligations: tuple[ObligationRef, ...],
    detachable: bool = True,
) -> Observation | None:
    """Return the observation of a file's size, or None for a non-file.

    The path holds, at some point, a candidate file of that length, or,
    when `detachable`, a directory or no binding.
    """
    assert recorded.result is not None
    if recorded.result.get("fuse_type") != "file":
        return None
    path = normalize_path(recorded.payload["path"])
    size = recorded.result.get("size")
    matching = [
        file_value(data)
        for data in candidates.of_path(path).values()
        if len(data) == size
    ]
    rows = [(of_class({ABSENT.cls, DIRECTORY}),)] if detachable else []
    if matching:
        rows.append((one_of(matching),))
    if not rows:
        rows.append((NEVER,))
    return Observation(
        id=f"o:{recorded.id}:size",
        action=recorded.id,
        cells=(path_cell(path),),
        expect=Expect(tuple(rows)),
        window=recorded.window,
        channel="fuse",
        obligations=obligations,
    )


def _made_by_mkdir(spec: RunSpec, path: str) -> bool:
    return any(
        action.kind == "mkdir" and normalize_path(action.payload["path"]) == path
        for action in spec.actions
    )


def getattr_observations(
    check_input: CheckInput,
    recorded: ActionFacts,
    candidates: Candidates,
    sink: CollectorSink,
    obligations: tuple[ObligationRef, ...] | None = None,
) -> list[Observation]:
    """Return a getattr's type, mode, and size observations.

    Each files under its own rule's obligation, declared here, unless
    `obligations` names the ones they all support (the differential
    property's legality evidence).
    """
    path = normalize_path(recorded.payload["path"])
    subject = path_subject(path)

    def under(rule: str, *witnesses: Witness) -> tuple[ObligationRef, ...]:
        if obligations is not None:
            return obligations
        return (
            sink.obligation(rule, recorded.id, subject=subject, witnesses=witnesses),
        )

    found: list[Observation] = []
    if not _made_by_mkdir(check_input.spec, path):
        assert recorded.result is not None
        fuse_type = Witness("fuse-type", recorded.result.get("fuse_type"))
        found.append(type_observation(recorded, under(TYPE_MISMATCH, fuse_type)))
    if recorded.result is not None and recorded.result.get("fuse_type") == "file":
        # The stat describes the inode the path walk found; only an inode
        # that can leave the path (moved away, swapped by a checkout, or
        # held open) can change after it stopped being the path's.
        detachable = any(
            (a.kind in MOVE_KINDS and normalize_path(a.payload["source"]) == path)
            or (a.kind == "open" and normalize_path(a.payload["path"]) == path)
            or a.kind == "checkout"
            for a in check_input.spec.actions
        )
        mode = mode_observation(recorded, under(MODE_REVERT), detachable)
        # A detached inode still holds a value of the path or of a path a
        # move carries it to: a size none of those has is wrong wherever it went.
        size = size_observation(
            recorded,
            candidates,
            under(SIZE_MISMATCH),
            detachable
            and any(
                len(data) == recorded.result.get("size")
                for carried in _carried(check_input.spec, path)
                for data in candidates.of_path(carried).values()
            ),
        )
        found += [item for item in (mode, size) if item is not None]
    return found


def _carried(spec: RunSpec, path: str) -> set[str]:
    """Return `path` and every path a chain of moves can carry its inode to."""
    found = {path}
    pending = [path]
    while pending:
        current = pending.pop()
        for action in spec.actions:
            if (
                action.kind in MOVE_KINDS
                and normalize_path(action.payload["source"]) == current
            ):
                moved = normalize_path(action.payload["destination"])
                if moved not in found:
                    found.add(moved)
                    pending.append(moved)
    return found


def _judge_mtimes(
    check_input: CheckInput, stats: list[ActionFacts], sink: CollectorSink
) -> None:
    """Compare each later getattr with every earlier comparable one of its path."""
    timed = [
        recorded
        for recorded in stats
        if recorded.result is not None and recorded.result.get("mtime_ns") is not None
    ]
    for later in timed:
        path = normalize_path(later.payload["path"])
        cells = frozenset({path_cell(path), mode_cell(path)})
        earlier = [
            first
            for first in timed
            if normalize_path(first.payload["path"]) == path
            and first.window.before(later.window)
            and not disturbed(
                check_input.effects, cells, Window(first.window.lo, later.window.hi)
            )
        ]
        if not earlier:
            continue
        ref = sink.obligation(MTIME_DECREASED, later.id, subject=path_subject(path))
        sink.covered(MTIMES_COMPARED, len(earlier))
        assert later.result is not None
        mtime = later.result.get("mtime_ns")
        above = [
            first
            for first in earlier
            if first.result is not None and first.result.get("mtime_ns") > mtime
        ]
        if not above:
            sink.satisfied(ref)
            continue
        first = above[0]
        assert first.result is not None
        sink.violated(
            ref,
            Attribution.UNDETERMINED,
            f"attributes/fuse≠model: mtime-decreased: {later.id} reports mtime_ns "
            f"{mtime} on {path}, below {first.id}'s {first.result.get('mtime_ns')} "
            "with nothing writing the path in between",
            (Witness("earlier-stat-action", first.id),),
        )


def collect_attributes(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every `ok` getattr: type, mode, and size, then the mtime order."""
    spec = check_input.spec
    candidates = Candidates(spec, eden_pinned(spec, check_input.pinned))
    stats = _stats(check_input)
    for recorded in stats:
        _judge_type(check_input, recorded, sink)
        for observation in getattr_observations(
            check_input, recorded, candidates, sink
        ):
            sink.observe(observation)
    _judge_mtimes(check_input, stats, sink)


ATTRIBUTES_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        RuleDef(
            id=TYPE_DISAGREEMENT,
            statement="FUSE and Thrift report one type for a path nothing can rebind "
            "over the getattr: FUSE answers through kernel caches EdenFS must "
            "invalidate, Thrift resolves the mount state directly.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("fuse-type", "thrift-type"),
        ),
        RuleDef(
            id=TYPE_MISMATCH,
            statement="A getattr's FUSE type is the type of the path's visible binding "
            "at a point inside the getattr window where the path is bound.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("fuse-type",),
        ),
        RuleDef(
            id=MODE_REVERT,
            statement="A file's FUSE executable bit equals its visible mode at a point "
            "inside the getattr window where the path holds a file.",
            subject_kind=SubjectKind.PATH,
        ),
        RuleDef(
            id=SIZE_MISMATCH,
            statement="A file's size equals the length of its visible content at a "
            "point inside the getattr window.",
            subject_kind=SubjectKind.PATH,
        ),
        RuleDef(
            id=MTIME_DECREASED,
            statement="A path's mtime does not decrease between two ordered getattrs "
            "when no effect group writing its content or mode has a window meeting "
            "their span.",
            subject_kind=SubjectKind.PATH,
        ),
    )
}

ATTRIBUTES_PROPERTY = PropertyDef(
    id=ATTRIBUTES,
    collect=collect_attributes,
    rules=ATTRIBUTES_RULES,
    coverage=(
        CoverageObligation(
            TYPES_COMPARED, "Getattrs whose FUSE and Thrift types were compared"
        ),
        CoverageObligation(MTIMES_COMPARED, "Getattr pairs whose mtimes were compared"),
    ),
    consumes=CONSUMES,
)
