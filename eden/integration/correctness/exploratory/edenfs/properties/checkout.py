# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `checkout` property: checkout shape as EdenFS and Sapling run a checkout.

- `checkout.parent-before-paths`: the contracts orders every per-path group
  of a checkout after its own parent group: the group requires a source
  mark only that parent group sets, and also the EdenFS parent at the
  destination when another checkout's parent move may land inside the
  window; a settled checkout, whose window nothing reads the parent in,
  gets no requirement (`contracts.checkout._order_requirement`). This collector
  declares the obligation.
- `checkout.sl-abort`: a successful `sl goto` met no blocking pre-state
  at any changed path (a contract requirement on its per-path groups), and
  each path an aborting one lists did block it (contract observations).
  An aborting update's listed paths are their own obligation
  (`contracts.checkout.abort_scope`), so the `blockers` identity role names the input
  class the engine found contradicted: `not-blocking` (a listed path does
  not block, whatever else is listed) or `unlisted` (inputs scoped by the
  action alone: a successful update's changed paths, a `--continue`'s
  interrupted-goto state).
- `checkout.conflict-count`: a Thrift checkout's recorded conflict count
  equals the weight of the paths whose per-path groups found a conflict
  state (contract markers read by one contract observation at completion).
  Every write, chmod, or unlink of a counted path is ordered by the
  engine against the path's visit. With no marker at all no legal state
  reports a conflict, so the count must be 0 (direct; nothing for the
  engine to read). Where the contracts cannot see every path that might
  conflict, a relevant path's conflict typing is not modeled, or the count
  matches too many marker sets, the obligation is not evaluated with the
  reason the contracts give.
