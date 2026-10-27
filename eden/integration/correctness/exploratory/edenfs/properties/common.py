# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Shared EdenFS collector helpers: serving paths, scopes, pinned facts, disturbance.

Collectors select typed facts and emit observations; nothing here chooses a
producer, an order, or an outcome.

"Disturbed" has one definition, `disturbed`: a span is disturbed for a set
of cells when a contract effect group writing one of them
(`EffectSet.writers`) has a window meeting the span. Contract groups are
every way the model lets a value change: an indeterminate or interrupted
action's group spans to its resolution boundary, and a fault or kill that
leaves an effect unknown shows in that group's status and window. A
nemesis that writes no cell (a fault lifetime, a killed process, a
background read) disturbs nothing. The direct checks (cross-channel bytes,
listing names, attribute types and mtimes) compare two results only over
an undisturbed span, where every legal state holds the cells constant, so
a disagreement is impossible in every legal state
(`disturbed_parity_test` checks that against the engine). A getattr a
fault targets is not type-compared (`faulted`): the fault changes that
answer, not the state.
"""

from __future__ import annotations

import hashlib
from collections.abc import Iterable, Mapping

from eden.integration.correctness.exploratory.core.effects import (
    Cell,
    EffectSet,
    PinnedFacts,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import (
    ActionFacts,
    HistoryFacts,
    OK,
)
from eden.integration.correctness.exploratory.core.jsonio import MAX_ID_BYTES
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.edenfs.contracts.model import EdenPinned

EDENFS_GROUP = "edenfs-inode"
SAPLING_GROUP = "sapling"
# Observation channels and the serving path each reaches. FUSE and Thrift
# (reads, status, journal) are one group: FUSE answers come through kernel
# page, attribute, and directory caches EdenFS must invalidate, Thrift
# resolves the mount state directly, and both are EdenFS's, so a stale
# value both report is no evidence against the model. sl status and sl diff are two outputs of one Sapling path; derived
# state observations of a Sapling command use `sl`.
SERVING_PATHS: Mapping[str, str] = {
    "fuse": EDENFS_GROUP,
    "thrift": EDENFS_GROUP,
    "journal": EDENFS_GROUP,
    "sl": SAPLING_GROUP,
    "sl-status": SAPLING_GROUP,
    "sl-diff": SAPLING_GROUP,
}


def eden_pinned(spec: RunSpec, pinned: PinnedFacts | None) -> EdenPinned:
    """Return the run's EdenFS pinned facts, building them from the spec when absent."""
    if isinstance(pinned, EdenPinned):
        return pinned
    if pinned is not None:
        raise TypeError(
            f"EdenFS collectors need EdenPinned, got {type(pinned).__name__}"
        )
    return EdenPinned.for_spec(spec)


def path_scope(action_id: str, path: str) -> str:
    """Return the obligation scope of one action's judgment of one path.

    It is `<action>:<path>`, or `<action>:p<sha256 prefix>` when that would
    exceed the scope bound.
    """
    scope = f"{action_id}:{path}"
    if len(scope.encode("utf-8", errors="surrogatepass")) <= MAX_ID_BYTES:
        return scope
    digest = hashlib.sha256(path.encode("utf-8", errors="surrogatepass")).hexdigest()
    return f"{action_id}:p{digest[:32]}"


def judged(recorded: ActionFacts) -> bool:
    """Return whether an action is an `ok`, decoded, completed primary-side result."""
    return (
        recorded.outcome == OK
        and recorded.result is not None
        and recorded.invoked is not None
        and recorded.completed is not None
    )


def disturbed(effects: EffectSet, cells: Iterable[Cell], span: Window) -> bool:
    """Return whether a group writing one of `cells` has a window meeting `span`."""
    return any(group.window.meets(span) for group in effects.writers(cells))


def faulted(facts: HistoryFacts, recorded: ActionFacts) -> bool:
    """Return whether an installed fault targets the action while it ran."""
    return any(
        install.payload.get("target_action") == recorded.id
        and Window(lo, hi).meets(recorded.window)
        for install, (lo, hi) in facts.fault_windows()
    )
