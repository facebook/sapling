# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `journal` property: acknowledged writes appear in complete journal windows.

A journal window is an `ok` `journal_window` that asked for changes since a
sequence (`changesSinceV2`). It is complete when it started at sequence
zero and reports no truncation and no lost changes; the backend refuses a
generation change that carries no loss notification, so a complete
window covers its whole mount generation up to its point.

The obligations are keyed on the contract ACKED write groups: a write
whose ack precedes a window's invocation is covered by that window and
every later one, and one obligation `(journal.missing-acked-write,
<first covering window>:<path>)` gathers every acked write of the path
with the same covering windows. Only acked writes are covered: an
indeterminate write may not have happened, and a write still in flight
when the window began may be journaled after it, since the journal
append and the bytes' visibility are not one established point.

For each covering complete window that omits the path, the collector
emits `MustAppearIn(write group, window observation)`; the engine
encodes it as "the window came first", which real time contradicts.
A path a covering window lists satisfies that window. Covering windows
that are all incomplete never accept an omission:
NotEvaluated(`journal.incomplete-window`), with the truncation and loss
facts kept in the detail and witnesses.

A complete window covers only its own mount generation, and a write
counts toward window W's generation (generation G) only when that is
established:

- in: some journal action (window or cursor peek) recorded G and
  completed before the write began, with no daemon exit from that
  record to W; or nothing that may precede W's point recorded another
  generation and no daemon exit may precede it.
- outside (W owes the write nothing): some journal action invoked
  after the write's ack and completed before W began recorded another
  generation, or a daemon exit lies strictly between the ack and W.
- otherwise unestablished: NotEvaluated(`journal.generation-unestablished`)
  when no other window decides the obligation.

A record of another generation only says the change happened somewhere
between that record's point and W's; a write acked after it may still
precede the change. Ties are never ordered. Outside is checked first,
so an impossible history (a generation recorded again after another
one, or after a daemon exit) never rejects.

Assumptions a daemon kill/restart migration must revisit: mount
generations are never reused, a daemon restart changes the generation,
and no generation changes without a recorded journal action or daemon
exit (MVP has no remount or restart action, and an unowned daemon exit
stops a live run). A
silent change before a window would make that window's omission of an
earlier write legal.
"""

from __future__ import annotations

import hashlib
from collections.abc import Mapping
from dataclasses import dataclass, field
from enum import Enum

from eden.integration.correctness.exploratory.core.effects import (
    ANY,
    EffectGroup,
    Expect,
    MustAppearIn,
    normalize_path,
    ObligationRef,
    Observation,
    Status,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import ActionFacts
from eden.integration.correctness.exploratory.core.history import INFO
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
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    DAEMON_EXIT,
    path_cell,
)
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    judged,
    path_scope,
)

JOURNAL = "journal"
MISSING_ACKED_WRITE = f"{JOURNAL}.missing-acked-write"
INCOMPLETE_WINDOW = f"{JOURNAL}.incomplete-window"
GENERATION_UNESTABLISHED = f"{JOURNAL}.generation-unestablished"
ACKED_WRITE_OBSERVED = f"{JOURNAL}.acked-write-observed"
WINDOW = "journal_window"
WRITE = "write"
CHANNEL = "journal"
GENERATION_UNESTABLISHED_STATEMENT = (
    "A complete window omits the path, and no record establishes whether the write "
    "ran in the window's mount generation: none of that generation finished before "
    "the write began, and another generation or a daemon exit may precede the window."
)
# Incomplete windows whose truncation and loss facts an obligation keeps.
MAX_GAP_WINDOWS = 4

CONSUMES = frozenset(
    (WINDOW, name)
    for name in ("mount_generation", "changed_paths.path", "truncated", "lost_changes")
)


@dataclass(frozen=True)
class _Window:
    """One judged journal window: its facts and what it says about completeness."""

    recorded: ActionFacts
    generation: int
    listed: frozenset[str]
    complete: bool
    gaps: tuple[str, ...]

    @property
    def id(self) -> str:
        return self.recorded.id

    @property
    def invoked(self) -> float:
        assert self.recorded.invoked is not None
        return self.recorded.invoked

    @property
    def window(self) -> Window:
        return self.recorded.window


def _window(recorded: ActionFacts) -> _Window:
    result = recorded.result
    assert result is not None
    since = recorded.payload.get("since_sequence")
    entries = result.get("changed_paths")
    truncated = result.get("truncated")
    lost = result.get("lost_changes")
    gaps = []
    if since != 0:
        gaps.append(f"since_sequence={since}")
    if entries is None:
        gaps.append("no changed_paths")
    if truncated is not False:
        gaps.append(f"truncated={truncated}")
    if lost is None or len(lost) > 0:
        gaps.append(f"lost_changes={None if lost is None else list(lost)}")
    return _Window(
        recorded=recorded,
        generation=result.get("mount_generation"),
        listed=frozenset(normalize_path(entry["path"]) for entry in entries or ()),
        complete=not gaps,
        gaps=tuple(gaps),
    )


@dataclass(frozen=True)
class _Record:
    """One journal action's recorded mount generation and its invocation window."""

    generation: int
    lo: float
    hi: float


