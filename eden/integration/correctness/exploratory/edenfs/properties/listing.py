# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `listing` property: one-shot listings of one directory agree across channels.

FUSE listings come through the kernel's directory cache, which EdenFS
must invalidate, and the readdir RPC resolves the mount state directly;
a disagreement is EdenFS's (a missed invalidation or a wrong state), so
two `ok` listings of one directory over FUSE and Thrift report equal
name sets when nothing can change the directory's entries over their
joint span: the span is not `disturbed` for the directory's own cell or
the cell of any name either listing holds (no fault site targets a
listing). A pair something can disturb is not comparable, so it is not
an obligation; whether the listings are right is the readdir rules'
question (`edenfs.properties.readdir`).

One obligation per listing that has an earlier comparable listing on
the other channel, scoped by the later one, compared with each such
earlier listing. The comparison is direct: with nothing able to change
the directory's entries over the span, two listings that differ are
impossible in every legal state, and no engine input observes a listing,
so it duplicates no engine decision. A disagreement is the product's.
"""

from __future__ import annotations

from collections.abc import Mapping
from pathlib import PurePosixPath

from eden.integration.correctness.exploratory.core.effects import (
    Cell,
    normalize_path,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import ActionFacts
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
from eden.integration.correctness.exploratory.edenfs.contracts.model import path_cell
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    disturbed,
    judged,
)

LISTING = "listing"
CHANNEL_DISAGREEMENT = f"{LISTING}.channel-disagreement"
COMPARED = "listing.cross-channel"
CONSUMES = frozenset({("readdir", "entries")})


def _entry_cells(directory: str, *listings: frozenset[str]) -> list[Cell]:
    """Return the directory's own cell and the cell of every name either listing holds."""
    names = sorted(set().union(*listings))
    return [
        path_cell(directory),
        *(path_cell(str(PurePosixPath(directory, n))) for n in names),
    ]


def _span(first: ActionFacts, second: ActionFacts) -> Window:
    assert first.invoked is not None and second.invoked is not None
    assert first.completed is not None and second.completed is not None
    return Window(
        min(first.invoked, second.invoked), max(first.completed, second.completed)
    )


def _names(recorded: ActionFacts) -> frozenset[str]:
    assert recorded.result is not None
    return frozenset(recorded.result.get("entries"))


def collect_listing(check_input: CheckInput, sink: CollectorSink) -> None:
    """Compare every later listing with each earlier comparable one on the other channel."""
    listings = [
        recorded
        for recorded in check_input.facts.actions_of("readdir")
        if judged(recorded)
    ]
    for index, later in enumerate(listings):
        directory = normalize_path(later.payload["path"])
        partners = [
            earlier
            for earlier in listings[:index]
            if normalize_path(earlier.payload["path"]) == directory
            and earlier.payload["channel"] != later.payload["channel"]
            and not disturbed(
                check_input.effects,
                _entry_cells(directory, _names(earlier), _names(later)),
                _span(earlier, later),
            )
        ]
        if not partners:
            continue
        ref = sink.obligation(
            CHANNEL_DISAGREEMENT, later.id, subject=path_subject(directory)
        )
        sink.covered(COMPARED, len(partners))
        mine = _names(later)
        for earlier in partners:
            theirs = _names(earlier)
            if mine == theirs:
                continue
            sink.violated(
                ref,
                Attribution.PRODUCT,
                f"readdir/{earlier.payload['channel']}≠{later.payload['channel']}: "
                f"{directory} differs between {earlier.id} and {later.id}: only "
                f"{earlier.id} {sorted(theirs - mine)}, only {later.id} "
                f"{sorted(mine - theirs)}",
                (Witness("other-listing-action", earlier.id),),
            )
            break
        else:
            sink.satisfied(ref)


LISTING_RULES: Mapping[str, RuleDef] = {
    CHANNEL_DISAGREEMENT: RuleDef(
        id=CHANNEL_DISAGREEMENT,
        statement="Two listings of one directory over FUSE and Thrift report equal "
        "name sets when no effect group writing the directory or a name either "
        "lists has a window meeting their joint span.",
        subject_kind=SubjectKind.PATH,
    ),
}

LISTING_PROPERTY = PropertyDef(
    id=LISTING,
    collect=collect_listing,
    rules=LISTING_RULES,
    coverage=(CoverageObligation(COMPARED, "FUSE and Thrift listing pairs compared"),),
    consumes=CONSUMES,
)
