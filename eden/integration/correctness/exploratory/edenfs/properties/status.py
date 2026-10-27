# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `status` property: what sl status, sl diff, and Thrift getScmStatusV2 report.

Per `ok` status and relevant path, the collector emits one observation over
the path's cells (`path`, `mode`, Sapling's `track` for sl, and the parent
cells the channel reads when more than one revision is possible) with one
Expect row per state that makes the reported code true, or `clean` for an
omitted path: sl status and Thrift status list every changed path, so an
omission is evidence (aggregate omission). Each path is observed at its
own point inside the status window; no atomic snapshot is claimed.
Every relevant path is judged, including one that is a directory at
some revision (its directory and absent states make an omission the
only legal report); untouched generated paths are judged through one
representative per equivalence class (the contracts' relevant paths),
and every path the status lists as an entry or a diff path is relevant
(`listed_paths`).

Which parent a status compares against is part of the observation, never
chosen here:

- `sl status`: the EdenFS parent (after a Thrift checkout Sapling's `.` is
  stale).
- `sl status --rev R`: R's manifest against Sapling's working copy, which
  takes a path the tree leaves clean against the EdenFS parent from
  Sapling's `.` (the `sapling` parent cell).
- Thrift status for R: R, which succeeds only while R is the EdenFS parent;
  without a revision it names Sapling's `.`, which must equal it.

The rows enumerate one state per class of the path's cell values (the
absent and directory values, each committed file a named revision holds,
any other file; every mode and track class; every possible parent) and
keep those in which the channel's code function (`_sl_codes`,
`_sl_rev_codes`, `_thrift_codes`) may report the listed code, so the
Expect is exactly that function's preimage.

An sl status result carries two outputs taken at two instants: `sl status`
and then a separate `sl diff --stat` (`backend_scm.status_sl`). The diff is
therefore its own observation (`sl-diff` channel) at its own point of the
action's window, over the path's cells and both parent cells, with one row
per state class in which `sl diff` may list (or omit) the path
(`_diff_listings`: an added path and an `sl rm` removal are
listed, a missing `!` path is not, an `sl mv` source is not, a content
change is; a mode-only change and any state while the two parents differ
are left open). Nothing about the diff is baked into the status
observation, so a writer, `sl rm`, rename, or checkout that lands between
the two commands is explained by placing the two observations apart.

The rule judging a path's observations follows from the two outputs
alone. When status and diff disagree (an `A` entry diff omits, a listed
path with no `M`/`A`/`R`/`!` entry), the two observations support the
`status.status-diff-mismatch` obligation and the engine decides it, as
it does for an `M` entry diff omits (which claims a mode-only change).
Agreeing outputs satisfy the comparison and file both observations: an
`M` both list under `status.model-live-stale-status`, an omission both
report under `status.sl-fuse-stale-status`. Everything else files the
status observation alone under `status.sl-model-stale-status`. Two
observations supporting one obligation are two observers of one serving
group, so their engine violations are `undetermined`. Other channels' observations of the path (a FUSE read) are their
own properties' evidence and never choose the rule. `sl diff` compares
with Sapling's `.`, not with a `--rev` revision, so a revision-qualified
status has no diff observation.

