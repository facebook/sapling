# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `readdir` property: the names a directory listing lists.

A listing is a one-shot `readdir` (FUSE or Thrift) or a drained stream
(`readdir_stream`, then its `readdir_resume`), which reads one directory
over a span that may cross checkouts and namespace operations. Nothing
here assumes the listing is a snapshot: an entry may have been read at
any point inside the listing's span (a one-shot listing's call window;
a stream's `[stream invoked, resume completed]`). Both kinds are judged
by the same rules over the same domain.

- `readdir.duplicate-entry`: a listing naming one name more than once
  is a direct violation (`undetermined`: one serving path), unless the
  directory can be rebound or the name repeats at most once per contract
  group whose window meets the listing's span and that can replace the
  file the name binds (remove it and bind another): POSIX leaves such
  entries unspecified, and EdenFS resumes a listing by inode number, so
  a replaced entry, which gets a new higher inode, may be listed under
  its old and its new inode. A write keeps the inode, and a checkout of
  either mode leaves an entry the two trees agree on alone
  (`canShortCircuitCheckout`).
- `readdir.unknown-entry`: every listed name was bound at some point of
  the span. Violated when nothing can bind the name: no start value, no
  revision the parent can hold, and no contract group binds it (an
  untracked name in a directory nothing rebinds: the fixture alone), or
  every explanation leaves a relevant path absent throughout the span.
- `readdir.missing-entry`: every name bound throughout the span is
  listed. The listing's authoritative domain is the directory's entries:
  an untracked wide-directory entry (its name never changes and every
  revision holds it) in a directory nothing rebinds is required, and so
  is a relevant entry every explanation leaves bound throughout the span
  (bound from the start and only ever rebound to another file or
  directory, or one value throughout). A stream need not list a name a
  move replaced while it ran (the move invoked before the stream's last
  request ended, and not completed before it began): EdenFS resumes a
  listing at an inode-number offset (`TreeInode::readdirImpl`) and a rename
  keeps the source's inode, which POSIX allows for a name removed and
  re-added between calls.

The entries may have been read anywhere in the span (a reader buffers
several entries per getdents call). The membership rules decide directly
only what no legal state can contradict: a path bound at the start that
no contract effect unbinds, a path nothing ever binds, and the fixture's
names in a directory nothing rebinds; a randomized parity test checks
that judgment against the engine. Every other relevant path's presence
is the engine's: for a one-shot listing, one observation per such path,
inside the call's window, that it was bound (listed) or unbound
(unlisted) at some point, under the rule's obligation. A stream's span
covers two actions and the engine places an observation inside one
action's window, so a stream's path is a query over the whole span asked
of `solver.satisfiable` with the contract inputs: a listed path no
explanation binds at any point is unknown, an unlisted one no
explanation unbinds is missing, and a path no explanation shows
otherwise than listed at any point passes. A path both queries admit, a
query the work does not decide, or contract inputs no explanation
satisfies leave the obligation not evaluated
(`readdir.membership-undecided`) when nothing else violates it.