- `checkout.in-progress-rejection`: a guard refusal is legal only as
  EdenFS words it: the interrupted advice while an interrupted checkout
  to another target stands, the in-progress text while another checkout
  held or may have held the guard (a contract observation over the
  `interrupted` cell, `contracts.checkout.rejection_observation`; this collector
  declares the obligation); the rejected attempt had no effects (its
  would-be groups are derived, each requiring the impossible under this
  obligation, so a read only they explain violates it together with the
  read's rule).
- `checkout.interrupted-refusal`: while the `interrupted` cell names a
  checkout, a status is refused and a checkout to another target does not
  get past the guard (derived: an observation on each served status, a
  requirement on each checkout's parent group, and each checkout's parent
  group before its own interruption; this collector declares the
  obligation).
"""

from __future__ import annotations

from collections.abc import Mapping

from eden.integration.correctness.exploratory.core.effects import ObligationRef
from eden.integration.correctness.exploratory.core.facts import ActionFacts, OK
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    checkout_subject,
    CollectorSink,
    CoverageObligation,
    PropertyDef,
    RuleDef,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    ABORT_BLOCKERS,
    CHECKOUT_ABORT_RULE,
    CHECKOUT_ORDER_RULE,
    CONFLICT_COUNT_RULE,
    count_judgment,
    IN_PROGRESS_RULE,
    INTERRUPTED_REFUSAL_RULE as INTERRUPTED_REFUSAL,
)

CHECKOUT = "checkout"
COUNT_UNMODELED = "checkout.conflict-count-unmodeled"
COUNT_JUDGED = "checkout.conflict-count-judged"
REJECTION_JUDGED = "checkout.rejection-judged"
CONSUMES = frozenset({("checkout", "conflicts")})
_CONTRACT_RULES = (
    CHECKOUT_ORDER_RULE,
    CHECKOUT_ABORT_RULE,
    IN_PROGRESS_RULE,
    INTERRUPTED_REFUSAL,
)


def _identity(rule_id: str, recorded: ActionFacts, part: str) -> tuple[Witness, ...]:
    if rule_id == INTERRUPTED_REFUSAL:
        return (Witness("action-kind", recorded.kind),)
    if rule_id != CHECKOUT_ABORT_RULE:
        return (Witness("via", recorded.payload["via"]),)
    outcome = "succeeded" if recorded.outcome == OK else "aborted"
    return (Witness("outcome", outcome), Witness("blockers", ABORT_BLOCKERS[part]))


def _declare_contract_obligations(check_input: CheckInput, sink: CollectorSink) -> None:
    """Declare the order, abort, and rejection obligations the contracts supports."""
    for ref in sorted(
        check_input.effects.contract_obligations(), key=ObligationRef.document
    ):
        if ref.rule_id not in _CONTRACT_RULES:
            continue
        action_id, part = ref.scope, ""
        if action_id not in check_input.facts.actions:
            action_id, part = ref.scope.rsplit(":", 1)
        recorded = check_input.facts.action(action_id)
        sink.obligation(
            ref.rule_id,
            ref.scope,
            subject=checkout_subject(action_id),
            witnesses=_identity(ref.rule_id, recorded, part),
        )
        if ref.rule_id == IN_PROGRESS_RULE:
            sink.covered(REJECTION_JUDGED)


def _judge_counts(check_input: CheckInput, sink: CollectorSink) -> None:
    """Declare one count obligation per acked Thrift checkout that recorded a count."""
    for recorded in check_input.facts.actions_of("checkout"):
        result = recorded.result
        if (
            recorded.outcome != OK
            or result is None
            or recorded.payload["via"] != "thrift"
            or type(result.get("conflicts")) is not int
        ):
            continue
        ref = sink.obligation(
            CONFLICT_COUNT_RULE,
            recorded.id,
            subject=checkout_subject(recorded.id),
            witnesses=(Witness("mode", recorded.payload["mode"]),),
        )
        if check_input.effects.supports(ref):
            sink.covered(COUNT_JUDGED)
            continue
        judgment = count_judgment(
            check_input.spec, check_input.facts, check_input.pinned, recorded.id
        )
        if judgment.gap is not None:
            sink.not_evaluated(ref, COUNT_UNMODELED, judgment.gap)
            continue
        if judgment.markers:
            sink.not_evaluated(
                ref, COUNT_UNMODELED, "the contracts supplied no conflict-count input"
            )
            continue
        count = result.get("conflicts")
        if count == 0:
            sink.satisfied(ref)
            continue
        sink.violated(
            ref,
            Attribution.PRODUCT,
            f"checkout/conflict-count: {recorded.id} reported {count} conflicts "
            "where no path can be in a conflict state",
        )


def collect_checkout(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every checkout-shape obligation of the run."""
    _declare_contract_obligations(check_input, sink)
    _judge_counts(check_input, sink)


CHECKOUT_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        RuleDef(
            id=CHECKOUT_ORDER_RULE,
            statement="A checkout moves the EdenFS parent to its destination before "
            "any of its path effects: unless the checkout is settled, every per-path "
            "group requires a source mark only its own parent group sets, and also "
            "the parent at the destination when another checkout's parent move may "
            "land inside its window.",
            subject_kind=SubjectKind.CHECKOUT,
            identity_roles=("via",),
        ),
        RuleDef(
            id=CHECKOUT_ABORT_RULE,
            statement="sl goto aborts with no effect iff a changed path holds a "
            "blocking pre-state at its DRY_RUN visit: any local change of a path "
            "the destination changes, removals included (a path absent, unlinked "
            "or removed through `sl rm` or as an `sl mv` source, while the "
            "destination changes it); dirty paths the destination leaves unchanged "
            "are carried. Each path an abort lists blocks. A `--continue` "
            "proceeds only when `.hg/updatestate` names its destination, and "
            "refuses as not interrupted only when the file is absent.",
            subject_kind=SubjectKind.CHECKOUT,
            identity_roles=("outcome", "blockers"),
        ),
        RuleDef(
            id=CONFLICT_COUNT_RULE,
            statement="A Thrift checkout's recorded conflict count equals the number "
            "of paths whose pre-visit state is a conflict ("
            "UNTRACKED_ADDED, REMOVED_MODIFIED, MISSING_REMOVED, MODIFIED_MODIFIED, "
            "MODIFIED_REMOVED; FORCE also any path the destination leaves unchanged "
            "that differs from it).",
            subject_kind=SubjectKind.CHECKOUT,
            identity_roles=("mode",),
        ),
        RuleDef(
            id=IN_PROGRESS_RULE,
            statement="A checkout-guard refusal is legal only as EdenFS words it: "
            "the interrupted advice while an interrupted checkout to another target "
            "stands, the in-progress text while another checkout held or may have "
            "held the guard; the rejected attempt has no effects.",
            subject_kind=SubjectKind.CHECKOUT,
            identity_roles=("via",),
        ),
        RuleDef(
            id=INTERRUPTED_REFUSAL,
            statement="While a checkout stands interrupted, every status and every "
            "checkout to another target is refused; a checkout is interrupted "
            "only after it got past the guard.",
            subject_kind=SubjectKind.CHECKOUT,
            identity_roles=("action-kind",),
        ),
    )
}

CHECKOUT_PROPERTY = PropertyDef(
    id=CHECKOUT,
    collect=collect_checkout,
    rules=CHECKOUT_RULES,
    coverage=(
        CoverageObligation(COUNT_JUDGED, "Thrift conflict counts the engine judged"),
        CoverageObligation(REJECTION_JUDGED, "Rejected checkouts judged"),
    ),
    not_evaluated_reasons={
        COUNT_UNMODELED: "A conflict count the contracts cannot judge: a path outside "
        "the contract set may conflict, or a relevant path's conflict typing is "
        "not modeled",
    },
    consumes=CONSUMES,
)