Identity names what the status reported (`observed`) and the kinds of
the run's actions whose declared footprint writes the path
(`producer-kinds`: a lost write and a lost unlink differ). Both come from
the RunSpec and the status output, never from when an action ran or how
it ended.
"""

from __future__ import annotations

import json
from collections.abc import Iterable, Mapping
from dataclasses import dataclass

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    ANY,
    Atom,
    Before,
    Cell,
    eq,
    Expect,
    none_of,
    normalize_path,
    Observation,
    of_class,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import ActionFacts
from eden.integration.correctness.exploratory.core.graph_rules import finish_before
from eden.integration.correctness.exploratory.core.property import (
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
from eden.integration.correctness.exploratory.edenfs.actions import (
    differential_footprint,
)
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    checkout_revisions,
    listed_paths,
    relevant_paths,
)
from eden.integration.correctness.exploratory.edenfs.contracts.files import (
    added_value,
    REMOVED,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    CLEAN,
    DIR,
    EDEN_PARENT,
    EdenPinned,
    mode_cell,
    mode_value,
    NEVER,
    parent_cell,
    path_cell,
    rev_value,
    SAPLING_PARENT,
    track_cell,
    within_footprint,
)
from eden.integration.correctness.exploratory.edenfs.fixture import FILE
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    eden_pinned,
    judged,
    path_scope,
)

STATUS = "status"
MODEL_LIVE = "status.model-live-stale-status"
SL_MODEL = "status.sl-model-stale-status"
# Judges two Sapling outputs (no FUSE observation); the ID is frozen for task keys.
SL_FUSE = "status.sl-fuse-stale-status"
DIFF_MISMATCH = "status.status-diff-mismatch"
THRIFT_MODEL = "status.thrift-model-stale-status"
STATUS_LOSS_CONTROL = "status-loss"
DIFF_LOSS_CONTROL = "diff-loss"
COMMON_STATUS_LOSS_CONTROL = "common-status-loss"
UNMODELED_CODE = "status.unmodeled-code"
STATUS_OBSERVED = "status.status-observed"
PRODUCER_KINDS = "producer-kinds"
NO_PRODUCER = "none"

CONSUMES = frozenset(
    {("status", "status.path"), ("status", "status.code"), ("status", "diff_paths")}
)

CLEAN_CODE = "clean"
ABSENT_CODE = "absent"
SL_CODES = ("M", "A", "R", "!", "?")
THRIFT_CODES = ("M", "A", "R")
# getScmStatusV2 names its codes; a status entry may carry either spelling.
_THRIFT_NAMES = {"MODIFIED": "M", "ADDED": "A", "REMOVED": "R", "IGNORED": "I"}
_ADDED = of_class({"added"})


@dataclass(frozen=True)
class _Probe:
    """One status action's channel, revision, and the parent cells it reads."""

    action: ActionFacts
    sl: bool
    revision: int | None
    candidates: tuple[int, ...]
    initial: int

    @property
    def revision_qualified(self) -> bool:
        """Return whether this is `sl status --rev`."""
        return self.sl and self.revision is not None

    @property
    def reads_sapling(self) -> bool:
        """Return whether the status reads Sapling's `.`: `--rev`, or Thrift without one."""
        return self.revision_qualified or (not self.sl and self.revision is None)

    @property
    def multi(self) -> bool:
        """Return whether more than one revision can be a parent during the run."""
        return len(self.candidates) > 1

    def cells(self, path: str, *, diff: bool = False) -> tuple[Cell, ...]:
        """Return the cells the status (or, with `diff`, its `sl diff`) reads for `path`.

        `sl diff` compares with Sapling's `.`, so it also reads the Sapling
        parent whenever more than one revision is possible.
        """
        local = [path_cell(path), mode_cell(path)]
        if self.sl:
            local.append(track_cell(path))
        if self.multi:
            local.append(parent_cell(EDEN_PARENT))
            if self.reads_sapling or diff:
                local.append(parent_cell(SAPLING_PARENT))
        return tuple(local)


_OTHER_FILE = Value(FILE, ("<another file>",))
_MODES = (ABSENT, mode_value(False), mode_value(True))
_ADDED_SAMPLE = added_value("")
# One sample per track class; `added` carries its copy source, which no code reads.
_TRACKS: tuple[tuple[Value, Atom], ...] = (
    (CLEAN, eq(CLEAN)),
    (REMOVED, eq(REMOVED)),
    (_ADDED_SAMPLE, _ADDED),
)


def _is_file(value: Value) -> bool:
    return value.cls == FILE


@dataclass(frozen=True)
class _Base:
    """A revision's committed entry and mode of one path."""

    entry: Value
    mode: Value