Direct and stream-query violations are `undetermined`: each listing has
one serving path. The engine attributes the one-shot observations it
violates.
"""

from __future__ import annotations

import functools
import hashlib
from collections import Counter
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass, replace
from pathlib import PurePosixPath

from eden.integration.correctness.exploratory.core import semantics, solver
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    EffectKind,
    EffectSet,
    eq,
    Expect,
    none_of,
    normalize_path,
    ObligationRef,
    Observation,
    Window,
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
    WorkAccount,
)
from eden.integration.correctness.exploratory.edenfs.actions import MOVE_KINDS
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    checkout_revisions,
    relevant_paths,
    resolution,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    DIR,
    EdenPinned,
    path_cell,
    Resolution,
)
from eden.integration.correctness.exploratory.edenfs.fixture import DIRECTORY
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    eden_pinned,
    judged,
)

READDIR = "readdir"
ONE_SHOT = "readdir"
DUPLICATE_ENTRY = f"{READDIR}.duplicate-entry"
UNKNOWN_ENTRY = f"{READDIR}.unknown-entry"
MISSING_ENTRY = f"{READDIR}.missing-entry"
DRAINED = "readdir.stream-drained"
UNDECIDED = "readdir.membership-undecided"
CONSUMES = frozenset({("readdir_resume", "entries"), (ONE_SHOT, "entries")})


def _parent(path: str) -> str:
    return normalize_path(str(PurePosixPath(path or ".").parent))


def _child(directory: str, name: str) -> str | None:
    """Return the canonical path of one listed name, or None when it is not one segment."""
    if not name or "/" in name or name in (".", ".."):
        return None
    return normalize_path(f"{directory}/{name}" if directory else name)


class Domain:
    """What can bind entries of one directory, from pinned facts and contract groups."""

    def __init__(self, check_input: CheckInput, directory: str) -> None:
        spec = check_input.spec
        self.spec = spec
        self.pinned: EdenPinned = eden_pinned(spec, check_input.pinned)
        self.effects: EffectSet = check_input.effects
        self.directory = directory
        self.revisions = checkout_revisions(spec, self.pinned)
        self.relevant = frozenset(relevant_paths(spec, self.pinned))
        self.rebound = self._rebound()

    def _rebound(self) -> bool:
        """Return whether some contract group can rebind the directory or an ancestor.

        A group that only (re)produces a directory there keeps the entries.
        """
        above = {self.directory}
        path = self.directory
        while path:
            path = _parent(path)
            above.add(path)
        cells = {path_cell(item) for item in above if item}
        if any(
            cell in cells and (effect.cell != cell or effect.value != DIR)
            for group in self.effects.groups
            for effect in group.effects
            for cell in effect.writes()
        ):
            return True
        return any(
            self.pinned.committed(self.directory, revision).cls != DIRECTORY
            for revision in self.revisions
        )

    def fixture_binds(self, path: str) -> bool:
        """Return whether the start or some revision the parent can hold binds `path`."""
        cell = path_cell(path)
        if self.pinned.initial(cell) != ABSENT:
            return True
        return any(
            self.pinned.committed(path, revision) != ABSENT
            for revision in self.revisions
        )

    def may_bind(self, path: str) -> bool:
        """Return whether any explanation binds `path` at some point."""
        if self.fixture_binds(path):
            return True
        cell = path_cell(path)
        return any(
            effect.cell == cell
            and (effect.kind == EffectKind.TRANSFER or effect.value != ABSENT)
            for group in self.effects.groups
            for effect in group.effects
        )

    def never_unbound(self, path: str) -> bool:
        """Return whether `path` is bound at the start and no contract effect can unbind it."""
        cell = path_cell(path)
        if self.pinned.initial(cell) == ABSENT:
            return False
        return not any(
            cell in effect.writes()
            and (effect.kind == EffectKind.TRANSFER or effect.value == ABSENT)
            for group in self.effects.groups
            for effect in group.effects
        )

    def required(self) -> list[str]:
        """Return the untracked entries bound throughout: the wide directory's names.

        Their names never change and every revision holds them, so only a
        relevant path (an action names it) or a rebound directory can lose
        one; anything else is not decided here.
        """
        layout = self.pinned.fixture.layout
        wide = layout.wide_dir
        if wide is None or normalize_path(wide.name) != self.directory or self.rebound:
            return []
        return [
            item.path
            for item in layout.wide_files()
            if normalize_path(item.path) not in self.relevant
        ]


# Action kinds that rewrite a bound file in place, keeping its inode.
_IN_PLACE_KINDS = frozenset({"write", "open", "chmod"})


def _replacements(domain: Domain, path: str, span: Window) -> int:
    """Return how many contract groups whose window meets `span` can replace
    the file `path` binds (remove it and bind another inode).

    A write keeps the file. A checkout replaces an entry only where the
    entry differs from the destination, in either mode (`CheckoutAction`
    and `TreeInode::checkout` leave an entry the two trees agree on, and
    that is not locally changed, alone: `canShortCircuitCheckout`), so a
    checkout group counts when the revisions disagree on the entry or
    another group writes it; any other group writing the path (an unlink,
    a move from or onto it, `sl rm`) counts. Contract groups are derived
    only for relevant paths (`relevant_paths`), so an entry no group
    writes counts each checkout meeting `span` once when the revisions
    disagree on it.
    """
    cell = path_cell(path)
    kinds = {action.id: action.kind for action in domain.spec.actions}
    writers = domain.effects.writers((cell,))
    values = {domain.pinned.initial(cell)}
    values.update(domain.pinned.committed(path, r) for r in domain.revisions)
    if not writers:
        checkouts = {
            group.action: group.window
            for group in domain.effects.groups
            if kinds[group.action] == "checkout"
        }
        return sum(
            len(values) > 1 and window.meets(span) for window in checkouts.values()
        )
    changes = len(values) > 1 or any(
        kinds[group.action] != "checkout" for group in writers
    )
    return sum(
        group.window.meets(span)
        and kinds[group.action] not in _IN_PLACE_KINDS
        and (changes or kinds[group.action] != "checkout")
        for group in writers
    )


def repeated_names(domain: Domain, entries: list[str], span: Window) -> list[str]:
    """Return the names a listing over `span` repeats more often than
    `readdir.duplicate-entry` allows, in listing order."""
    counts = Counter(entries)
    return [
        name
        for name in dict.fromkeys(entries)
        if counts[name] > 1
        and not domain.rebound
        and (
            (child := _child(domain.directory, name)) is None
            or counts[name] > 1 + _replacements(domain, child, span)
        )
    ]


def _judge_duplicates(
    domain: Domain,
    entries: list[str],
    span: Window,
    channel: str,
    ref: ObligationRef,
    sink: CollectorSink,
) -> None:
    counts = Counter(entries)
    repeated = repeated_names(domain, entries, span)
    if not repeated:
        sink.satisfied(ref)
        return
    sink.violated(
        ref,
        Attribution.UNDETERMINED,
        f"readdir/{channel}: duplicate-entry: the listing names {repeated[0]!r} "
        f"{counts[repeated[0]]} times",
        (Witness("entry", repeated[0]), Witness("count", str(len(repeated)))),
    )


def _bound(domain: Domain, path: str) -> bool | None:
    """Return whether a relevant path is bound throughout the run in every explanation.

    True when bound throughout, False when no explanation binds it at any
    point, None when neither is decided without the engine.
    """
    if domain.never_unbound(path):
        return True
    if not domain.may_bind(path):
        return False
    return None


@dataclass(frozen=True)
class Membership:
    """What the domain decides about one listing's names.

    `unknown` are listed names nothing binds at any point of the span,
    `missing` the paths bound throughout it that the listing omits;
    `undecided_listed` and `undecided_unlisted` are the relevant paths
    whose presence is not decided, listed or not.
    """

    unknown: tuple[str, ...]
    missing: tuple[str, ...]
    undecided_listed: tuple[str, ...]
    undecided_unlisted: tuple[str, ...]


def membership(
    domain: Domain,
    entries: Iterable[str],
    agrees: Callable[[str, bool], bool | None] | None = None,
) -> Membership:
    """Judge one listing of `domain.directory`.

    `agrees(path, listed)`, when given, decides a relevant path the
    domain leaves undecided: whether some explanation agrees with the
    listing (binds a listed path, unbinds an unlisted one) at some point
    of its span, or None when that is not decided.
    """
    names = list(dict.fromkeys(entries))
    listed = {path for name in names if (path := _child(domain.directory, name))}
    bound = {
        path: _bound(domain, path)
        for path in sorted(domain.relevant)
        if _parent(path) == domain.directory
    }
    if agrees is not None:
        for path, held in bound.items():
            if held is None and (agree := agrees(path, path in listed)) is not None:
                # A listed path no explanation binds, or an unlisted one no
                # explanation unbinds, is judged as one bound throughout or never.
                bound[path] = agree == (path in listed)
    unknown: list[str] = []
    for name in names:
        path = _child(domain.directory, name)
        if path is None:
            unknown.append(name)
        elif path in domain.relevant:
            if bound[path] is False:
                unknown.append(name)
        elif not domain.rebound and not domain.fixture_binds(path):
            unknown.append(name)
    missing = [path for path in domain.required() if path not in listed]
    missing += [path for path, held in bound.items() if held and path not in listed]
    undecided = [path for path, held in bound.items() if held is None]
    return Membership(
        unknown=tuple(unknown),
        missing=tuple(missing),
        undecided_listed=tuple(path for path in undecided if path in listed),
        undecided_unlisted=tuple(path for path in undecided if path not in listed),
    )


def undecided_observations(
    action: str,
    window: Window,
    channel: str,
    found: Membership,
    unknown: tuple[ObligationRef, ...],
    missing: tuple[ObligationRef, ...],
) -> list[Observation]:
    """Return a one-shot listing's observations of its undecided paths.

    Each listed one was bound, and each unlisted one unbound, at some
    point of the call (supporting `unknown` and `missing` respectively).
    """
    claims = [(path, True, unknown) for path in found.undecided_listed] + [
        (path, False, missing) for path in found.undecided_unlisted
    ]
    return [
        _presence(action, window, channel, path, listed, refs)
        for path, listed, refs in claims
        if refs
    ]


def _presence(
    action: str,
    window: Window,
    channel: str,
    path: str,
    listed: bool,
    refs: tuple[ObligationRef, ...],
) -> Observation:
    """Return the observation that `path` was bound (listed) or unbound at some point of `window`."""
    digest = hashlib.sha256(path.encode("utf-8", errors="surrogatepass"))
    part = "listed" if listed else "unlisted"
    return Observation(
        id=f"o:{action}:{part}:p{digest.hexdigest()[:16]}",
        action=action,
        cells=(path_cell(path),),
        expect=Expect(((none_of({ABSENT}) if listed else eq(ABSENT),),)),
        window=window,
        channel=channel,
        obligations=refs,
    )


def _undecided_detail(paths: tuple[str, ...], channel: str) -> str:
    return (
        f"readdir/{channel}: the engine did not decide the presence of "
        f"{paths[0]!r} over the span ({len(paths)} such paths)"
    )


def _judge_unknown(
    directory: str,
    judged_names: Membership,
    channel: str,
    one_shot: bool,
    ref: ObligationRef,
    sink: CollectorSink,
) -> None:
    unknown = judged_names.unknown
    if unknown:
        sink.violated(
            ref,
            Attribution.UNDETERMINED,
            f"readdir/{channel}: unknown-entry: the listing names {unknown[0]!r}, "
            f"which nothing binds in {directory or '.'} over its span",
            (Witness("entry", unknown[0]), Witness("count", str(len(unknown)))),
        )
    elif not judged_names.undecided_listed:
        sink.satisfied(ref)
    elif not one_shot:
        sink.not_evaluated(
            ref, UNDECIDED, _undecided_detail(judged_names.undecided_listed, channel)
        )


def _judge_missing(
    judged_names: Membership,
    channel: str,
    one_shot: bool,
    ref: ObligationRef,
    sink: CollectorSink,
) -> None:
    missing = judged_names.missing
    if missing:
        sink.violated(
            ref,
            Attribution.UNDETERMINED,
            f"readdir/{channel}: missing-entry: the listing omits {missing[0]!r}, "
            "which the directory binds throughout its span",
            (
                Witness("entry", PurePosixPath(missing[0]).name),
                Witness("count", str(len(missing))),
            ),
        )
    elif not judged_names.undecided_unlisted:
        sink.satisfied(ref)
    elif not one_shot:
        sink.not_evaluated(
            ref,
            UNDECIDED,
            _undecided_detail(judged_names.undecided_unlisted, channel),
        )


@dataclass(frozen=True)
class _Listing:
    """One completed listing: the action owning its obligations and what it read."""

    action: str
    directory: str
    channel: str
    window: Window
    entries: list[str]
    one_shot: bool


def _listings(check_input: CheckInput) -> list[_Listing]:
    """Return every `ok` one-shot listing and every drained stream, by action ID."""
    facts = check_input.facts
    found: list[_Listing] = []
    for recorded in facts.actions_of(ONE_SHOT):
        if not judged(recorded):
            continue
        assert recorded.result is not None
        assert recorded.invoked is not None and recorded.completed is not None
        found.append(
            _Listing(
                recorded.id,
                normalize_path(recorded.payload["path"]),
                recorded.payload["channel"],
                Window(recorded.invoked, recorded.completed),
                [str(name) for name in recorded.result.get("entries")],
                True,
            )
        )
    for resume in facts.actions_of("readdir_resume"):
        stream = facts.action(resume.payload["stream"])
        if not judged(resume) or not judged(stream):
            continue
        assert resume.result is not None
        assert stream.invoked is not None and resume.completed is not None
        found.append(
            _Listing(
                resume.id,
                normalize_path(stream.payload["path"]),
                "fuse",
                Window(stream.invoked, resume.completed),
                [str(name) for name in resume.result.get("entries")],
                False,
            )
        )
    return sorted(found, key=lambda listing: listing.action)


def _stream_agrees(
    effects: EffectSet,
    listing: _Listing,
    refs: tuple[ObligationRef, ObligationRef],
    account: WorkAccount | None,
) -> Callable[[str, bool], bool | None]:
    """Return a stream's `membership` decider: engine queries of a path over the span.

    A query observes the path bound (under `unknown-entry`) or unbound
    (under `missing-entry`) at some point of the span, with every derived
    input. The listing disagrees when no explanation satisfies the query
    of its own claim, and agrees when none satisfies the opposite one
    (the claim then holds throughout). Contract inputs no explanation
    satisfies, or work that runs out (per query, or the check's `account`),
    decide nothing.
    """
    base = semantics.Problem(
        effects.groups, effects.observations, effects.constraints, effects.initial
    )

    @functools.cache
    def consistent() -> bool:
        verdict, _used = solver.satisfiable(
            base, work=solver.DECISION_WORK, account=account
        )
        return verdict == "sat"

    def satisfiable(path: str, bound: bool) -> str:
        query = _presence(
            listing.action,
            listing.window,
            listing.channel,
            path,
            bound,
            (refs[0] if bound else refs[1],),
        )
        problem = replace(base, observations=(*base.observations, query))
        return solver.satisfiable(problem, work=solver.DECISION_WORK, account=account)[
            0
        ]

    def agrees(path: str, listed: bool) -> bool | None:
        if not consistent():
            return None
        if satisfiable(path, listed) == "unsat":
            return False
        if satisfiable(path, not listed) == "unsat":
            return True
        return None

    return agrees


def collect_readdir(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every listing: duplicates, then unknown and missing entries."""
    for listing in _listings(check_input):
        subject = path_subject(listing.directory)
        if not listing.one_shot:
            sink.covered(DRAINED)
        domain = Domain(check_input, listing.directory)
        _judge_duplicates(
            domain,
            listing.entries,
            listing.window,
            listing.channel,
            sink.obligation(DUPLICATE_ENTRY, listing.action, subject=subject),
            sink,
        )
        unknown = sink.obligation(UNKNOWN_ENTRY, listing.action, subject=subject)
        missing = sink.obligation(MISSING_ENTRY, listing.action, subject=subject)
        found = membership(
            domain,
            listing.entries,
            (
                None
                if listing.one_shot
                else _stream_agrees(
                    check_input.effects, listing, (unknown, missing), check_input.work
                )
            ),
        )
        if not listing.one_shot:
            # A multi-request listing may skip a name a move replaced between
            # requests: EdenFS resumes by inode number and the name now carries
            # the source's (POSIX: removed and re-added during the stream).
            # A move whose result says it had no effect excuses nothing.
            moved_onto = {
                normalize_path(move.payload["destination"])
                for kind in MOVE_KINDS
                for move in check_input.facts.actions_of(kind)
                if move.invoked is not None
                and move.invoked < listing.window.hi
                and (move.completed is None or move.completed > listing.window.lo)
                and resolution(move, check_input.facts) != Resolution.NONE
            }
            found = replace(
                found,
                missing=tuple(p for p in found.missing if p not in moved_onto),
            )
        _judge_unknown(
            listing.directory, found, listing.channel, listing.one_shot, unknown, sink
        )
        _judge_missing(found, listing.channel, listing.one_shot, missing, sink)
        if listing.one_shot:
            for observation in undecided_observations(
                listing.action,
                listing.window,
                listing.channel,
                found,
                () if found.unknown else (unknown,),
                () if found.missing else (missing,),
            ):
                sink.observe(observation)


READDIR_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        RuleDef(
            id=DUPLICATE_ENTRY,
            statement="A directory listing (one-shot or a drained stream) repeats "
            "a name at most once per contract group meeting its span that can "
            "replace the file the name binds.",
            subject_kind=SubjectKind.PATH,
        ),
        RuleDef(
            id=UNKNOWN_ENTRY,
            statement="A directory listing (one-shot or a drained stream) names only "
            "names something binds in the directory at some point of its span.",
            subject_kind=SubjectKind.PATH,
        ),
        RuleDef(
            id=MISSING_ENTRY,
            statement="A directory listing (one-shot or a drained stream) names every "
            "name the directory binds throughout its span.",
            subject_kind=SubjectKind.PATH,
        ),
    )
}

READDIR_PROPERTY = PropertyDef(
    id=READDIR,
    collect=collect_readdir,
    rules=READDIR_RULES,
    coverage=(CoverageObligation(DRAINED, "Drained directory streams"),),
    not_evaluated_reasons={
        UNDECIDED: "No violation, and the engine did not decide a relevant "
        "path's presence over a stream's span within its work",
    },
    consumes=CONSUMES,
)
