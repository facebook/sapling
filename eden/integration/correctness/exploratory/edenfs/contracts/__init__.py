# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Target-owned EdenFS effect contracts and typed checkout contracts.

`make_contracts` returns the target's `contracts` hook. It reads the decoded
history and the pinned fixture and records, per action, the concrete
effect groups the action can have caused, plus one observation per
state-dependent errno for `core.input-legality`. Nothing here decides a
verdict: the consistency engine chooses which indeterminate groups
happened and where every group and observation lies in time.

The cell model and contexts are in `model`, file operations and
`sl rm`/`sl mv` in `files`, checkouts in `checkout`.
"""

from __future__ import annotations

from collections.abc import Callable, Iterable, Iterator, Mapping

from eden.integration.correctness.exploratory.core.effects import (
    Before,
    ContractSink,
    EffectGroup,
    Observation,
    PinnedFacts,
    Status,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.edenfs.actions import ACTION_DEFS

from .checkout import (
    _checkout_groups,
    _context,
    _descriptor_groups,
    _descriptor_plans,
    _directory_object_observation,
    _interruption_groups,
    _rejected_groups,
    _updatestate_groups,
    abort_observations,
    continue_observation,
    CONTRACT_RULES,
    count_inputs,
    effect_window,
    INTERRUPTED_REFUSAL_RULE,
    rejection_observation,
    resolution,
    served_observation,
    state_observation,
)
from .files import (
    _chmod_groups,
    _mkdir_groups,
    _rename_groups,
    _sl_mv_groups,
    _sl_rm_groups,
    _unlink_groups,
    _write_groups,
    DESCRIPTOR_RULE,
)
from .model import (
    _Context,
    _eden_pinned,
    _group_id,
    GETATTR_ABSENCE_RULE,
    INTERRUPTED,
    Resolution,
)


_GROUPS: Mapping[
    str, Callable[[_Context, Action, Status, Window], Iterator[EffectGroup]]
] = {
    "write": _write_groups,
    "rename": _rename_groups,
    "unlink": _unlink_groups,
    "mkdir": _mkdir_groups,
    "chmod": _chmod_groups,
    "sl_rm": _sl_rm_groups,
    "sl_mv": _sl_mv_groups,
    "checkout": _checkout_groups,
}


def action_groups(ctx: _Context, action: Action) -> tuple[EffectGroup, ...]:
    """Return every effect group one action may have caused."""
    builder = _GROUPS.get(action.kind)
    recorded = ctx.facts.action(action.id)
    status = resolution(recorded, ctx.facts)
    groups: tuple[EffectGroup, ...] = ()
    if action.kind == "checkout" and status == Resolution.NONE:
        groups = _rejected_groups(ctx, action)
    elif builder is not None and status != Resolution.NONE:
        window = effect_window(recorded, status, ctx.boundary)
        group_status = (
            Status.ACKED if status == Resolution.ACKED else Status.INDETERMINATE
        )
        groups = tuple(builder(ctx, action, group_status, window))
    if action.kind == "checkout":
        groups = (
            *groups,
            *_updatestate_groups(ctx, action),
            *_interruption_groups(ctx, action),
        )
    return groups


def record_contracts(  # noqa: C901 - one pass keeps the sink's input order
    spec: RunSpec,
    facts: HistoryFacts,
    pinned: PinnedFacts | None,
    sink: ContractSink,
    rules: frozenset[str] = CONTRACT_RULES,
) -> None:
    """Record every action's effect groups and state observations, in spec order.

    Contract inputs supporting a rule outside `rules` are left out, which
    only weakens what the engine can prove.
    """
    ctx = _context(spec, facts, _eden_pinned(spec, pinned), rules)
    plans = _descriptor_plans(ctx) if DESCRIPTOR_RULE in rules else {}
    derived = {
        action.id: _descriptor_groups(ctx, plans, action, action_groups(ctx, action))
        for action in spec.actions
    }
    every = tuple(group for groups in derived.values() for group in groups)
    for action in spec.actions:
        for group in derived[action.id]:
            sink.group(group)
        if action.kind == "checkout":
            for item in count_inputs(ctx, action, every):
                if isinstance(item, Observation):
                    sink.observe(item)
                else:
                    sink.constrain(item)
        definition = ACTION_DEFS.get(action.kind)
        observation = (
            None
            if definition is None
            else state_observation(ctx, action, definition.result)
        )
        if observation is None and action.kind == "read_fd":
            observation = _directory_object_observation(ctx, plans, action)
        if observation is not None:
            sink.observe(observation)
        if action.kind == "checkout":
            for abort in abort_observations(ctx, action):
                sink.observe(abort)
            for refusal in (
                continue_observation(ctx, action),
                rejection_observation(ctx, action),
            ):
                if refusal is not None:
                    sink.observe(refusal)
            ids = {group.id for group in derived[action.id]}
            parent, interrupted = (
                _group_id(action.id, part) for part in ("parent", INTERRUPTED)
            )
            ref = ctx.obligation(INTERRUPTED_REFUSAL_RULE, action.id)
            if ref is not None and {parent, interrupted} <= ids:
                # A checkout is interrupted only after it got past the guard.
                sink.constrain(Before(parent, interrupted, ref))
        served = served_observation(ctx, action)
        if served is not None:
            sink.observe(served)


def make_contracts(
    rules: Iterable[str] = CONTRACT_RULES,
) -> Callable[[RunSpec, HistoryFacts, PinnedFacts | None, ContractSink], None]:
    """Return a `contracts` hook whose contract inputs support only `rules`.

    A target passes the rules its properties register, so it never
    records an input for an obligation nobody declares.
    """
    enabled = frozenset(rules) & (CONTRACT_RULES | {GETATTR_ABSENCE_RULE})

    def contracts(
        spec: RunSpec,
        facts: HistoryFacts,
        pinned: PinnedFacts | None,
        sink: ContractSink,
    ) -> None:
        """Record one run's contract groups and observations into `sink`."""
        record_contracts(spec, facts, pinned, sink, enabled)

    return contracts