class _Membership(Enum):
    IN = "in"
    OUTSIDE = "outside"
    UNESTABLISHED = "unestablished"


@dataclass(frozen=True)
class _Generations:
    """Every recorded mount generation and every daemon exit of a history."""

    records: tuple[_Record, ...]
    exits: tuple[float, ...]

    def membership(self, write: Window, window: _Window) -> _Membership:
        """Return whether a write counts toward a complete window's generation."""
        start, end = window.window.lo, window.window.hi
        other = [r for r in self.records if r.generation != window.generation]
        if any(write.hi < r.lo and r.hi < start for r in other) or any(
            write.hi < time < start for time in self.exits
        ):
            return _Membership.OUTSIDE
        if any(
            r.generation == window.generation
            and r.hi < write.lo
            and not any(r.lo <= time <= end for time in self.exits)
            for r in self.records
        ):
            return _Membership.IN
        if not any(r.lo <= end for r in other) and not any(
            time <= end for time in self.exits
        ):
            return _Membership.IN
        return _Membership.UNESTABLISHED


def _generations(check_input: CheckInput) -> _Generations:
    """Collect every `ok` journal action's generation and every daemon exit."""
    records = []
    for recorded in check_input.facts.actions_of(WINDOW):
        if not judged(recorded):
            continue
        assert recorded.result is not None
        assert recorded.invoked is not None and recorded.completed is not None
        records.append(
            _Record(
                recorded.result.get("mount_generation"),
                recorded.invoked,
                recorded.completed,
            )
        )
    exits = tuple(
        event.time
        for event in check_input.facts.run_events
        if event.type == INFO and event.data.get("reason") == DAEMON_EXIT
    )
    return _Generations(tuple(records), exits)


@dataclass
class _Obligation:
    """One `(first covering window, path)` obligation and the acked writes it gathers."""

    path: str
    covering: tuple[_Window, ...]
    writes: list[EffectGroup] = field(default_factory=list)


def _acked_writes(check_input: CheckInput) -> list[tuple[EffectGroup, str]]:
    """Return every ACKED contract group of a write action with its normalized path."""
    writes = {
        recorded.id: normalize_path(recorded.payload["path"])
        for recorded in check_input.facts.actions_of(WRITE)
    }
    return [
        (group, writes[group.action])
        for group in check_input.effects.groups
        if group.status == Status.ACKED and group.action in writes
    ]


def _observation_id(window: _Window, path: str) -> str:
    digest = hashlib.sha256(path.encode("utf-8", errors="surrogatepass")).hexdigest()
    return f"o:{window.id}:journal:p{digest[:16]}"


@dataclass
class _Decision:
    """What one obligation's covering windows decide."""

    window: _Window | None = None
    write: EffectGroup | None = None
    unestablished: list[str] = field(default_factory=list)
    unestablished_writes: set[str] = field(default_factory=set)


def _decision(obligation: _Obligation, generations: _Generations) -> _Decision:
    """Find the first complete covering window that omits the path from a write
    established in its generation; collect the windows whose generation is
    unestablished on the way. A window every write is outside of owes nothing.
    """
    decision = _Decision()
    for window in obligation.covering:
        if obligation.path in window.listed or not window.complete:
            continue
        membership = {
            group.id: generations.membership(group.window, window)
            for group in obligation.writes
        }
        inside = [g for g in obligation.writes if membership[g.id] == _Membership.IN]
        if inside:
            decision.window = window
            decision.write = max(inside, key=lambda group: (group.window.lo, group.id))
            return decision
        open_ = [
            g.action
            for g in obligation.writes
            if membership[g.id] == _Membership.UNESTABLISHED
        ]
        if open_:
            decision.unestablished.append(window.id)
            decision.unestablished_writes.update(open_)
    return decision