def _sl_codes(value: Value, mode: Value, track: Value, base: _Base) -> frozenset[str]:
    """Return the codes `sl status` may report for a path's state against `base`.

    The base is the EdenFS parent (after a Thrift checkout sl status
    reports dirt against EdenFS's parent). A path is tracked when
    the base holds a file or Sapling records it added; `added` onto a
    path the base holds is a tracked copy (`M` or clean, never `A`). A
    removed path is listed `R` whether or not its file reappeared
    (Sapling's `compute_status`, `scm/lib/workingcopy/src/status.rs:123`).
    """
    on_disk = _is_file(value)
    in_base = _is_file(base.entry)
    if track == REMOVED:
        return frozenset({"R"})
    if not (track.cls == "added" or in_base):
        return frozenset({"?"} if on_disk else {CLEAN_CODE})
    if not on_disk:
        return frozenset({"!"})
    if not in_base:
        return frozenset({"A"})
    changed = (value, mode) != (base.entry, base.mode)
    return frozenset({"M"} if changed else {CLEAN_CODE})


def _sl_rev_codes(
    value: Value, mode: Value, track: Value, eden: _Base, sapling: _Base, rev: _Base
) -> frozenset[str]:
    """Return the codes `sl status --rev R` may report for a path's state.

    Sapling diffs R's manifest with its working copy: `A` tracked there
    but not in R, `R` in R but not tracked, `!` tracked but missing, `?`
    untracked on disk, `M` content or mode against R (a clean checkout at
    the tip lists `A`, `R`, and `M` against revision 0). Sapling's
    working copy takes a path the EdenFS tree leaves clean against the
    EdenFS parent from its own `.` (after a Thrift checkout, `--rev` of
    the stale `.` is clean), so such a path is viewed
    as `.`'s entry.
    """
    if track == CLEAN and (value, mode) == (eden.entry, eden.mode):
        value, mode = sapling.entry, sapling.mode
    on_disk = _is_file(value)
    in_rev = _is_file(rev.entry)
    if track == REMOVED:
        return frozenset({"R"} if in_rev else {CLEAN_CODE})
    if not (track.cls == "added" or _is_file(sapling.entry)):
        if on_disk:
            return frozenset({"?", "R"} if in_rev else {"?"})
        return frozenset({"R"} if in_rev else {CLEAN_CODE})
    if not on_disk:
        return frozenset({"!"})
    if not in_rev:
        return frozenset({"A"})
    changed = (value, mode) != (rev.entry, rev.mode)
    return frozenset({"M"} if changed else {CLEAN_CODE})


def _thrift_codes(value: Value, mode: Value, base: _Base) -> frozenset[str]:
    """Return the codes getScmStatusV2 reports for a path against `base`.

    EdenFS compares its tree with the revision and ignores Sapling's
    dirstate: `A` a file the revision lacks, `R` a file the tree lost,
    `M` a changed file or mode.
    """
    on_disk, in_base = _is_file(value), _is_file(base.entry)
    if on_disk and not in_base:
        return frozenset({"A"})
    if in_base and not on_disk:
        return frozenset({"R"})
    if on_disk and (value, mode) != (base.entry, base.mode):
        return frozenset({"M"})
    return frozenset({CLEAN_CODE})


@dataclass(frozen=True)
class _State:
    """One enumerated state of a path's cells, with the atoms that select it."""

    value: Value
    mode: Value
    track: Value
    eden: int
    sapling: int
    atoms: tuple[Atom, ...]


def _states(
    probe: _Probe, pinned: EdenPinned, path: str, *, diff: bool = False
) -> list[_State]:
    """Return one state per class of the path's cell values, covering every value.

    A path value is the absent or directory value, a committed file of a
    revision the run can name, or another file (one class); mode and
    track values are finite; parents range over the possible revisions
    (fixed at the initial revision when only one is possible, with no
    cell). `diff` adds the Sapling parent, as `_Probe.cells` does.
    """
    revisions = set(probe.candidates)
    if probe.revision is not None:
        revisions.add(probe.revision)
    named = {ABSENT, DIR} | {pinned.committed(path, r) for r in sorted(revisions)}
    values = [(v, eq(v)) for v in sorted(named, key=_value_order)]
    values.append((_OTHER_FILE, none_of(named)))
    tracks = _TRACKS if probe.sl else ((CLEAN, None),)
    parents = probe.candidates if probe.multi else (probe.initial,)
    saplings = parents if probe.reads_sapling or (diff and probe.multi) else (None,)
    found = []
    for value, value_atom in values:
        for mode in _MODES:
            for track, track_atom in tracks:
                for eden in parents:
                    for sapling in saplings:
                        atoms: list[Atom] = [value_atom, eq(mode)]
                        if track_atom is not None:
                            atoms.append(track_atom)
                        if probe.multi:
                            atoms.append(eq(rev_value(eden)))
                            if sapling is not None:
                                atoms.append(eq(rev_value(sapling)))
                        found.append(
                            _State(
                                value,
                                mode,
                                track,
                                eden,
                                eden if sapling is None else sapling,
                                tuple(atoms),
                            )
                        )
    return found


