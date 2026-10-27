# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `cross-channel` property: FUSE and Thrift reads that must agree, do.

Two `ok` reads of one path are *legally comparable* when no contract group
that writes the path's cell (acked or indeterminate, any action, guarded
or not) can take its point between theirs: no such group's window meets
the hull of the two read windows. Every explanation then shows both reads
the same visible value, so their bytes must be equal whatever the sync
timeout (on Linux a Thrift read has no pending writes to wait for).

The windows of the path's writers split time into gaps; a read is
comparable to another exactly when both windows lie inside the same gap,
so comparability is an equivalence and each gap's reads form a clique.
A read whose window meets a writer is comparable to nothing. The rule
compares channels, so the graph keeps the comparable pairs whose reads
are on different channels, and one spanning tree per gap is checked
instead of every pair. The root is the earliest read holding the gap's
most common value; a read on another channel hangs off the root, and a
read on the root's channel hangs off the earliest read on another
channel that holds the root's value (else the earliest read on another
channel). Equality is transitive, so the tree's edges agree exactly
when every comparable cross-channel pair does, and a gap of n reads
owns at most n - 1 obligations, each scoped by its child read. Rooting
at the most common value makes the anchor a read that disagrees with
it: one odd read among agreeing reads of both channels is the only
violated scope, whatever its position in time. Which value is legal is
the `bytes` property's question, decided by the engine.

A disagreement is the product's: FUSE answers come through kernel
caches EdenFS must invalidate and Thrift resolves the mount state
directly (one serving group), so two answers about one value with
nothing in between disagree only through a missed invalidation or a
wrong state, both EdenFS's.
"""

from __future__ import annotations

from collections import Counter
from collections.abc import Iterable, Mapping
from dataclasses import dataclass

from eden.integration.correctness.exploratory.core.effects import (
    EffectSet,
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

CROSS_CHANNEL = "cross-channel"
BYTE_DISAGREEMENT = f"{CROSS_CHANNEL}.byte-disagreement"
BYTES_COMPARED = f"{CROSS_CHANNEL}.bytes-compared"
READ = "read"
CHANNELS = ("fuse", "thrift")

CONSUMES = frozenset({(READ, "sha256")})


@dataclass(frozen=True)
class _Read:
    id: str
    path: str
    channel: str
    lo: float
    hi: float
    token: str


def _reads(facts: Iterable[ActionFacts]) -> list[_Read]:
    found = []
    for recorded in facts:
        if not judged(recorded) or recorded.payload["channel"] not in CHANNELS:
            continue
        assert recorded.result is not None
        assert recorded.invoked is not None and recorded.completed is not None
        found.append(
            _Read(
                id=recorded.id,
                path=normalize_path(recorded.payload["path"]),
                channel=recorded.payload["channel"],
                lo=recorded.invoked,
                hi=recorded.completed,
                token=recorded.result.get("sha256"),
            )
        )
    return found


def comparable_classes(effects: EffectSet, reads: list[_Read]) -> list[list[_Read]]:
    """Return the classes of legally comparable reads, each sorted by window then ID.

    Reads of one path are comparable when no group writing the path meets
    either window and none lies between them, so a class is keyed by how
    many of the path's writers end before its reads.
    """
    classes: dict[tuple[str, int], list[_Read]] = {}
    for read in reads:
        cells, span = (path_cell(read.path),), Window(read.lo, read.hi)
        if not disturbed(effects, cells, span):
            before = sum(g.window.before(span) for g in effects.writers(cells))
            classes.setdefault((read.path, before), []).append(read)
    return [
        sorted(members, key=lambda read: (read.lo, read.hi, read.id))
        for _key, members in sorted(classes.items())
    ]


def spanning_edges(members: list[_Read]) -> list[tuple[_Read, _Read]]:
    """Return a spanning tree of one class's cross-channel graph as (parent, child) edges.

    `members` are in window order. Empty when the class holds one channel only.
    """
    counts = Counter(read.token for read in members)
    most = max(counts.values())
    root = next(read for read in members if counts[read.token] == most)
    across = [read for read in members if read.channel != root.channel]
    if not across:
        return []
    other = next((read for read in across if read.token == root.token), across[0])
    return [
        (root if read.channel != root.channel else other, read)
        for read in members
        if read is not root
    ]


def collect_cross_channel(check_input: CheckInput, sink: CollectorSink) -> None:
    """Compare each comparable class's spanning-tree edges directly."""
    reads = _reads(check_input.facts.actions_of(READ))
    for members in comparable_classes(check_input.effects, reads):
        for parent, child in spanning_edges(members):
            ref = sink.obligation(
                BYTE_DISAGREEMENT,
                child.id,
                subject=path_subject(child.path),
                witnesses=(
                    Witness(f"{parent.channel}-read-action", parent.id),
                    Witness(f"{child.channel}-read-action", child.id),
                ),
            )
            sink.covered(BYTES_COMPARED)
            if parent.token == child.token:
                sink.satisfied(ref)
                continue
            sink.violated(
                ref,
                Attribution.PRODUCT,
                f"bytes/{parent.channel}≠{child.channel}: {child.path} differs "
                f"between {parent.id} and {child.id} with no writer of the path "
                "able to run between them",
            )


CROSS_CHANNEL_RULES: Mapping[str, RuleDef] = {
    BYTE_DISAGREEMENT: RuleDef(
        id=BYTE_DISAGREEMENT,
        statement="A FUSE read and a Thrift read of one path over a span no "
        "producer of the path can touch return equal bytes, whatever the sync "
        "timeout.",
        subject_kind=SubjectKind.PATH,
    ),
}

CROSS_CHANNEL_PROPERTY = PropertyDef(
    id=CROSS_CHANNEL,
    collect=collect_cross_channel,
    rules=CROSS_CHANNEL_RULES,
    coverage=(
        CoverageObligation(
            BYTES_COMPARED,
            "Spanning-tree pairs of legally comparable FUSE and Thrift reads compared",
        ),
    ),
    consumes=CONSUMES,
)