def _apply(
    obligation: _Obligation,
    decision: _Decision,
    ref: ObligationRef,
    sink: CollectorSink,
) -> None:
    """Record one obligation's constraint or its direct result."""
    window, write = decision.window, decision.write
    if window is not None and write is not None:
        observation = Observation(
            id=_observation_id(window, obligation.path),
            action=window.id,
            cells=(path_cell(obligation.path),),
            expect=Expect(((ANY,),)),
            window=window.window,
            channel=CHANNEL,
            obligations=(ref,),
        )
        sink.observe(observation)
        sink.constrain(MustAppearIn(write.id, observation.id, ref))
        return
    if decision.unestablished:
        sink.not_evaluated(
            ref,
            GENERATION_UNESTABLISHED,
            f"no record establishes that {', '.join(sorted(decision.unestablished_writes))} "
            f"ran in the generation of {', '.join(decision.unestablished)}",
        )
        return
    complete = [window for window in obligation.covering if window.complete]
    if complete or any(obligation.path in w.listed for w in obligation.covering):
        sink.satisfied(ref)
        return
    shown = obligation.covering[:MAX_GAP_WINDOWS]
    detail = "; ".join(f"{window.id}: {', '.join(window.gaps)}" for window in shown)
    if len(obligation.covering) > len(shown):
        detail += f" (+{len(obligation.covering) - len(shown)} more)"
    sink.not_evaluated(ref, INCOMPLETE_WINDOW, detail)


def _witnesses(obligation: _Obligation, decision: _Decision) -> tuple[Witness, ...]:
    """Name the writes, the first covering window, the window that omits the path,
    and the first incomplete windows' gaps.
    """
    found = [Witness("write-action", group.action) for group in obligation.writes]
    found.append(Witness("window-action", obligation.covering[0].id))
    if decision.window is not None:
        found.append(Witness("omitting-window", decision.window.id))
    found += [Witness("unestablished-window", w) for w in decision.unestablished]
    incomplete = [w for w in obligation.covering if not w.complete]
    for window in incomplete[:MAX_GAP_WINDOWS]:
        found += [Witness("window-gap", f"{window.id}: {gap}") for gap in window.gaps]
    return tuple(found)


def collect_journal(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every acked write against the journal windows that began after its ack."""
    windows = sorted(
        (
            _window(recorded)
            for recorded in check_input.facts.actions_of(WINDOW)
            if judged(recorded) and "since_sequence" in recorded.payload
        ),
        key=lambda window: (window.invoked, window.id),
    )
    if not windows:
        return
    generations = _generations(check_input)
    obligations: dict[tuple[str, str], _Obligation] = {}
    for group, path in _acked_writes(check_input):
        covering = tuple(w for w in windows if group.window.hi < w.invoked)
        if not covering:
            continue
        key = (covering[0].id, path)
        if key not in obligations:
            obligations[key] = _Obligation(path, covering)
        obligations[key].writes.append(group)
        if any(window.complete for window in covering):
            sink.covered(ACKED_WRITE_OBSERVED)
    for (first, path), obligation in sorted(obligations.items()):
        obligation.writes.sort(key=lambda group: group.id)
        decision = _decision(obligation, generations)
        ref = sink.obligation(
            MISSING_ACKED_WRITE,
            path_scope(first, path),
            subject=path_subject(path),
            witnesses=_witnesses(obligation, decision),
        )
        _apply(obligation, decision, ref, sink)


JOURNAL_RULES: Mapping[str, RuleDef] = {
    MISSING_ACKED_WRITE: RuleDef(
        id=MISSING_ACKED_WRITE,
        statement="A write acknowledged before a complete (not truncated, no lost "
        "changes, from sequence zero) same-generation journal window began appears "
        "in that window's changed paths (MustAppearIn).",
        subject_kind=SubjectKind.PATH,
    ),
}

JOURNAL_PROPERTY = PropertyDef(
    id=JOURNAL,
    collect=collect_journal,
    rules=JOURNAL_RULES,
    coverage=(
        CoverageObligation(
            ACKED_WRITE_OBSERVED,
            "Acknowledged writes some complete journal window began after",
        ),
    ),
    not_evaluated_reasons={
        INCOMPLETE_WINDOW: "Every window that began after the write's ack omits "
        "its path and is incomplete: truncated, lost changes, or not from sequence "
        "zero.",
        GENERATION_UNESTABLISHED: GENERATION_UNESTABLISHED_STATEMENT,
    },
    consumes=CONSUMES,
)