def _value_order(value: Value) -> str:
    return json.dumps(value.document(), sort_keys=True)


def _base(pinned: EdenPinned, path: str, revision: int) -> _Base:
    return _Base(
        pinned.committed(path, revision), pinned.committed_mode(path, revision)
    )


def _codes(
    probe: _Probe, pinned: EdenPinned, path: str, state: _State
) -> frozenset[str]:
    """Return the codes the probe's status may report in one state; none if it fails.

    A Thrift status names a revision that must be the EdenFS parent (else
    `OUT_OF_DATE_PARENT`); without one it names Sapling's `.`.
    """
    if not probe.sl:
        named = probe.revision if probe.revision is not None else state.sapling
        if named != state.eden:
            return frozenset()
        return _thrift_codes(state.value, state.mode, _base(pinned, path, named))
    eden = _base(pinned, path, state.eden)
    if probe.revision is None:
        return _sl_codes(state.value, state.mode, state.track, eden)
    return _sl_rev_codes(
        state.value,
        state.mode,
        state.track,
        eden,
        _base(pinned, path, state.sapling),
        _base(pinned, path, probe.revision),
    )


def _rows(
    probe: _Probe, pinned: EdenPinned, path: str, code: str
) -> tuple[tuple[Atom, ...], ...]:
    """Return the observation's rows: every state class in which `code` may be reported.

    With no such state the single row is unsatisfiable.
    """
    rows = [
        state.atoms
        for state in _states(probe, pinned, path)
        if code in _codes(probe, pinned, path, state)
    ]
    if not rows:
        rows.append((NEVER, *([ANY] * (len(probe.cells(path)) - 1))))
    return tuple(dict.fromkeys(rows))


_EITHER = frozenset({True, False})
# Whether `sl diff --stat` lists a path sl status reports with each code
# against the same base, as a live EdenFS mount reports: an added path is
# listed; an `sl rm` removal is listed but an `sl mv` source is not; a
# missing (`!`) path is not listed; a clean or untracked path is not
# listed.
_LISTINGS: Mapping[str, frozenset[bool]] = {
    "A": frozenset({True}),
    "R": _EITHER,
    "!": frozenset({False}),
    "?": frozenset({False}),
    CLEAN_CODE: frozenset({False}),
}


def _diff_listings(pinned: EdenPinned, path: str, state: _State) -> frozenset[bool]:
    """Return whether `sl diff --stat` may list a path in one state: True, False, or both.

    A content change of a tracked file is listed; a mode-only change may
    or may not be (unmeasured). While the EdenFS parent and Sapling's `.`
    differ (after a Thrift checkout) the diff's base is not the status's,
    which no live measurement covered, so both are allowed.
    """
    if state.eden != state.sapling:
        return _EITHER
    base = _base(pinned, path, state.eden)
    found: set[bool] = set()
    for code in _sl_codes(state.value, state.mode, state.track, base):
        if code == "M":
            found |= {True} if state.value != base.entry else _EITHER
        else:
            found |= _LISTINGS[code]
    return frozenset(found)


def _diff_rows(
    probe: _Probe, pinned: EdenPinned, path: str, listed: bool
) -> tuple[tuple[Atom, ...], ...]:
    """Return the `sl diff` observation's rows: every state class giving its listing."""
    rows = [
        state.atoms
        for state in _states(probe, pinned, path, diff=True)
        if listed in _diff_listings(pinned, path, state)
    ]
    if not rows:
        rows.append((NEVER, *([ANY] * (len(probe.cells(path, diff=True)) - 1))))
    return tuple(dict.fromkeys(rows))


