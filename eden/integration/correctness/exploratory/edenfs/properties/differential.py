# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `differential` property: each shared action against its reference side.

Only RunSpecs the differential validator admits pair actions
(`setup_profile.reference_clone`, shared kinds and channels, and every
pair with conflicting footprints ordered by a finished edge). The EdenFS
side is judged by every other property as usual;
here the two sides of each paired action are compared directly after
normalization (`core.differential.observe`). There is no per-side
legality model: a reference side that fails or reads something else is evidence about the reference, never a
product finding by itself.

- `differential.outcome-disagreement` (by-path actions): both sides end
  `ok`, or fail with the same errno (else the same return code).
  A disagreement whose reference side succeeded is `product`. One whose
  reference side failed is `reference` once the EdenFS side's outcome is
  legal: an EdenFS read or `read_fd` that served bytes is judged as a
  value disagreement (below), any other EdenFS `ok` input is legal,
  and an EdenFS failure is judged by `core.input-legality`.
- `differential.value-disagreement` (by-path actions with compared
  fields, both sides `ok`): equal normalized compared fields. A
  disagreement is attributed only once the EdenFS side's own legality is
  known: `product` when its read or `read_fd` bytes are illegal (the
  direct byte rules reject them, or the engine violates the observation
  that the bytes or fd property files for them, here filed under this
  obligation too), `reference` when that observation holds, not evaluated
  when the engine cannot decide or a descriptor gap reaches the read. A
  getattr is judged the same way through the attributes property's type,
  mode, and size observations; a one-shot readdir through the readdir
  property's membership judgment over the listing's window (an unknown or
  missing entry is the product's; the paths it leaves undecided are judged
  by the engine through the readdir property's observations; else the
  reference's).
- `differential.checkout-disagreement` (sl status, sl goto): the same
  outcome and compared fields; Sapling runs on both sides, so a
  disagreement is `undetermined` (`reference` when the reference side
  failed and the EdenFS side did not fail the same way).

A pair is compared only when both sides completed (`ok` or `fail`) with
decoded results. A reference side that did not (it was killed, timed out,
or recorded nothing decodable) while the EdenFS side did is a `reference`
finding on the pair's outcome obligation, never a silent clean, and so
is a paired sl status or sl goto whose compared fields cannot be read;
an unresolved or malformed EdenFS side is the result-schema property's
or nobody's to judge. Either leaves the footprint diverged. Once
a mutation's two sides disagree or one is unresolved, later pairs whose
footprints conflict with it are not comparable (not obligations), and a
descriptor whose `open` diverged leaves its reads and close uncompared.
"""

from __future__ import annotations

import errno
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

from eden.integration.correctness.exploratory.core.differential import SideResult
from eden.integration.correctness.exploratory.core.differential_schedule import (
    MalformedObservation,
    observe,
)
from eden.integration.correctness.exploratory.core.effects import (
    Footprint,
    footprints_interact,
    normalize_path,
    ObligationRef,
    Observation,
    ROOT,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import ActionFacts, FAIL, OK
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    checkout_subject,
    CollectorSink,
    CoverageObligation,
    path_subject,
    PropertyDef,
    RuleDef,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.core.target import Role
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    differential_footprint,
    DIFFERENTIAL_OBSERVATIONS,
    expects_reference,
    reference_clone,
)
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    descriptor_plans,
    DescriptorPlan,
)
from eden.integration.correctness.exploratory.edenfs.properties import (
    attributes as attributes_property,
    bytes as bytes_property,
    fd as fd_property,
    readdir as readdir_property,
)
from eden.integration.correctness.exploratory.edenfs.properties.bytes import Candidates
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    eden_pinned,
)

DIFFERENTIAL = "differential"
PRIMARY_UNDECIDED = "differential.primary-undecided"
# The EdenFS side's value fails a direct rule.
_ILLEGAL = object()
# The EdenFS side's listing passes the direct membership rules.
_LEGAL = object()
OUTCOME_DISAGREEMENT = f"{DIFFERENTIAL}.outcome-disagreement"
VALUE_DISAGREEMENT = f"{DIFFERENTIAL}.value-disagreement"
CHECKOUT_DISAGREEMENT = f"{DIFFERENTIAL}.checkout-disagreement"
COMPARED = "differential.pairs-compared"
CHECKOUT_WIDE = frozenset({"status", "checkout"})
DESCRIPTOR_KINDS = frozenset({"read_fd", "close"})
_MUTATING = frozenset({Role.PRODUCE, Role.REMOVE, Role.TRANSFER})
# A side's compared keys, and the errno or return code its failure is
# observed by (`outcome_label`).
CONSUMES = frozenset(
    (kind, name)
    for kind, definition in DIFFERENTIAL_OBSERVATIONS.items()
    for name in definition.keys
) | frozenset(
    (kind, spec.name)
    for kind in DIFFERENTIAL_OBSERVATIONS
    for spec in ACTION_DEFS[kind].result.fail
    if spec.name in ("errno", "returncode")
)


def plain(value: Any) -> Any:
    """Return decoded (read-only) JSON as plain lists and dicts."""
    if isinstance(value, Mapping):
        return {key: plain(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [plain(item) for item in value]
    return value


def _side(recorded: ActionFacts) -> SideResult:
    assert recorded.result is not None
    assert recorded.invoked is not None and recorded.completed is not None
    return SideResult(
        str(recorded.outcome),
        plain(recorded.result.fields),
        recorded.invoked,
        recorded.completed,
    )


def outcome_label(recorded: ActionFacts) -> str:
    """Name a side's outcome: `ok`, its errno, else a failed process's exit, else `fail`."""
    assert recorded.result is not None
    if recorded.outcome == OK:
        return "ok"
    code = recorded.result.errno
    if code is not None:
        return errno.errorcode.get(code, str(code))
    returncode = recorded.result.get("returncode")
    return f"exit {returncode}" if type(returncode) is int else "fail"


def _comparable_side(recorded: ActionFacts | None) -> bool:
    return (
        recorded is not None
        and recorded.outcome in (OK, FAIL)
        and recorded.result is not None
        and not recorded.problems
        and recorded.invoked is not None
        and recorded.completed is not None
    )


@dataclass(frozen=True)
class _Pair:
    action: Action
    primary: ActionFacts
    reference: ActionFacts


def _path_subject(spec: RunSpec, action: Action) -> Subject | None:
    """Return the PATH subject of a by-path action, or None for a checkout-wide one.

    It is the subject `core.input-legality` keys the action by: its declared
    input subject, else its footprint's single path.
    """
    if action.kind in CHECKOUT_WIDE:
        return None
    definition = ACTION_DEFS[action.kind]
    if definition.input_subject is not None:
        return path_subject(definition.input_subject(spec, action))
    paths = sorted(definition.footprint(spec, action).paths())
    return path_subject(ROOT if ROOT in paths or not paths else paths[0])


def _value(pair: _Pair, side: ActionFacts) -> str:
    observed = observe(_side(side), DIFFERENTIAL_OBSERVATIONS[pair.action.kind])
    assert observed is not None
    return observed.value


class _Comparer:
    """Compares pairs in invocation order while diverged footprints accumulate."""

    def __init__(self, check_input: CheckInput, sink: CollectorSink) -> None:
        self.check_input = check_input
        self.spec = check_input.spec
        self.facts = check_input.facts
        self.sink = sink
        self.diverged: list[tuple[float, Footprint]] = []
        self.handles: set[str] = set()
        pinned = eden_pinned(self.spec, check_input.pinned)
        self.candidates = Candidates(self.spec, pinned)
        self.plans: Mapping[str, DescriptorPlan] = descriptor_plans(
            self.spec, self.facts, pinned
        )

    def _mutates(self, action: Action) -> bool:
        definition = ACTION_DEFS[action.kind]
        return bool(definition.roles & _MUTATING) or action.kind == "checkout"

    def _diverge(self, action: Action, recorded: ActionFacts) -> None:
        if action.kind == "open":
            self.handles.add(action.id)
        if not self._mutates(action):
            return
        ended = recorded.completed if recorded.completed is not None else float("inf")
        reference = recorded.reference
        if reference is not None and reference.completed is not None:
            ended = max(ended, reference.completed)
        self.diverged.append((ended, differential_footprint(self.spec, action)))

    def _comparable(self, action: Action, recorded: ActionFacts) -> bool:
        if action.kind in DESCRIPTOR_KINDS and action.payload["fd"] in self.handles:
            return False
        assert recorded.invoked is not None
        footprint = differential_footprint(self.spec, action)
        return not any(
            ended < recorded.invoked and footprints_interact(other, footprint)
            for ended, other in self.diverged
        )

    def judge(self, action: Action) -> None:
        recorded = self.facts.action(action.id)
        if recorded.invoked is None:
            return
        reference = recorded.reference
        if not (_comparable_side(recorded) and _comparable_side(reference)):
            if _comparable_side(recorded) and self._comparable(action, recorded):
                self._reference_unresolved(action, recorded)
            self._diverge(action, recorded)
            return
        assert reference is not None
        if not self._comparable(action, recorded):
            self._diverge(action, recorded)
            return
        if not self._record(_Pair(action, recorded, reference)):
            self._diverge(action, recorded)

    def _reference_unresolved(self, action: Action, recorded: ActionFacts) -> None:
        """Violate the pair's outcome obligation as the reference's: no comparable side."""
        reference = recorded.reference
        found = "nothing" if reference is None else str(reference.outcome)
        subject = _path_subject(self.spec, action)
        if subject is None:
            ref = self.sink.obligation(
                CHECKOUT_DISAGREEMENT,
                action.id,
                subject=checkout_subject(action.id),
                witnesses=(
                    Witness("action-kind", action.kind),
                    Witness("divergence", "outcome"),
                ),
            )
        else:
            ref = self.sink.obligation(
                OUTCOME_DISAGREEMENT,
                action.id,
                subject=subject,
                witnesses=(
                    Witness("action-kind", action.kind),
                    Witness("primary-outcome", outcome_label(recorded)),
                    Witness("reference-outcome", "unresolved"),
                ),
            )
        self.sink.covered(COMPARED)
        self.sink.violated(
            ref,
            Attribution.REFERENCE,
            f"differential/eden≠reference: {action.id} {action.kind} completed on "
            f"EdenFS but the reference side recorded {found}, no comparable result",
            (Witness("action", action.id),),
        )

    def _record(self, pair: _Pair) -> bool:
        """Record one pair's obligations; return whether its two sides agree."""
        self.sink.covered(COMPARED)
        primary, reference = outcome_label(pair.primary), outcome_label(pair.reference)
        subject = _path_subject(self.spec, pair.action)
        if subject is None:
            return self._record_checkout(pair, primary, reference)
        kind = pair.action.kind
        outcome = self.sink.obligation(
            OUTCOME_DISAGREEMENT,
            pair.action.id,
            subject=subject,
            witnesses=(
                Witness("action-kind", kind),
                Witness("primary-outcome", primary),
                Witness("reference-outcome", reference),
            ),
        )
        if primary != reference:
            self._judge_outcome(pair, outcome, primary, reference)
            return False
        self.sink.satisfied(outcome)
        if pair.primary.outcome != OK or not DIFFERENTIAL_OBSERVATIONS[kind].keys:
            return True
        value = self.sink.obligation(
            VALUE_DISAGREEMENT,
            pair.action.id,
            subject=subject,
            witnesses=(Witness("action-kind", kind),),
        )
        ours, theirs = _value(pair, pair.primary), _value(pair, pair.reference)
        if ours == theirs:
            self.sink.satisfied(value)
            return True
        self._judge_disagreement(pair, value)
        return False

    def _judge_outcome(
        self, pair: _Pair, ref: ObligationRef, primary: str, reference: str
    ) -> None:
        """Attribute an outcome disagreement.

        An EdenFS failure the reference side did not share is the
        product's. A reference failure is the reference's once the EdenFS
        side's outcome is legal: an EdenFS read or `read_fd` that served
        bytes is judged by those bytes (`_attribute_after_legality`), and
        any other EdenFS `ok` input is legal (`core.input-legality`
        satisfies it). When both sides failed differently the EdenFS
        failure is judged by `core.input-legality`, and the disagreement is
        the reference's.
        """
        reason = (
            f"differential/eden≠reference: {pair.action.id} {pair.action.kind} "
            f"returned {primary} on EdenFS and {reference} on the reference"
        )
        witnesses = (Witness("action", pair.action.id),)
        if pair.reference.outcome != FAIL:
            self.sink.violated(ref, Attribution.PRODUCT, reason, witnesses)
        elif pair.primary.outcome == OK:
            self._attribute_after_legality(
                pair, ref, reason, witnesses, Attribution.REFERENCE
            )
        else:
            self.sink.violated(ref, Attribution.REFERENCE, reason, witnesses)

    def _judge_disagreement(self, pair: _Pair, ref: ObligationRef) -> None:
        """Attribute a value disagreement once the EdenFS side's own legality is known.

        An EdenFS value the direct byte rules reject is the product's. A
        read or `read_fd` value they admit is observed again under this
        obligation (the observation the bytes or fd property files): the
        engine violating it makes the disagreement the product's;
        satisfying it means the EdenFS side is legal under the ordinary
        rules, so the disagreement is the reference's; an engine that cannot
        decide, or a descriptor read a gap reaches, leaves it not evaluated.
        A getattr's observations and a listing's membership checks are
        judged the same way; a kind with no per-value model is
        `undetermined`.
        """
        kind = pair.action.kind
        keys = ", ".join(DIFFERENTIAL_OBSERVATIONS[kind].keys)
        reason = (
            f"differential/eden≠reference: {pair.action.id} {kind} observed "
            f"different {keys} on the two sides"
        )
        witnesses = (Witness("action", pair.action.id),)
        self._attribute_after_legality(
            pair, ref, reason, witnesses, Attribution.UNDETERMINED
        )

    def _attribute_after_legality(
        self,
        pair: _Pair,
        ref: ObligationRef,
        reason: str,
        witnesses: tuple[Witness, ...],
        unmodeled: Attribution,
    ) -> None:
        """Violate `ref` as the product's when the EdenFS side's `ok` is illegal, else the reference's.

        `unmodeled` attributes a kind with no per-value model.
        """
        primary = self._primary_evidence(pair, ref)
        if primary is _ILLEGAL:
            self.sink.violated(
                ref,
                Attribution.PRODUCT,
                f"{reason}; the EdenFS side's value fails a direct rule",
                witnesses,
            )
        elif primary is _LEGAL:
            self.sink.violated(
                ref,
                Attribution.REFERENCE,
                f"{reason}; the EdenFS side's listing is legal",
                witnesses,
            )
        elif primary is None:
            self.sink.violated(ref, unmodeled, reason, witnesses)
        elif isinstance(primary, str):
            self.sink.not_evaluated(ref, PRIMARY_UNDECIDED, primary)
        else:
            assert isinstance(primary, tuple)
            for observation in primary:
                self.sink.observe(observation)
            self.sink.violated_unless_engine(
                ref,
                Attribution.REFERENCE,
                f"{reason}; the EdenFS side's value is legal",
                witnesses,
            )

    def _primary_evidence(
        self, pair: _Pair, ref: ObligationRef
    ) -> tuple[Observation, ...] | str | object | None:
        """Return the EdenFS side's legality evidence for a disagreeing pair.

        `_ILLEGAL` when a direct rule rejects its value, `_LEGAL` when a
        listing passes the membership rules with every path decided, the
        observations the engine must judge, the reason a descriptor read
        is not modeled, or None for a kind with no per-value model.
        """
        recorded = pair.primary
        if pair.action.kind == "read":
            if not bytes_property.directly_legal(recorded, self.candidates):
                return _ILLEGAL
            return (bytes_property.read_observation(recorded, (ref,)),)
        if pair.action.kind == "getattr":
            return tuple(
                attributes_property.getattr_observations(
                    self.check_input, recorded, self.candidates, self.sink, (ref,)
                )
            )
        if pair.action.kind == "readdir":
            return self._listing_legality(recorded, ref)
        if pair.action.kind == "read_fd":
            plan = self.plans.get(pair.action.payload["fd"])
            if plan is None:
                return None
            if not fd_property.directly_legal(recorded, plan, self.candidates):
                return _ILLEGAL
            gap = plan.gap(recorded.window)
            if gap is not None:
                return gap
            return (fd_property.object_observation(recorded, plan, (ref,)),)
        return None

    def _listing_legality(
        self, recorded: ActionFacts, ref: ObligationRef
    ) -> tuple[Observation, ...] | object:
        """Return the readdir property's judgment of a one-shot listing.

        `_ILLEGAL` when it repeats a name `readdir.duplicate-entry` forbids,
        names an unknown, or omits a bound entry, else the readdir
        property's observations of the paths whose presence the shared
        effects do not decide, for the engine (`_LEGAL` when there are none).
        """
        assert recorded.result is not None
        assert recorded.invoked is not None and recorded.completed is not None
        directory = normalize_path(recorded.payload["path"])
        window = Window(recorded.invoked, recorded.completed)
        domain = readdir_property.Domain(self.check_input, directory)
        entries = [str(name) for name in recorded.result.get("entries")]
        if readdir_property.repeated_names(domain, entries, window):
            return _ILLEGAL
        found = readdir_property.membership(domain, entries)
        if found.unknown or found.missing:
            return _ILLEGAL
        observations = readdir_property.undecided_observations(
            recorded.id,
            window,
            recorded.payload["channel"],
            found,
            (ref,),
            (ref,),
        )
        return tuple(observations) if observations else _LEGAL

    def _record_checkout(self, pair: _Pair, primary: str, reference: str) -> bool:
        kind = pair.action.kind
        same_outcome = primary == reference
        ref = self.sink.obligation(
            CHECKOUT_DISAGREEMENT,
            pair.action.id,
            subject=checkout_subject(pair.action.id),
            witnesses=(
                Witness("action-kind", kind),
                Witness("divergence", "value" if same_outcome else "outcome"),
            ),
        )
        try:
            equal = same_outcome and (
                pair.primary.outcome != OK
                or _value(pair, pair.primary) == _value(pair, pair.reference)
            )
        except MalformedObservation as error:
            self.sink.violated(
                ref,
                Attribution.REFERENCE,
                f"differential/malformed: {pair.action.id} {kind}: {error}",
                (Witness("action", pair.action.id),),
            )
            return False
        if equal:
            self.sink.satisfied(ref)
            return True
        attribution = (
            Attribution.REFERENCE
            if not same_outcome and pair.reference.outcome == FAIL
            else Attribution.UNDETERMINED
        )
        detail = (
            "observed different compared fields"
            if same_outcome
            else f"returned {primary} on EdenFS and {reference} on the reference"
        )
        self.sink.violated(
            ref,
            attribution,
            f"differential/eden≠reference: {pair.action.id} {kind} {detail}",
            (Witness("action", pair.action.id),),
        )
        return False


def collect_differential(check_input: CheckInput, sink: CollectorSink) -> None:
    """Compare every shared action of a differential run, in invocation order."""
    spec = check_input.spec
    if not reference_clone(spec):
        return
    comparer = _Comparer(check_input, sink)
    invoked = sorted(
        (recorded.invoked, action.id, action)
        for action in spec.actions
        if expects_reference(spec, action)
        and (recorded := check_input.facts.action(action.id)).invoked is not None
    )
    for _time, _action_id, action in invoked:
        comparer.judge(action)


DIFFERENTIAL_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        RuleDef(
            id=OUTCOME_DISAGREEMENT,
            statement="A validator-admitted shared by-path action has the same "
            "outcome (ok, the same errno, else the same return code) on EdenFS and on "
            "the plain reference checkout; a disagreement whose reference side "
            "failed while the EdenFS side's outcome is legal is the reference's.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("action-kind", "primary-outcome", "reference-outcome"),
        ),
        RuleDef(
            id=VALUE_DISAGREEMENT,
            statement="A validator-admitted shared by-path action that succeeds on "
            "both sides observes equal normalized compared fields; a disagreement "
            "whose EdenFS side is legal under the ordinary rules is the "
            "reference's.",
            subject_kind=SubjectKind.PATH,
            identity_roles=("action-kind",),
        ),
        RuleDef(
            id=CHECKOUT_DISAGREEMENT,
            statement="A validator-admitted shared checkout-wide action (sl status, "
            "sl goto) has the same outcome and normalized compared fields on both "
            "sides.",
            subject_kind=SubjectKind.CHECKOUT,
            identity_roles=("action-kind", "divergence"),
        ),
    )
}

DIFFERENTIAL_PROPERTY = PropertyDef(
    id=DIFFERENTIAL,
    collect=collect_differential,
    rules=DIFFERENTIAL_RULES,
    coverage=(CoverageObligation(COMPARED, "Shared action pairs compared"),),
    not_evaluated_reasons={
        PRIMARY_UNDECIDED: "A value disagreement whose EdenFS side's legality "
        "is not decided",
    },
    consumes=CONSUMES,
)