def _entries(recorded: ActionFacts, sl: bool) -> dict[str, str]:
    assert recorded.result is not None
    found: dict[str, str] = {}
    for entry in recorded.result.get("status"):
        code = entry["code"]
        found[normalize_path(entry["path"])] = (
            code if sl else _THRIFT_NAMES.get(code, code)
        )
    return found


def _producer_kinds(
    spec: RunSpec, paths: Iterable[str], status_id: str
) -> dict[str, str]:
    """Return, per path, the kinds of the actions whose declared footprint writes it.

    The value joins the sorted kinds with `+` (`unlink+write`); a path no
    action writes is absent. An action the spec orders after status
    `status_id` (`graph_rules.finish_before`) cannot have produced what it
    reported and is left out. Footprints and the order are static
    declarations, so the answer depends neither on when an action ran nor
    on its outcome (a write that failed ENOENT, an aborted `sl goto`).
    """
    footprints = [
        (action.kind, differential_footprint(spec, action))
        for action in spec.actions
        if status_id not in finish_before(spec, action.id)
    ]
    found = {}
    for path in paths:
        kinds = {
            kind
            for kind, footprint in footprints
            if within_footprint(path_cell(path), footprint)
        }
        if kinds:
            found[path] = "+".join(sorted(kinds))
    return found


@dataclass(frozen=True)
class _RuleChoice:
    """The rule judging a path's observations, and whether the `sl diff` one joins it."""

    rule: str
    diff: bool = False


def _diff_agrees(code: str | None, listed: bool) -> bool:
    """Return whether an sl status entry and its own `sl diff --stat` say the same.

    On a live mount, diff lists an added path and an `sl rm` removal, never
    a missing (`!`) path, and for `sl mv` only the destination, so an `A`
    entry must be listed and a listed path must have an entry; an `M`
    entry diff omits claims a mode-only change, which only the path's
    state can confirm.
    """
    if code in ("A", "M") and not listed:
        return False
    return not listed or code in ("M", "A", "R")


def _judge_sl(
    probe: _Probe,
    path: str,
    code: str | None,
    diff: frozenset[str] | None,
    sink: CollectorSink,
    identity: tuple[Witness, ...],
) -> _RuleChoice:
    """Compare sl status with its `sl diff` and choose the rule judging the path's observations.

    The choice reads the two outputs only: a disagreement, an omission
    both report, an `M` both list, or anything diff cannot corroborate.
    """
    if diff is None or probe.revision_qualified:
        return _RuleChoice(SL_MODEL)
    listed = path in diff
    ref = sink.obligation(
        DIFF_MISMATCH,
        path_scope(probe.action.id, path),
        subject=path_subject(path),
        witnesses=identity,
    )
    if not _diff_agrees(code, listed):
        return _RuleChoice(DIFF_MISMATCH, diff=True)
    sink.satisfied(ref)
    if code is None:
        return _RuleChoice(SL_FUSE, diff=True)
    if code == "M":
        return _RuleChoice(MODEL_LIVE, diff=True)
    return _RuleChoice(SL_MODEL)


def _probe(
    check_input: CheckInput, pinned: EdenPinned, recorded: ActionFacts
) -> _Probe:
    return _Probe(
        action=recorded,
        sl=recorded.payload["channel"] == "sl",
        revision=recorded.payload.get("revision"),
        candidates=checkout_revisions(check_input.spec, pinned),
        initial=pinned.initial_revision,
    )


def _judge_path(
    probe: _Probe,
    pinned: EdenPinned,
    path: str,
    entries: Mapping[str, str],
    diff: frozenset[str] | None,
    producers: Mapping[str, str],
    sink: CollectorSink,
) -> None:
    """Judge one path of one status: the direct comparison, then one observation."""
    recorded = probe.action
    code = entries.get(path)
    observed = ABSENT_CODE if code is None else code
    identity = (
        Witness("observed", observed),
        Witness(PRODUCER_KINDS, producers.get(path, NO_PRODUCER)),
    )
    if probe.sl:
        choice = _judge_sl(probe, path, code, diff, sink, identity)
    else:
        choice = _RuleChoice(THRIFT_MODEL)
    scope = path_scope(recorded.id, path)
    ref = sink.obligation(
        choice.rule, scope, subject=path_subject(path), witnesses=identity
    )
    modeled = SL_CODES if probe.sl else THRIFT_CODES
    if code is not None and code not in modeled:
        sink.not_evaluated(ref, UNMODELED_CODE, f"{recorded.id} lists {code!r}")
        return
    assert recorded.invoked is not None and recorded.completed is not None
    window = Window(recorded.invoked, recorded.completed)
    sink.observe(
        Observation(
            id=f"o:{scope}:status",
            action=recorded.id,
            cells=probe.cells(path),
            expect=Expect(_rows(probe, pinned, path, code or CLEAN_CODE)),
            window=window,
            channel="sl-status" if probe.sl else "thrift",
            obligations=(ref,),
        )
    )
    if choice.diff:
        assert diff is not None
        sink.observe(
            Observation(
                id=f"o:{scope}:diff",
                action=recorded.id,
                cells=probe.cells(path, diff=True),
                expect=Expect(_diff_rows(probe, pinned, path, path in diff)),
                window=window,
                channel="sl-diff",
                obligations=(ref,),
            )
        )
        # The executor runs `sl diff` only after `sl status` exits.
        sink.constrain(Before(f"o:{scope}:status", f"o:{scope}:diff", ref))


def collect_status(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every `ok` primary-side status, path by path, in spec then path order."""
    spec = check_input.spec
    pinned = eden_pinned(spec, check_input.pinned)
    paths = relevant_paths(spec, pinned, listed_paths(check_input.facts))
    for recorded in check_input.facts.actions_of(STATUS):
        if not judged(recorded):
            continue
        producers = _producer_kinds(spec, paths, recorded.id)
        probe = _probe(check_input, pinned, recorded)
        entries = _entries(recorded, probe.sl)
        assert recorded.result is not None
        raw_diff = recorded.result.get("diff_paths") if probe.sl else None
        diff = (
            None
            if raw_diff is None
            else frozenset(normalize_path(item) for item in raw_diff)
        )
        sink.covered(STATUS_OBSERVED)
        for path in paths:
            _judge_path(probe, pinned, path, entries, diff, producers, sink)


def _rule(
    rule_id: str,
    statement: str,
    *,
    controls: frozenset[str] = frozenset(),
) -> RuleDef:
    return RuleDef(
        id=rule_id,
        statement=statement,
        subject_kind=SubjectKind.PATH,
        identity_roles=("observed", PRODUCER_KINDS),
        red_controls=controls,
    )


STATUS_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        _rule(
            MODEL_LIVE,
            "An `M` entry sl diff also lists holds against the path's visible "
            "state at a point inside the status window, and so does the diff's "
            "listing at its own point.",
        ),
        _rule(
            SL_MODEL,
            "An sl status entry sl diff cannot corroborate holds against the "
            "path's visible state at a point inside the status window.",
        ),
        _rule(
            SL_FUSE,
            "A path sl status and sl diff both omit is clean in the visible "
            "(FUSE-served) tree at a point inside the status window, and so is "
            "the diff's omission at its own point.",
            controls=frozenset({COMMON_STATUS_LOSS_CONTROL}),
        ),
        _rule(
            DIFF_MISMATCH,
            "One sl status result's entries and diff paths agree on content "
            "changes: each holds against the path's visible state at its own "
            "point inside the status window (an `M` entry diff omits is a "
            "mode-only change).",
            controls=frozenset({STATUS_LOSS_CONTROL, DIFF_LOSS_CONTROL}),
        ),
        _rule(
            THRIFT_MODEL,
            "A Thrift getScmStatusV2 result for revision R holds against the "
            "path's visible state and an EdenFS parent equal to R at a point "
            "inside the status window.",
        ),
    )
}

STATUS_PROPERTY = PropertyDef(
    id=STATUS,
    collect=collect_status,
    rules=STATUS_RULES,
    coverage=(CoverageObligation(STATUS_OBSERVED, "Successful status results judged"),),
    not_evaluated_reasons={
        UNMODELED_CODE: "The status lists a code the property does not model",
    },
    consumes=CONSUMES,
)
