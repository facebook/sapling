# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Turn one RunSpec and its recorded history into a verdict (`check`).

1. Gate: the spec must be in bounds and the history must follow it;
   otherwise the verdict is not evaluated and no property runs.
2. Facts and effects: decode every result once (`facts`), then let the
   target's contracts hook record effect groups and observations
   (`_ContractRecorder`).
3. Collect: each core property (daemon-exit, activity, input-legality,
   result-schema, defined here) and each target property records
   obligations and evidence into its own `CollectorRecorder`. A property
   that breaks its contract ends incomplete and its inputs are dropped.
4. Engine: `solver.solve` decides every obligation with engine input.
5. Attribute each engine proof (`_attribute_proof`), resolve each
   obligation (`_Pending.resolve`), and project the verdict kind
   (`project`).

The verdict binds digests of what it judged and of what judged it (`bind`).
"""

from __future__ import annotations

import dataclasses
import errno
import functools
import hashlib
import inspect
import json
import os
import re
import sys
import traceback
from collections.abc import Iterable, Iterator, Mapping, Sequence
from contextlib import contextmanager
from dataclasses import dataclass, field, KW_ONLY
from typing import Any

from . import effects, jsonio, semantics, solver
from .facts import (
    ActionFacts,
    ErrnoUse,
    ERROR_TYPE_FIELD,
    FAIL,
    HistoryFacts,
    READ_PREFIX_BYTES,
    ResultSchema,
    TypedResult,
    UNEXPECTED_REFERENCE,
)
from .history import (
    DAEMON_EXIT,
    Event,
    event_to_json,
    history_problems,
    INFO,
    OK,
    RESERVED_DATA_KEYS,
)
from .jsonio import MAX_ID_BYTES, MAX_SCALAR_BYTES
from .property import (
    Attribution,
    check_scope,
    CheckInput,
    CHECKOUT_SUBJECT,
    CORE_NOT_EVALUATED_REASONS,
    DAEMON_SUBJECT,
    identity_digest,
    is_core,
    MAX_RUN_OBLIGATIONS,
    ObligationResult,
    ObligationStatus,
    path_subject,
    PRODUCT_ATTRIBUTIONS,
    property_error,
    PropertyDef,
    PropertyError,
    PropertyResult,
    RESERVED_NAMESPACES,
    RuleDef,
    SOLVER_INPUT_EXCLUDED,
    SOLVER_UNAVAILABLE,
    SOLVER_WITNESS_REJECTED,
    Subject,
    SubjectKind,
    validate_obligation_shape,
    validate_property_error,
    validate_registry,
    validate_result,
    validate_subject,
    validate_witness,
    Witness,
    WorkAccount,
)
from .runspec import Action, RunSpec, spec_problems, to_json
from .sources import (
    CHECKER_POLICY,
    CheckerSourceUnavailable,
    read_module_source,
    source_closure,
    SOURCE_NAMESPACE,
    SourcePolicy,
    SourceReader,
)
from .target import (
    ActionDef,
    DEFAULT_SEMANTICS_PROFILE,
    MUTATING_ROLES,
    Target,
    validate_target_metadata,
    validate_target_sources,
)


ACCEPT = "accept"
REJECT = "reject"
HARNESS_SUSPECT = "harness-suspect"
NOT_EVALUATED = "not-evaluated"
KINDS = (ACCEPT, NOT_EVALUATED, HARNESS_SUSPECT, REJECT)

INVALID_HISTORY = "invalid-history"
INVALID_RUNSPEC = "invalid-runspec"
PROPERTY_ERROR = "property-error"
BLOCKED = "blocked"
NO_EVALUATION = "no-evaluation"
_GATE_CAUSES = frozenset({INVALID_HISTORY, INVALID_RUNSPEC})

VERDICT_SCHEMA_VERSION = 1
CHECKER_VERSION = "exploratory-checker-v1"
VERDICT_OPERATIONAL_BYTES = 2 * 1024 * 1024

# Deterministic work bounds of the consistency engine; they enter the
# semantic digest so a budget change rebinds every verdict.
DECISION_WORK = solver.DECISION_WORK
EXPLANATION_WORK = solver.EXPLANATION_WORK
MAX_LIVE_STATES = solver.MAX_LIVE_STATES
MAX_CANDIDATES = solver.MAX_CANDIDATES
SOLVER_BUDGETS: Mapping[str, int] = solver.SOLVER_BUDGETS

_ALL_ACCEPT = "all-properties-accept"
NO_PROPERTY_REASON = "no-evaluation/properties≠target: the target declares no property"
_NO_OBLIGATION_REASON = (
    "no-evaluation/properties≠history: no target property declared an obligation"
)
_DIGEST = re.compile(r"sha256:[0-9a-f]{64}")
_PLAIN_ID = re.compile(r"[a-z][a-z0-9-]*")
_RUN_SCOPE = "run"

CORE_DAEMON_EXIT = "core.daemon-exit"
CORE_ACTIVITY = "core.activity"
CORE_INPUT_LEGALITY = "core.input-legality"
CORE_RESULT_SCHEMA = "core.result-schema"
CORE_PROPERTY_IDS = (
    CORE_DAEMON_EXIT,
    CORE_ACTIVITY,
    CORE_INPUT_LEGALITY,
    CORE_RESULT_SCHEMA,
)
CORE_RULES: Mapping[str, RuleDef] = {
    CORE_DAEMON_EXIT: RuleDef(
        id=CORE_DAEMON_EXIT,
        statement=(
            "The daemon does not exit before cleanup unless a nemesis owns the exit."
        ),
        subject_kind=SubjectKind.DAEMON,
        identity_roles=("exit-class",),
    ),
    CORE_ACTIVITY: RuleDef(
        id=CORE_ACTIVITY,
        statement="A run completes at least one operation that exercises the target: "
        "a modeled kind, or a kind whose declared roles mutate state.",
        subject_kind=SubjectKind.CHECKOUT,
    ),
    CORE_INPUT_LEGALITY: RuleDef(
        id=CORE_INPUT_LEGALITY,
        statement=(
            "A modeled input fails only in one of these ways: with a state "
            "errno the state at the failure allows; with any errno a nemesis "
            "caused; or with ETIMEDOUT under the timeout excuse. A failure "
            "without an errno is judged the same way by its error type."
        ),
        subject_kind=SubjectKind.PATH,
        identity_roles=("input-kind", "errno"),
    ),
    CORE_RESULT_SCHEMA: RuleDef(
        id=CORE_RESULT_SCHEMA,
        statement=(
            "Every recorded result of a completed modeled action, and every "
            "recorded reference side of any action, has exactly the fields and "
            "kinds its result schema declares for its outcome."
        ),
        subject_kind=SubjectKind.CHECKOUT,
        identity_roles=("action-kind",),
    ),
}


def content_digest(data: bytes) -> dict[str, Any]:
    """Return the read facts for ``data``: its ``READ_PREFIX_BYTES`` prefix, length, sha256."""
    return {
        "bytes_prefix": data[:READ_PREFIX_BYTES].hex(),
        "length": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


@dataclass(frozen=True)
class Bindings:
    """Content digests binding a verdict to exactly what judged it and what it judged.

    `semantic` digests the checker version, `checker_sources(target)` and
    its import policy, every action's static declarations, and the engine
    budgets.
    `execution` is the digest of the containment execution document when
    the caller supplies one. `semantics_profile` is evidence metadata.
    """

    runspec: str
    history: str
    semantic: str
    execution: str | None = None
    semantics_profile: str = DEFAULT_SEMANTICS_PROFILE


@dataclass(frozen=True)
class Work:
    """Deterministic work counters per phase and per-component high-water marks."""

    phases: Mapping[str, Mapping[str, int]] = field(default_factory=dict)
    components: Mapping[str, Mapping[str, int]] = field(default_factory=dict)


@dataclass(frozen=True)
class PropertyReport:
    """Every property result behind a verdict, plus any structural gate failure.

    `gate` lists the structural problems that kept evaluation from running;
    it is non-empty exactly when `results` is empty because of a gate.
    `proofs` are the certificates violated obligations reference.
    """

    results: tuple[PropertyResult, ...]
    identity_digest: str
    bindings: Bindings
    gate: tuple[str, ...] = ()
    proofs: tuple[effects.ProofCertificate, ...] = ()
    work: Work = field(default_factory=Work)


@dataclass(frozen=True)
class Verdict:
    """A judged run: its projection (`kind`, `reason`, lanes) and every result behind it.

    `kind` and `reason` project the per-obligation results
    (reject > harness-suspect > not-evaluated > accept) and never supply
    task identity. `lanes` maps each target property to its own projected
    kind. `not_evaluated_cause` types a not-evaluated verdict.
    """

    kind: str
    reason: str
    _: KW_ONLY
    lanes: dict[str, str] = field(default_factory=dict)
    not_evaluated_cause: str | None = None
    report: PropertyReport | None = None


def type_name(value: Any) -> str:
    """Return a value's type name; never raises, whatever the type defines."""
    try:
        name = type(value).__name__
    except Exception:  # noqa: BLE001 - a hostile metaclass cannot escape
        return "<unnamed type>"
    return name if type(name) is str and name else "<unnamed type>"


def exception_text(raised: BaseException) -> str:
    """Return an exception's text; never raises, whatever its ``__str__`` does."""
    try:
        text = str(raised)
    except Exception:  # noqa: BLE001 - a hostile __str__ cannot escape
        return f"<unprintable {type_name(raised)}>"
    return text if type(text) is str else f"<unprintable {type_name(raised)}>"


def value_label(value: Any) -> str:
    """Return a short label for a value in a diagnostic; never raises.

    Integers wider than 64 bits are named by their size, since their text
    can exceed the interpreter's conversion limit; non-scalars by type.
    """
    if type(value) is int:
        if value.bit_length() > 64:
            return f"<int of {value.bit_length()} bits>"
        return str(value)
    if value is None or type(value) in (bool, float):
        return repr(value)
    if type(value) is str and len(value) <= 64:
        return repr(value)
    return f"<{type_name(value)}>"


def _count(value: Any, what: str) -> int:
    if type(value) is not int or value < 0:
        raise ValueError(f"{what} must be a non-negative int, got {value_label(value)}")
    return value


def _bounded(value: Any, what: str) -> str:
    if type(value) is not str or len(value.encode("utf-8")) > MAX_SCALAR_BYTES:
        raise ValueError(f"{what} must be a string of at most {MAX_SCALAR_BYTES} bytes")
    return value


# When one obligation collects violations with different attributions, the
# most severe stands. Product outranks harness because a recorder defect
# cannot fabricate product evidence (the target's contracts leave the
# effects of an action whose result does not decode indeterminate), while
# ranking harness first would let an unrelated recorder glitch mask a
# product finding.
_SEVERITY: Mapping[Attribution, int] = {
    Attribution.PRODUCT: 4,
    Attribution.UNDETERMINED: 3,
    Attribution.ORACLE_SUSPECT: 2,
    Attribution.HARNESS: 1,
    Attribution.REFERENCE: 0,
}


@dataclass
class _Pending:
    """One declared obligation and everything recorded against it so far."""

    subject: Subject
    witnesses: tuple[Witness, ...]
    status: ObligationStatus | None = None
    attribution: Attribution | None = None
    reason: str | None = None
    reason_id: str | None = None
    extra: tuple[Witness, ...] = ()
    inputs: int = 0
    agree: int = 0
    # The most severe attribution over the engine's proofs, and one proof that carries it.
    engine_attribution: Attribution = Attribution.PRODUCT
    engine_proof: str | None = None
    # A violation that holds once the engine satisfies the obligation.
    unless_engine: tuple[Attribution, str, tuple[Witness, ...]] | None = None

    def resolve(
        self, rule_id: str, scope: str, outcome: solver.Outcome | None = None
    ) -> ObligationResult | None:
        """Return the final result, or None when nothing resolved the obligation.

        A direct violation is final. Otherwise an engine violation wins
        (even over a collector's not-evaluated result), then a collector's
        not-evaluated result; an obligation an Agree constraint supports is
        not evaluated unless a direct check satisfied it (Agree is a direct
        comparison, never engine input); then the engine's result for an
        obligation with engine input (not evaluated when no engine decided
        it; a satisfied one becomes its `violated_unless_engine` violation
        when one was recorded), then that violation without engine input,
        then a direct satisfaction.
        """
        common = {"rule_id": rule_id, "scope": scope, "subject": self.subject}
        if self.status == ObligationStatus.VIOLATED:
            witnesses = tuple(dict.fromkeys((*self.witnesses, *self.extra)))
            return ObligationResult(
                **common,
                status=self.status,
                witnesses=witnesses,
                attribution=self.attribution,
                reason=self.reason,
            )
        if outcome is not None and outcome.status == ObligationStatus.VIOLATED:
            proof = self.engine_proof or outcome.proofs[0]
            return ObligationResult(
                **common,
                status=ObligationStatus.VIOLATED,
                witnesses=self.witnesses,
                attribution=self.engine_attribution,
                reason=_engine_reason(rule_id, proof, outcome),
                proof=proof,
                component=outcome.component,
            )
        if self.status == ObligationStatus.NOT_EVALUATED:
            return ObligationResult(
                **common,
                status=ObligationStatus.NOT_EVALUATED,
                witnesses=self.witnesses,
                reason=self.reason or None,
                not_evaluated_reason=self.reason_id,
            )
        if self.agree and self.status != ObligationStatus.SATISFIED:
            return ObligationResult(
                **common,
                status=ObligationStatus.NOT_EVALUATED,
                witnesses=self.witnesses,
                not_evaluated_reason=SOLVER_UNAVAILABLE,
            )
        if self.inputs:
            if outcome is None or outcome.status == ObligationStatus.NOT_EVALUATED:
                return ObligationResult(
                    **common,
                    status=ObligationStatus.NOT_EVALUATED,
                    witnesses=self.witnesses,
                    not_evaluated_reason=(
                        SOLVER_UNAVAILABLE if outcome is None else outcome.reason
                    ),
                    component=None if outcome is None else outcome.component,
                )
            if self.unless_engine is not None:
                return self._unless_engine(common, outcome.component)
            return ObligationResult(
                **common,
                status=ObligationStatus.SATISFIED,
                witnesses=self.witnesses,
                component=outcome.component,
            )
        if self.unless_engine is not None:
            return self._unless_engine(common, None)
        if self.status == ObligationStatus.SATISFIED:
            return ObligationResult(
                **common, status=self.status, witnesses=self.witnesses
            )
        return None

    def _unless_engine(
        self, common: dict[str, Any], component: str | None
    ) -> ObligationResult:
        assert self.unless_engine is not None
        attribution, reason, extra = self.unless_engine
        return ObligationResult(
            **common,
            status=ObligationStatus.VIOLATED,
            witnesses=tuple(dict.fromkeys((*self.witnesses, *extra))),
            attribution=attribution,
            reason=reason,
            component=component,
        )


def _engine_reason(rule_id: str, proof: str, outcome: solver.Outcome) -> str:
    return (
        f"solver/{rule_id}: no explanation satisfies the inputs of {proof} "
        f"in {outcome.component}"
    )


def _witnesses(raw: Iterable[Witness]) -> tuple[Witness, ...]:
    witnesses = tuple(raw)
    for witness in witnesses:
        validate_witness(witness)
    return witnesses


class _Nodes:
    """Solver nodes and constraints: the contract ones plus what was recorded here.

    Each property records into its own table seeded with the contracts
    hook's groups and observations, so its constraints name only contract
    nodes and its own; `merge_nodes` joins the tables of the properties
    that finished. Recording an observation whose content equals a known
    one merges the obligations; any other reuse of a node ID raises.
    Constraints are named by `effects.constraint_id`; recording one twice
    raises, its endpoints must name known nodes of the types its class
    requires, and every constraint but `Agree` joins nodes (or outcome
    variables) of one side.
    """

    def __init__(self, effect_set: effects.EffectSet) -> None:
        self.groups = effect_set.group_ids
        self.outcome_vars = effect_set.outcome_vars
        self.observations: dict[str, effects.Observation] = {
            item.id: item for item in effect_set.observations
        }
        self.constraints: dict[str, effects.Constraint] = {}
        self._group_sides = {group.id: group.side for group in effect_set.groups}
        self._var_sides: dict[str, set[str]] = {}
        for group in effect_set.groups:
            self._var_sides.setdefault(group.outcome_var, set()).add(group.side)

    def _sides(self, constraint: effects.Constraint) -> set[str]:
        if isinstance(constraint, effects.Exclusive):
            return {
                side for var in constraint.outcome_vars for side in self._var_sides[var]
            }
        return {
            self._group_sides[node]
            if node in self._group_sides
            else self.observations[node].side
            for node in constraint.nodes()
        }

    def observe(self, observation: effects.Observation) -> None:
        """Record an observation, merging an identical one's obligations."""
        node = observation.id
        if node in self.groups:
            raise ValueError(f"observation reuses group id {node!r:.200}")
        known = self.observations.get(node)
        if known is None:
            self.observations[node] = observation
            return
        if known.content() != observation.content():
            raise ValueError(f"node id {node!r:.200} names a different observation")
        merged = (*known.obligations, *observation.obligations)
        self.observations[node] = dataclasses.replace(known, obligations=merged)

    def _check_endpoint(self, node: str, required: effects.NodeType) -> None:
        group, observed = node in self.groups, node in self.observations
        if not group and not observed:
            raise ValueError(f"constraint names unknown node {node!r:.200}")
        if required == effects.NodeType.GROUP and not group:
            raise ValueError(f"constraint endpoint {node!r:.200} is not a group")
        if required == effects.NodeType.OBSERVATION and not observed:
            raise ValueError(f"constraint endpoint {node!r:.200} is not an observation")

    def constrain(self, constraint: effects.Constraint) -> None:
        """Record a constraint whose endpoints match its signature."""
        for node, required in constraint.signature():
            self._check_endpoint(node, required)
        if isinstance(constraint, effects.Exclusive):
            unknown = sorted(set(constraint.outcome_vars) - self.outcome_vars)
            if unknown:
                raise ValueError(f"exclusive names unknown outcome variables {unknown}")
        if not isinstance(constraint, effects.Agree):
            sides = sorted(self._sides(constraint))
            if len(sides) > 1:
                raise ValueError(
                    f"{type_name(constraint)} constraint joins sides {sides}"
                )
        name = effects.constraint_id(constraint)
        if name in self.constraints:
            raise ValueError(f"constraint {name} recorded twice")
        self.constraints[name] = constraint


def _check_node_action(
    facts: HistoryFacts, action_id: str, window: effects.Window, *, completes: bool
) -> None:
    """Reject a node whose action or window disagrees with the recorded history.

    The action is in the spec and was invoked, and the window starts no
    earlier than the invocation. A node that `completes` (an observation,
    which a result reports, or an acked group) needs a completed action
    and ends no later than its completion.
    """
    recorded = facts.actions.get(action_id)
    if recorded is None or recorded.invoked is None:
        raise ValueError(f"node names action {action_id!r:.200}, never invoked")
    if window.lo < recorded.invoked:
        raise ValueError(f"node window starts before {action_id!r:.200} was invoked")
    if not completes:
        return
    if recorded.completed is None:
        raise ValueError(f"node needs {action_id!r:.200} to complete, and it did not")
    if window.hi > recorded.completed:
        raise ValueError(f"node window ends after {action_id!r:.200} completed")


class CollectorRecorder:
    """The checker-owned `CollectorSink` one property's collector writes to.

    Every method validates against the property's declarations and raises
    on a mistake. The first violation is also kept, so a collector that
    catches the exception still ends incomplete with that error.
    Declaring more than `MAX_RUN_OBLIGATIONS` obligations is a violation.
    Declaring an obligation claims every contract observation,
    requirement, and non-Agree constraint supporting it as solver input.
    One obligation's observations on one side share a serving path
    (`serving_paths`; an undeclared channel is its own), because proof
    attribution groups co-observers by obligation.
    """

    def __init__(
        self,
        definition: PropertyDef,
        nodes: _Nodes,
        check_input: CheckInput,
        serving_paths: Mapping[str, str] | None = None,
    ) -> None:
        self._definition = definition
        self._nodes = nodes
        self._input = check_input
        self._serving_paths = serving_paths or {}
        self._observed_paths: dict[tuple[effects.ObligationRef, str], str] = {}
        self._coverage_ids = frozenset(o.id for o in definition.coverage)
        self._pending: dict[tuple[str, str], _Pending] = {}
        self._coverage: dict[str, int] = {}
        self._violation: PropertyError | None = None

    @property
    def violation(self) -> PropertyError | None:
        """Return the first contract violation, even if the collector caught it."""
        return self._violation

    @contextmanager
    def _contract(self) -> Iterator[None]:
        try:
            yield
        except Exception as error:
            if self._violation is None:
                self._violation = property_error(
                    "PropertyContractError",
                    f"{type_name(error)}: {exception_text(error)}",
                )
            raise

    def obligation(
        self,
        rule_id: str,
        scope: str,
        *,
        subject: Subject,
        witnesses: Iterable[Witness] = (),
    ) -> effects.ObligationRef:
        """Declare one `(rule obligation, scope)`; repeating it returns the same ref."""
        with self._contract():
            return self._declare(rule_id, scope, subject, _witnesses(witnesses))

    def _declare(
        self, rule_id: str, scope: str, subject: Subject, witnesses: tuple[Witness, ...]
    ) -> effects.ObligationRef:
        pid = self._definition.id
        rule = self._definition.rules.get(rule_id)
        if rule is None:
            raise ValueError(f"{pid}: obligation names unknown rule {rule_id!r:.200}")
        check_scope(scope)
        validate_subject(subject)
        if subject.kind != rule.subject_kind:
            raise ValueError(
                f"{rule_id}: subject {subject.token!r:.200} is not a {rule.subject_kind}"
            )
        key = (rule_id, scope)
        known = self._pending.get(key)
        if known is not None:
            if (known.subject, known.witnesses) != (subject, witnesses):
                raise ValueError(f"{rule_id}[{scope!r:.200}] redeclared differently")
            return effects.ObligationRef(rule_id, scope)
        if len(self._pending) >= MAX_RUN_OBLIGATIONS:
            raise ValueError(f"{pid}: more than {MAX_RUN_OBLIGATIONS} obligations")
        ref = effects.ObligationRef(rule_id, scope)
        derived = self._input.effects
        agree = derived.agree_supports(ref)
        self._pending[key] = _Pending(
            subject, witnesses, inputs=derived.supports(ref) - agree, agree=agree
        )
        return ref

    def _state(self, ref: Any) -> _Pending:
        if not isinstance(ref, effects.ObligationRef):
            raise ValueError(f"{type_name(ref)} is not an ObligationRef")
        state = self._pending.get((ref.rule_id, ref.scope))
        if state is None:
            raise ValueError(f"{self._definition.id}: undeclared obligation {ref}")
        return state

    def observe(self, observation: effects.Observation) -> None:
        """Record one observation supporting its obligations, all declared here."""
        with self._contract():
            if not isinstance(observation, effects.Observation):
                raise ValueError(f"{type_name(observation)} is not an Observation")
            states = [self._state(ref) for ref in observation.obligations]
            _check_node_action(
                self._input.facts,
                observation.action,
                observation.window,
                completes=True,
            )
            path = self._serving_paths.get(observation.channel, observation.channel)
            for ref in observation.obligations:
                known = self._observed_paths.setdefault((ref, observation.side), path)
                if known != path:
                    raise ValueError(
                        f"{ref.rule_id}[{ref.scope!r:.200}] has {observation.side} "
                        f"observations on serving paths {known!r} and {path!r}"
                    )
            self._nodes.observe(observation)
            for state in states:
                state.inputs += 1

    def constrain(self, constraint: effects.Constraint) -> None:
        """Record one constraint whose endpoints match its signature."""
        with self._contract():
            if not isinstance(constraint, effects.CONSTRAINT_TYPES):
                raise ValueError(f"{type_name(constraint)} is not a Constraint")
            state = self._state(constraint.obligation)
            self._nodes.constrain(constraint)
            if isinstance(constraint, effects.Agree):
                state.agree += 1
            else:
                state.inputs += 1

    def unclaimed(self) -> list[effects.ObligationRef]:
        """Return contract inputs' obligations of this property it never declared."""
        return sorted(
            (
                ref
                for ref in self._input.effects.contract_obligations()
                if ref.rule_id in self._definition.rules
                and (ref.rule_id, ref.scope) not in self._pending
            ),
            key=effects.ObligationRef.document,
        )

    def satisfied(self, ref: effects.ObligationRef) -> None:
        """Record that a direct check satisfied the obligation."""
        with self._contract():
            state = self._state(ref)
            if state.status is None:
                state.status = ObligationStatus.SATISFIED

    def violated(
        self,
        ref: effects.ObligationRef,
        attribution: Attribution,
        reason: str,
        witnesses: Iterable[Witness] = (),
    ) -> None:
        """Record a direct violation; the most severe attribution is final.

        Severity is product > undetermined > oracle-suspect > harness >
        reference. A later violation replaces the recorded one only when it
        is strictly more severe, so a violation is never downgraded; among
        equally severe violations the earliest recorded wins.
        """
        with self._contract():
            state = self._state(ref)
            typed = Attribution(attribution)
            text = _bounded(reason, "reason")
            extra = _witnesses(witnesses)
            if (
                state.status != ObligationStatus.VIOLATED
                or _SEVERITY[typed] > _SEVERITY[Attribution(state.attribution)]
            ):
                state.status = ObligationStatus.VIOLATED
                state.attribution, state.reason, state.extra = typed, text, extra
                state.reason_id = None

    def violated_unless_engine(
        self,
        ref: effects.ObligationRef,
        attribution: Attribution,
        reason: str,
        witnesses: Iterable[Witness] = (),
    ) -> None:
        """Record a violation that holds once the engine satisfies the obligation.

        The most severe one recorded is kept, as for direct violations.
        """
        with self._contract():
            state = self._state(ref)
            typed = Attribution(attribution)
            found = (typed, _bounded(reason, "reason"), _witnesses(witnesses))
            if (
                state.unless_engine is None
                or _SEVERITY[typed] > _SEVERITY[state.unless_engine[0]]
            ):
                state.unless_engine = found

    def not_evaluated(
        self, ref: effects.ObligationRef, reason_id: str, detail: str = ""
    ) -> None:
        """Record why the obligation could not be decided, by declared reason."""
        with self._contract():
            state = self._state(ref)
            if (
                reason_id not in self._definition.not_evaluated_reasons
                and reason_id not in CORE_NOT_EVALUATED_REASONS
            ):
                raise ValueError(
                    f"{self._definition.id}: undeclared reason {reason_id!r:.200}"
                )
            text = _bounded(detail, "detail")
            if state.status in (None, ObligationStatus.SATISFIED):
                state.status = ObligationStatus.NOT_EVALUATED
                state.reason, state.reason_id = text, reason_id

    def covered(self, obligation_id: str, count: int = 1) -> None:
        """Count observations toward one declared coverage obligation."""
        with self._contract():
            if obligation_id not in self._coverage_ids:
                raise ValueError(f"{self._definition.id}: undeclared {obligation_id!r}")
            self._coverage[obligation_id] = self._coverage.get(
                obligation_id, 0
            ) + _count(count, "coverage count")

    def unresolved(self) -> list[tuple[str, str]]:
        """Return the declared obligations nothing resolved, in key order."""
        return [
            key
            for key, state in sorted(self._pending.items())
            if state.resolve(*key) is None
        ]

    def counters(self) -> dict[str, int]:
        """Return this collector's deterministic construction counters."""
        return {"obligations": len(self._pending)}

    def declared(self) -> frozenset[effects.ObligationRef]:
        """Return every obligation this collector declared."""
        return frozenset(effects.ObligationRef(*key) for key in self._pending)

    def attribute(
        self, found: Mapping[effects.ObligationRef, tuple[Attribution, str]]
    ) -> None:
        """Set the attribution and proof engine violations of the given obligations carry."""
        for ref, (attribution, proof) in found.items():
            state = self._pending.get((ref.rule_id, ref.scope))
            if state is not None:
                state.engine_attribution, state.engine_proof = attribution, proof

    def result(
        self,
        error: PropertyError | None = None,
        outcomes: Mapping[effects.ObligationRef, solver.Outcome] | None = None,
    ) -> PropertyResult:
        """Return every resolved obligation, sorted, incomplete when an error stopped it.

        `outcomes` are the engine's per-obligation results; without them an
        obligation with solver input is not evaluated.
        """
        found = outcomes or {}
        obligations = tuple(
            resolved
            for key, state in sorted(self._pending.items())
            if (resolved := state.resolve(*key, found.get(effects.ObligationRef(*key))))
            is not None
        )
        return PropertyResult(
            property_id=self._definition.id,
            obligations=obligations,
            coverage=dict(sorted(self._coverage.items())),
            error=error,
        )


def _more(items: Sequence[Any]) -> str:
    return f" (+{len(items) - 1} more)" if len(items) > 1 else ""


def _evaluate_one(
    definition: PropertyDef,
    check_input: CheckInput,
    serving_paths: Mapping[str, str] | None = None,
) -> tuple[CollectorRecorder, _Nodes, PropertyError | None]:
    """Run one collector into its own recorder and node table; return its error."""
    nodes = _Nodes(check_input.effects)
    recorder = CollectorRecorder(definition, nodes, check_input, serving_paths)
    escaped: PropertyError | None = None
    try:
        definition.collect(check_input, recorder)
    except Exception as raised:  # noqa: BLE001 - kept as a typed property error
        escaped = property_error(type_name(raised), exception_text(raised))
    error = recorder.violation or escaped
    unresolved = recorder.unresolved()
    if error is None and unresolved:
        rule_id, scope = unresolved[0]
        error = property_error(
            "PropertyContractError",
            f"{rule_id}[{scope!r:.200}] has no result and no solver input"
            + _more(unresolved),
        )
    unclaimed = recorder.unclaimed()
    if error is None and unclaimed:
        error = property_error(
            "PropertyContractError",
            f"a contract input supports {unclaimed[0]}, which {definition.id} never "
            f"declared" + _more(unclaimed),
        )
    if error is None:
        try:
            validate_result(definition, recorder.result())
        except ValueError as invalid:
            error = property_error("InvalidResult", exception_text(invalid))
    return recorder, nodes, error


def merge_nodes(
    effect_set: effects.EffectSet, tables: Mapping[str, _Nodes]
) -> tuple[_Nodes, dict[str, PropertyError]]:
    """Join the node tables of the properties that finished, independent of order.

    An observation ID that two properties recorded with different
    content is a contract error of every property that recorded it,
    whatever their order, and none of their nodes is kept. The rest merge:
    identical observations union their obligations and constraints join,
    together with the contracts hook's constraints. Returns the merged
    table and the error of each conflicting property.
    """
    derived = {item.id for item in effect_set.observations}
    contents: dict[str, dict[str, bytes]] = {}
    for pid, table in tables.items():
        for node, observation in table.observations.items():
            if node not in derived:
                contents.setdefault(node, {})[pid] = _canonical(observation.content())
    clashes: dict[str, list[tuple[str, list[str]]]] = {}
    for node, by_property in sorted(contents.items()):
        if len(set(by_property.values())) > 1:
            for pid in by_property:
                others = sorted(other for other in by_property if other != pid)
                clashes.setdefault(pid, []).append((node, others))
    errors = {
        pid: property_error(
            "PropertyContractError",
            f"node id {found[0][0]!r:.200} names a different observation in "
            f"{found[0][1]}" + _more(found),
        )
        for pid, found in clashes.items()
    }
    merged = _Nodes(effect_set)
    constraints: dict[str, effects.Constraint] = {
        effects.constraint_id(item): item for item in effect_set.constraints
    }
    for pid, table in tables.items():
        if pid in errors:
            continue
        for observation in table.observations.values():
            merged.observe(observation)
        constraints.update(table.constraints)
    merged.constraints = dict(sorted(constraints.items()))
    return merged, errors


def _finish(
    definition: PropertyDef,
    recorder: CollectorRecorder,
    error: PropertyError | None,
    outcomes: Mapping[effects.ObligationRef, solver.Outcome],
) -> PropertyResult:
    """Resolve one property's obligations with the engine's outcomes and validate them.

    An incomplete property keeps what it resolved itself; its solver
    input was never engine input, so those obligations are not evaluated.
    """
    if error is not None:
        return recorder.result(error)
    result = recorder.result(None, outcomes)
    try:
        validate_result(definition, result)
    except ValueError as invalid:
        result = recorder.result(
            property_error("InvalidResult", exception_text(invalid))
        )
    return result


def engine_problem(
    effect_set: effects.EffectSet,
    nodes: _Nodes,
    healthy: frozenset[effects.ObligationRef],
) -> tuple[semantics.Problem, frozenset[effects.ObligationRef]]:
    """Return the engine problem over inputs supporting a complete property's obligations.

    Every group stays; a requirement, observation, or constraint that
    supports no obligation of a complete property is left out, and an
    observation keeps only such obligations. Also returns the obligations
    of complete properties' constraints left out because they name such an
    observation: those obligations are not evaluated (unless violated).
    """
    groups = tuple(
        dataclasses.replace(
            group,
            requires=tuple(r for r in group.requires if r.obligation in healthy),
        )
        for group in effect_set.groups
    )
    observations = []
    for observation in sorted(nodes.observations.values(), key=lambda o: o.id):
        refs = tuple(ref for ref in observation.obligations if ref in healthy)
        if refs:
            observations.append(dataclasses.replace(observation, obligations=refs))
    present = {o.id for o in observations} | effect_set.group_ids
    constraints = []
    excluded = set()
    for _name, constraint in sorted(nodes.constraints.items()):
        if constraint.obligation not in healthy:
            continue
        if all(node in present for node in constraint.nodes()):
            constraints.append(constraint)
        else:
            excluded.add(constraint.obligation)
    problem = semantics.Problem(
        groups, tuple(observations), tuple(constraints), effect_set.initial
    )
    return problem, frozenset(excluded)


def _certified(
    problem: semantics.Problem, solution: solver.Solution
) -> solver.Solution:
    """Return `solution`, with every outcome not evaluated if its witness fails replay.

    The engine returns a witness only when it found every input satisfiable
    at once; `semantics.explains` replays it independently of the search.
    """
    if solution.witness is None:
        return solution
    try:
        holds = semantics.explains(problem, solution.witness)
    except effects.UnknownInitial:
        holds = False
    if holds:
        return solution
    rejected = solver.Outcome(
        ObligationStatus.NOT_EVALUATED, reason=SOLVER_WITNESS_REJECTED
    )
    return dataclasses.replace(
        solution, outcomes={ref: rejected for ref in solution.outcomes}
    )


def _with_excluded(
    solution: solver.Solution, excluded: frozenset[effects.ObligationRef]
) -> dict[effects.ObligationRef, solver.Outcome]:
    """Return the engine outcomes, with excluded-input obligations not evaluated."""
    outcomes = dict(solution.outcomes)
    for ref in excluded:
        known = outcomes.get(ref)
        if known is None or known.status != ObligationStatus.VIOLATED:
            outcomes[ref] = solver.Outcome(
                ObligationStatus.NOT_EVALUATED, reason=SOLVER_INPUT_EXCLUDED
            )
    return outcomes


ATTRIBUTION_WORK = solver.EXPLANATION_WORK
ATTRIBUTION_TRIAL_WORK = solver.EXPLANATION_WORK // solver.TRIAL_SHARE


def _observation_key(observation: effects.Observation) -> bytes:
    """Return what two corroborating observations share: side, cells, expectation."""
    return _canonical(
        [
            observation.side,
            [cell.document() for cell in observation.cells],
            observation.expect.document(),
        ]
    )


def _co_observers(
    problem: semantics.Problem,
) -> dict[str, frozenset[str]]:
    """Return, per observation, the channels of same-side observations sharing an obligation.

    A collector that files two outputs of one action under one obligation
    (sl status and its `sl diff`) declares them two observers of one claim.
    """
    by_ref: dict[tuple[str, effects.ObligationRef], set[str]] = {}
    for o in problem.observations:
        for ref in o.obligations:
            by_ref.setdefault((o.side, ref), set()).add(o.channel)
    return {
        o.id: frozenset(
            channel for ref in o.obligations for channel in by_ref[(o.side, ref)]
        )
        for o in problem.observations
    }


def _attribute_proof(
    problem: semantics.Problem,
    proof: effects.ProofCertificate,
    serving_paths: Mapping[str, str],
    alternatives: Mapping[bytes, Sequence[effects.Observation]],
    co_observers: Mapping[str, frozenset[str]],
    meter: dict[str, int],
) -> Attribution:
    """Attribute one proof from its observations' corroborators.

    For each proof observation `o`, a corroborator is a same-side
    observation with `o`'s cells and expectation that keeps the proof
    unsatisfiable when it replaces `o`, decided by re-solving; and every
    same-side observation supporting one of `o`'s obligations observes
    the same claim. A trial runs only for a candidate whose channel is
    new among `o` and its corroborators, the only ones that can change the
    result. When some `o` and its corroborators span two serving paths
    the observers agree against the contract effects: `oracle-suspect`;
    when they span one serving path but two channels: `undetermined`;
    otherwise `product`. A channel without a declared serving path is its
    own serving path.

    The proof has its own budget of `ATTRIBUTION_WORK` (each trial at
    most `ATTRIBUTION_TRIAL_WORK`), so its attribution never depends on
    other proofs or on node IDs. A trial that is undecided, or never runs because the
    budget ran out, could have found a corroborator: a proof that would
    otherwise be `product` is then `undetermined`, still product-actionable.
    """
    by_id = {o.id: o for o in problem.observations}
    core = set(proof.observations) | set(proof.constraints)
    suspect = shared = truncated = False
    spent = 0
    for name in proof.observations:
        observation = by_id[name]
        channels = {observation.channel} | co_observers.get(name, frozenset())
        for other in alternatives.get(_observation_key(observation), ()):
            if other.id in core or other.channel in channels:
                continue
            budget = min(ATTRIBUTION_TRIAL_WORK, ATTRIBUTION_WORK - spent)
            if budget <= 0:
                meter["exhausted"] += 1
                truncated = True
                break
            trial = problem.restrict((core - {name}) | {other.id})
            verdict, used = solver.satisfiable(trial, work=budget)
            spent += used
            meter["trials"] += 1
            if verdict == "unsat":
                channels.add(other.channel)
            elif verdict == "unknown":
                meter["unknown"] += 1
                truncated = True
        groups = {serving_paths.get(channel, channel) for channel in channels}
        suspect = suspect or len(groups) > 1
        shared = shared or len(channels) > 1
    meter["work"] += spent
    if suspect:
        return Attribution.ORACLE_SUSPECT
    return Attribution.UNDETERMINED if shared or truncated else Attribution.PRODUCT


def _engine_attributions(
    problem: semantics.Problem,
    solution: solver.Solution,
    serving_paths: Mapping[str, str],
) -> tuple[dict[effects.ObligationRef, tuple[Attribution, str]], dict[str, int]]:
    """Attribute each engine-violated obligation and count the attribution work.

    A proof entirely on the reference side is `reference`; any other is
    attributed by `_attribute_proof`. Proofs are taken in ID order and
    candidates in the engine's ID-free deletion order, so the result is
    deterministic; every proof has its own attribution budget. Each
    obligation takes the most severe attribution over all of its proofs,
    with the first proof (in the engine's order) that carries it.
    """
    sides: dict[str, str] = {group.id: group.side for group in problem.groups}
    sides.update((o.id, o.side) for o in problem.observations)
    alternatives: dict[bytes, list[effects.Observation]] = {}
    if solution.proofs:
        order = semantics.deletion_order(problem)
        for observation in sorted(problem.observations, key=lambda o: order[o.id]):
            key = _observation_key(observation)
            alternatives.setdefault(key, []).append(observation)
    meter = {"work": 0, "trials": 0, "unknown": 0, "exhausted": 0}
    co_observers = _co_observers(problem)
    attributions: dict[str, Attribution] = {}
    for proof in solution.proofs:
        touched = {sides[node] for node in (*proof.observations, *proof.groups)}
        if touched == {effects.REFERENCE}:
            attributions[proof.id] = Attribution.REFERENCE
        else:
            attributions[proof.id] = _attribute_proof(
                problem,
                proof,
                serving_paths,
                alternatives,
                co_observers,
                meter,
            )
    found: dict[effects.ObligationRef, tuple[Attribution, str]] = {}
    for ref, outcome in solution.outcomes.items():
        if outcome.proofs:
            proof = max(outcome.proofs, key=lambda p: _SEVERITY[attributions[p]])
            found[ref] = (attributions[proof], proof)
    counters = {f"attribution_{name}": value for name, value in meter.items() if value}
    return found, counters


def _cited_proofs(
    results: Sequence[PropertyResult], solution: solver.Solution
) -> tuple[effects.ProofCertificate, ...]:
    """Return each proof some result cites, listing exactly the obligations citing it."""
    citing: dict[str, list[effects.ObligationRef]] = {}
    for result in results:
        for obligation in result.obligations:
            if obligation.proof is not None:
                citing.setdefault(obligation.proof, []).append(
                    effects.ObligationRef(obligation.rule_id, obligation.scope)
                )
    return tuple(
        dataclasses.replace(
            proof,
            obligations=tuple(
                sorted(citing[proof.id], key=effects.ObligationRef.document)
            ),
        )
        for proof in solution.proofs
        if proof.id in citing
    )


@dataclass(frozen=True)
class Evaluation:
    """Every property result, deterministic counters, merged solver inputs, and cited proofs."""

    results: tuple[PropertyResult, ...]
    work: Work
    nodes: _Nodes
    proofs: tuple[effects.ProofCertificate, ...]


def evaluate(
    properties: Sequence[PropertyDef],
    check_input: CheckInput,
    *,
    target_error: PropertyError | None = None,
    serving_paths: Mapping[str, str] | None = None,
) -> Evaluation:
    """Evaluate every property independently, then decide their inputs with the engine.

    A target property does not run when `target_error` says its effects
    could not be derived. Each property
    records into its own node table. A contract violation, an exception
    escaping a collector, an unresolved obligation, a result that
    disagrees with its declarations, or an observation that conflicts
    with another property's (`merge_nodes`) yields an incomplete result
    that keeps every obligation resolved before it, and none of that
    property's nodes or constraints; siblings still run. The consistency
    engine then decides every obligation of the complete properties from
    the merged inputs (`engine_problem`). Results do not depend on
    property order beyond their own order. When the collectors declared
    more than `MAX_RUN_OBLIGATIONS` obligations in all, the engine decides
    nothing: direct results stand and every obligation that needs the
    engine is not evaluated (`core.solver.unavailable`). `serving_paths` maps observation
    channels to their serving paths for attributing engine proofs.
    """
    finished: dict[str, tuple[CollectorRecorder, PropertyError | None]] = {}
    tables: dict[str, _Nodes] = {}
    obligations = 0
    for definition in properties:
        if target_error is not None and not is_core(definition.id):
            continue
        recorder, table, error = _evaluate_one(definition, check_input, serving_paths)
        finished[definition.id] = (recorder, error)
        if error is None:
            tables[definition.id] = table
        obligations += recorder.counters()["obligations"]
    nodes, conflicts = merge_nodes(check_input.effects, tables)
    healthy = frozenset(
        ref
        for pid, (recorder, error) in finished.items()
        if error is None and pid not in conflicts and obligations <= MAX_RUN_OBLIGATIONS
        for ref in recorder.declared()
    )
    problem, excluded = engine_problem(check_input.effects, nodes, healthy)
    solution = _certified(problem, solver.solve(problem, account=check_input.work))
    outcomes = _with_excluded(solution, excluded)
    attributions, attribution_work = _engine_attributions(
        problem, solution, serving_paths or {}
    )
    results: list[PropertyResult] = []
    for definition in properties:
        if definition.id not in finished:
            results.append(PropertyResult(definition.id, error=target_error))
            continue
        recorder, error = finished[definition.id]
        recorder.attribute(attributions)
        results.append(
            _finish(
                definition,
                recorder,
                error or conflicts.get(definition.id),
                outcomes,
            )
        )
    phases = {name: dict(counters) for name, counters in solution.phases.items()}
    construction = phases["construction"]
    construction["obligations"] = obligations
    construction["groups"] = len(check_input.effects.groups)
    construction["observations"] = len(nodes.observations)
    construction["constraints"] = len(nodes.constraints)
    phases["construction"] = dict(sorted(construction.items()))
    phases["explanation"] = dict(
        sorted({**phases["explanation"], **attribution_work}.items())
    )
    work = Work(phases=phases, components=solution.components)
    return Evaluation(tuple(results), work, nodes, _cited_proofs(results, solution))


def evaluate_with_nodes(
    properties: Sequence[PropertyDef],
    check_input: CheckInput,
    *,
    target_error: PropertyError | None = None,
    serving_paths: Mapping[str, str] | None = None,
) -> tuple[tuple[PropertyResult, ...], Work, _Nodes]:
    """Return `evaluate`'s results, counters, and merged solver inputs."""
    found = evaluate(
        properties,
        check_input,
        target_error=target_error,
        serving_paths=serving_paths,
    )
    return found.results, found.work, found.nodes


def evaluate_properties(
    properties: Sequence[PropertyDef],
    check_input: CheckInput,
    *,
    target_error: PropertyError | None = None,
    serving_paths: Mapping[str, str] | None = None,
) -> tuple[tuple[PropertyResult, ...], Work]:
    """Return `evaluate`'s results and deterministic counters."""
    found = evaluate(
        properties,
        check_input,
        target_error=target_error,
        serving_paths=serving_paths,
    )
    return found.results, found.work


def _collect_daemon_exit(check_input: CheckInput, sink: Any) -> None:
    """Find the first daemon exit reported before cleanup; a daemon exits once."""
    ref = sink.obligation(CORE_DAEMON_EXIT, _RUN_SCOPE, subject=DAEMON_SUBJECT)
    facts = check_input.facts
    for event in facts.run_events:
        if (
            event.type != INFO
            or event.data.get("reason") != DAEMON_EXIT
            or event.time >= facts.cleanup_time
        ):
            continue
        returncode = event.data.get("returncode")
        signal = event.data.get("signal")
        detail = f"exit {returncode}"
        if signal:
            detail += f" ({signal})"
        sink.violated(
            ref,
            Attribution.PRODUCT,
            f"daemon/crashed: {detail} at t={event.time}; no nemesis window",
            (
                Witness("exit-class", f"signal:{signal}" if signal else "exit"),
                Witness("returncode", str(returncode)),
                Witness("time", repr(event.time)),
            ),
        )
        return
    sink.satisfied(ref)


CORE_ACTIVITY_INCOMPLETE = f"{CORE_ACTIVITY}.incomplete"
CORE_INPUT_UNDECODED = f"{CORE_INPUT_LEGALITY}.undecoded"
CORE_INPUT_STATE_UNOBSERVED = f"{CORE_INPUT_LEGALITY}.state-unobserved"
_COMPLETED_EXECUTION = "completed"


def _modeled(target: Target, kind: str) -> bool:
    definition = target.actions.get(kind)
    return definition is not None and definition.modeled


def _exercised(target: Target, kind: str) -> bool:
    """Return whether completing an action of `kind` exercises the target.

    A modeled kind does, and so does any kind whose static roles mutate
    state (its effects are derived), modeled input or not.
    """
    definition = target.actions.get(kind)
    return definition is not None and (
        definition.modeled or bool(definition.roles & MUTATING_ROLES)
    )


def _collect_activity(
    target: Target, check_input: CheckInput, sink: Any, execution_status: str | None
) -> None:
    """Require one operation that exercises the target (`_exercised`) and completed ok or fail.

    An `info` completion (an operation the sweep or a watchdog ended) does
    not show the target ran it, nor that it did not. Such a completion, or
    an execution known not to have completed (whose history may lack
    completions), cannot show that none happened: the obligation is not
    evaluated instead of violated.
    """
    ref = sink.obligation(CORE_ACTIVITY, _RUN_SCOPE, subject=CHECKOUT_SUBJECT)
    actions = [
        f for f in check_input.facts.actions.values() if _exercised(target, f.kind)
    ]
    if any(f.outcome in (OK, FAIL) for f in actions):
        sink.satisfied(ref)
        return
    if execution_status is not None and execution_status != _COMPLETED_EXECUTION:
        sink.not_evaluated(
            ref, CORE_ACTIVITY_INCOMPLETE, f"execution {execution_status}"
        )
        return
    if any(f.outcome == INFO for f in actions):
        sink.not_evaluated(ref, CORE_ACTIVITY_INCOMPLETE, "only info completions")
        return
    sink.violated(
        ref,
        Attribution.UNDETERMINED,
        "no-modeled-operations/history≠spec: no modeled action completed in history",
    )


def _overlaps(span: tuple[float, float], window: tuple[float, float]) -> bool:
    return span[0] <= window[1] and window[0] <= span[1]


def _removal_started(facts: HistoryFacts, fault_id: Any) -> float:
    """Return when the first `fault_remove` of a fault was invoked, or never."""
    return min(
        (
            remove.invoked
            for remove in facts.actions_of("fault_remove")
            if remove.payload.get("fault_id") == fault_id and remove.invoked is not None
        ),
        default=float("inf"),
    )


def _timeout_excused(
    target: Target,
    check_input: CheckInput,
    action: Action,
    install: Action,
    span: tuple[float, float],
) -> bool:
    """Excuse ETIMEDOUT only on a timeout-capable channel, for a witnessed fault,
    when the operation completed before that fault's removal was invoked."""
    spec, facts = check_input.spec, check_input.facts
    removal = _removal_started(facts, install.payload.get("fault_id"))
    return (
        target.timeout_capable is not None
        and target.timeout_capable(spec, facts, action)
        and target.install_witnessed(spec, facts, install)
        and span[1] < removal
    )


def _killed(facts: HistoryFacts, action_id: str, span: tuple[float, float]) -> bool:
    return any(
        kill.payload.get("target_action") == action_id
        and kill.interval is not None
        and _overlaps(span, kill.interval)
        for kill in facts.actions_of("kill_process")
    )


def targeting_faults(facts: HistoryFacts, action_id: str) -> list[Action]:
    """Return the fault installs targeting an action whose windows overlap its interval."""
    span = facts.action(action_id).interval
    if span is None:
        return []
    return [
        install
        for install, window in facts.fault_windows()
        if install.payload.get("target_action") == action_id and _overlaps(span, window)
    ]


def fault_excuses(behavior: object, use: ErrnoUse | None, name: str) -> bool:
    """Return whether a fault with this behavior can cause its target's failure.

    An `error` fault (an injected `runtime_error`) surfaces as FUSE EIO, an
    errno-less Thrift EdenError, or a failed Sapling command: it can cause
    a FAULT, undeclared, or errno-less failure, never a named state errno
    or ETIMEDOUT. `block` and `delay` fail nothing themselves: they can
    cause only a declared ETIMEDOUT, which `_timeout_excused` must also
    excuse.
    """
    if name == errno.errorcode[errno.ETIMEDOUT]:
        return use == ErrnoUse.TIMEOUT and behavior in ("block", "delay")
    return behavior == "error" and (
        use in (None, ErrnoUse.FAULT) or name == NO_FAILURE_ERRNO
    )


def _nemesis_caused(
    target: Target,
    check_input: CheckInput,
    action: Action,
    use: ErrnoUse | None,
    name: str,
) -> bool:
    """Excuse a failure a targeting fault can cause (`fault_excuses`), or a kill.

    No fault excuses a failure of an action it does not target. A kill of
    the action excuses any failure.
    """
    facts = check_input.facts
    span = facts.action(action.id).interval
    assert span is not None
    for install in targeting_faults(facts, action.id):
        if fault_excuses(install.payload.get("behavior"), use, name) and (
            use != ErrnoUse.TIMEOUT
            or _timeout_excused(target, check_input, action, install, span)
        ):
            return True
    return _killed(facts, action.id, span)


def _input_failure_attribution(facts: HistoryFacts, action: ActionFacts) -> Attribution:
    """Attribute an unexcused input failure: product, unless it is an ETIMEDOUT
    overlapping a fault window that targets another action.

    A fault parked on another request can delay this one past its timeout,
    so such a timeout is not evidence against the product by itself.
    """
    span = action.interval
    if span is None or action.result is None or action.result.errno != errno.ETIMEDOUT:
        return Attribution.PRODUCT
    unrelated = any(
        install.payload.get("target_action") != action.id and _overlaps(span, window)
        for install, window in facts.fault_windows()
    )
    return Attribution.UNDETERMINED if unrelated else Attribution.PRODUCT


def errno_name(code: int | None) -> str:
    """Return an errno's symbolic name, its number when unnamed, or `none`."""
    if code is None:
        return "none"
    return errno.errorcode.get(code, str(code))


def failure_use(
    schema: ResultSchema, result: TypedResult
) -> tuple[str, ErrnoUse | None]:
    """Return a failure's name and its declared use, None when undeclared.

    The name is its errno's; an errno-less failure of a schema with
    error types is named and judged by its `error_type` instead, so an
    undeclared type is judged like an undeclared errno.
    """
    error_type = result.fields.get(ERROR_TYPE_FIELD)
    if result.errno is None and schema.error_types and type(error_type) is str:
        return error_type, schema.error_types.get(error_type)
    name = errno_name(result.errno)
    return name, schema.errnos.get(name)


ROOT_PATH_SUBJECT = path_subject(effects.ROOT)


class TargetContractError(ValueError):
    """A target's declarations cannot answer what the core needs for one action."""


def input_subject(target: Target, spec: RunSpec, action: Action) -> Subject:
    """Return the PATH subject `core.input-legality` keys one input by.

    It is the action's declared `input_subject` path; without one, the
    single path its footprint names, or ``path:.`` when the footprint
    names ROOT. A footprint naming no path or several paths without a
    declared `input_subject`, or a declared subject that is not a
    canonical path, raises TargetContractError: unrelated inputs never
    share a subject.
    """
    definition = target.actions[action.kind]
    if definition.input_subject is not None:
        path = definition.input_subject(spec, action)
        if not effects.is_canonical_key(path):
            raise TargetContractError(
                f"{action.kind}: input_subject returned {path!r:.200}, "
                "not a canonical path"
            )
        return path_subject(path)
    paths = definition.footprint(spec, action).paths()
    if effects.ROOT in paths:
        return ROOT_PATH_SUBJECT
    if len(paths) != 1:
        raise TargetContractError(
            f"{action.kind}: the footprint of {action.id!r:.200} names "
            f"{len(paths)} paths and the action declares no input_subject"
        )
    return path_subject(next(iter(paths)))


# The `errno` identity value of an input that did not fail with a decoded result.
NO_FAILURE_ERRNO = "none"


def _judge_failure(
    target: Target,
    check_input: CheckInput,
    action: Action,
    schema: ResultSchema,
    ref: effects.ObligationRef,
    sink: Any,
) -> bool:
    """Resolve a decoded input failure, or return False when it is a violation.

    A nemesis-caused failure is legal. A state errno is legal only in the
    states that allow it: the obligation stays open for a derived
    observation, and without one it is not evaluated.
    """
    result = check_input.facts.action(action.id).result
    assert result is not None
    name, use = failure_use(schema, result)
    if _nemesis_caused(target, check_input, action, use, name):
        sink.satisfied(ref)
        return True
    if use != ErrnoUse.STATE:
        return False
    if not check_input.effects.supports(ref):
        sink.not_evaluated(
            ref,
            CORE_INPUT_STATE_UNOBSERVED,
            f"{action.id} failed with {name}",
        )
    return True


def _collect_input_legality(target: Target, check_input: CheckInput, sink: Any) -> None:
    """Judge every completed modeled input; violate each failure nothing excuses.

    Each input's obligation is on its `input_subject`. A failure a nemesis
    caused is legal. A state errno is legal only in some states, so it is
    never satisfied here: an observation the target's contracts hook records
    for this obligation decides it, and without one it is not evaluated
    (`core.input-legality.state-unobserved`). A fault or undeclared errno
    is legal only when a nemesis caused it, and ETIMEDOUT only under the
    declared timeout excuse. An unexcused failure is a product violation,
    except an ETIMEDOUT overlapping an unrelated fault, which is
    undetermined. Unmodeled inputs are not obligations. The `errno`
    identity role is declared with the obligation, so an engine violation
    of a state errno carries it too; an input that did not fail with a
    decoded result names `none`.
    """
    facts = check_input.facts
    for action in check_input.spec.actions:
        definition = target.actions.get(action.kind)
        recorded = facts.action(action.id)
        if (
            definition is None
            or definition.axis != "input"
            or not definition.modeled
            or recorded.interval is None
        ):
            continue
        errno_value = (
            failure_use(definition.result, recorded.result)[0]
            if recorded.outcome == FAIL and recorded.result is not None
            else NO_FAILURE_ERRNO
        )
        ref = sink.obligation(
            CORE_INPUT_LEGALITY,
            action.id,
            subject=input_subject(target, check_input.spec, action),
            witnesses=(
                Witness("input-kind", action.kind),
                Witness("errno", errno_value),
            ),
        )
        if recorded.outcome != FAIL:
            sink.satisfied(ref)
        elif recorded.result is None:
            sink.not_evaluated(ref, CORE_INPUT_UNDECODED, action.id)
        elif not _judge_failure(
            target, check_input, action, definition.result, ref, sink
        ):
            sink.violated(
                ref,
                _input_failure_attribution(facts, recorded),
                f"input/{action.kind}-failed: {action.id} failed and neither a "
                "nemesis nor its declared errnos excuse it",
                (Witness("action", action.id),),
            )


def _schema_violation(
    sink: Any, ref: Any, recorded: ActionFacts, attribution: Attribution
) -> None:
    problem = recorded.problems[0][:1024]
    side = "reference " if recorded.side == "reference" else ""
    sink.violated(
        ref,
        attribution,
        f"result-schema/{recorded.kind}≠schema: {recorded.id} {side}"
        f"{recorded.outcome}: {problem}",
        (
            Witness("action", recorded.id),
            Witness("side", recorded.side),
            Witness("outcome", str(recorded.outcome)),
            Witness("problem", problem),
        ),
    )


def _collect_result_schema(target: Target, check_input: CheckInput, sink: Any) -> None:
    """Violate every modeled result, and every reference side, that did not decode.

    A primary result that did not decode is the recorder's fault
    (harness). A reference side that did not decode, ran outside its
    primary's span, or is missing from an action that owes one (the
    target's `expects_reference`) is the reference's. A side on an action
    that owes none is the harness's, whether or not the kind is modeled:
    no runtime should have run it. One obligation per action covers both
    sides, and the more severe attribution is kept.
    """
    for recorded in check_input.facts.actions.values():
        reference = recorded.reference
        modeled = _modeled(target, recorded.kind) and recorded.completed is not None
        if not modeled and reference is None:
            continue
        ref = sink.obligation(
            CORE_RESULT_SCHEMA,
            recorded.id,
            subject=CHECKOUT_SUBJECT,
            witnesses=(Witness("action-kind", recorded.kind),),
        )
        primary_bad = modeled and bool(recorded.problems)
        reference_bad = reference is not None and bool(reference.problems)
        if primary_bad:
            _schema_violation(sink, ref, recorded, Attribution.HARNESS)
        if reference_bad:
            assert reference is not None
            unexpected = reference.problems == (UNEXPECTED_REFERENCE,)
            _schema_violation(
                sink,
                ref,
                reference,
                Attribution.HARNESS if unexpected else Attribution.REFERENCE,
            )
        if not primary_bad and not reference_bad:
            sink.satisfied(ref)


def core_properties(
    target: Target, execution_status: str | None = None
) -> tuple[PropertyDef, ...]:
    """Return the core properties every target shares, in evaluation order.

    `execution_status` is the containment status when known; only
    `core.activity` reads it.
    """

    def activity(check_input: CheckInput, sink: Any) -> None:
        _collect_activity(target, check_input, sink, execution_status)

    def input_legality(check_input: CheckInput, sink: Any) -> None:
        _collect_input_legality(target, check_input, sink)

    def result_schema(check_input: CheckInput, sink: Any) -> None:
        _collect_result_schema(target, check_input, sink)

    return (
        PropertyDef(
            id=CORE_DAEMON_EXIT,
            collect=_collect_daemon_exit,
            rules={CORE_DAEMON_EXIT: CORE_RULES[CORE_DAEMON_EXIT]},
        ),
        PropertyDef(
            id=CORE_ACTIVITY,
            collect=activity,
            rules={CORE_ACTIVITY: CORE_RULES[CORE_ACTIVITY]},
            not_evaluated_reasons={
                CORE_ACTIVITY_INCOMPLETE: (
                    "The execution did not complete and no completed operation "
                    "exercised the target, so its history cannot show that none would"
                ),
            },
        ),
        PropertyDef(
            id=CORE_INPUT_LEGALITY,
            collect=input_legality,
            rules={CORE_INPUT_LEGALITY: CORE_RULES[CORE_INPUT_LEGALITY]},
            not_evaluated_reasons={
                CORE_INPUT_UNDECODED: (
                    "The failed input's result did not decode, so its errno is unknown"
                ),
                CORE_INPUT_STATE_UNOBSERVED: (
                    "The input failed with a state errno and the target's contracts recorded no "
                    "observation of the state that would allow it"
                ),
            },
        ),
        PropertyDef(
            id=CORE_RESULT_SCHEMA,
            collect=result_schema,
            rules={CORE_RESULT_SCHEMA: CORE_RULES[CORE_RESULT_SCHEMA]},
        ),
    )


def registry(
    target: Target, execution_status: str | None = None
) -> tuple[PropertyDef, ...]:
    """Return the validated core plus target registry, in evaluation order."""
    properties = (*core_properties(target, execution_status), *target.properties)
    validate_registry(properties, target.red_controls)
    return properties


def _canonical(value: Any) -> bytes:
    try:
        return jsonio.canonical(value, default=_label)
    except (TypeError, ValueError, RecursionError):
        return jsonio.canonical(_total_json(value))


def _label(value: Any) -> str:
    """Name a value JSON cannot represent by its type, deterministically."""
    return f"<{type_name(value)}>"


def _total_json(value: Any, depth: int = 0) -> Any:
    """Return a JSON value for any input, deterministically, for digesting malformed input.

    Ints are written in hex past 64 bits (decimal text of an enormous int
    can exceed the interpreter's limit), dicts become key-sorted pairs
    (their keys need not be strings or mutually comparable), dataclasses
    become their fields, and anything else is labeled by its type.
    """
    if depth > 64:
        return "<too deep>"
    if value is None or type(value) in (bool, str, float):
        return value
    if type(value) is int:
        return value if value.bit_length() <= 64 else {"$int": hex(value)}
    if type(value) is dict:
        pairs = [
            [_total_json(key, depth + 1), _total_json(item, depth + 1)]
            for key, item in value.items()
        ]
        return {"$map": sorted(pairs, key=lambda pair: _canonical(pair[0]))}
    if type(value) in (list, tuple):
        return [_total_json(item, depth + 1) for item in value]
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return {
            "$" + type_name(value): {
                field.name: _total_json(getattr(value, field.name), depth + 1)
                for field in dataclasses.fields(value)
            }
        }
    return _label(value)


_CORE_SOURCES = tuple(
    f"{__package__}.{name}"
    for name in (
        "actions",
        "checker",
        "effects",
        "facts",
        "history",
        "identity",
        "runspec",
        "sources",
        "target",
    )
)


def _defining_module(item: Any) -> str:
    """Return the module whose source defines a callable, through wrappers.

    A partial is its function only when it binds no arguments: bound
    values are not source the digest can attest.
    """
    if isinstance(item, functools.partial):
        if item.args or item.keywords:
            raise CheckerSourceUnavailable(f"{item!r:.300} binds arguments")
        return _defining_module(item.func)
    function = getattr(item, "__func__", item)
    unwrapped = inspect.unwrap(function) if callable(function) else function
    module_name = getattr(unwrapped, "__module__", None)
    if type(module_name) is not str:
        raise CheckerSourceUnavailable(f"{item!r:.300} names no defining module")
    return module_name


def source_policy(target: Target | None = None) -> SourcePolicy:
    """Return which imports the semantic digest follows and which are environment."""
    if target is None or target.sources is None:
        return CHECKER_POLICY
    return SourcePolicy(
        follow=(SOURCE_NAMESPACE, *target.sources.packages),
        external=target.sources.external,
        dynamic=target.sources.dynamic,
    )


def _target_callables(target: Target) -> list[Any]:
    callables: list[Any] = [
        target.install_witnessed,
        target.timeout_capable,
        target.contracts,
        target.pinned,
        target.expects_reference,
    ]
    for definition in target.properties:
        callables.append(definition.collect)
    for action in target.actions.values():
        callables += [action.footprint, action.input_subject]
    return [item for item in callables if item is not None]


def source_roots(target: Target | None = None) -> frozenset[str]:
    """Return the modules a semantic digest starts from.

    These are the core checker, facts, effects, identity, history,
    RunSpec, target-interface, source-closure, and core action-table
    modules; with a target, its declared implementation and behavior
    modules and the module defining each of its predicates, effect hooks,
    collectors, footprints, and input subjects. Raises CheckerSourceUnavailable for
    a target that declares no `TargetSources`, a partial that binds
    arguments, or a callable that names no defining module.
    """
    roots = set(_CORE_SOURCES)
    if target is None:
        return frozenset(roots)
    if target.sources is None:
        raise CheckerSourceUnavailable(f"target {target.name!r} declares no sources")
    validate_target_sources(target.sources)
    roots.update((target.sources.implementation, *target.sources.modules))
    roots.update(_defining_module(item) for item in _target_callables(target))
    return frozenset(roots)


def checker_sources(
    target: Target | None = None, *, reader: SourceReader = read_module_source
) -> Mapping[str, str]:
    """Return the source digest of every module that can change a verdict, by name.

    The modules are `source_roots(target)` and every module their static
    imports reach under `source_policy(target)`. Raises
    CheckerSourceUnavailable when any of them has no readable source, an
    import is undeclared, or the target's declaration is missing or
    invalid, so a verdict never binds a weakened digest.
    """
    try:
        roots = source_roots(target)
    except ValueError as error:
        raise CheckerSourceUnavailable(f"invalid target sources: {error}") from error
    return source_closure(roots, policy=source_policy(target), reader=reader)


def action_declaration(definition: ActionDef) -> dict[str, Any]:
    """Return the canonical document of one action's static declarations."""
    return {
        "axis": definition.axis,
        "modeled": definition.modeled,
        "result": definition.result.document(),
        "roles": sorted(str(role) for role in definition.roles),
        "binding": str(definition.binding),
        "grouping": str(definition.grouping),
        "input_subject": definition.input_subject is not None,
    }


def semantic_digest(
    target: Target | None = None, *, reader: SourceReader = read_module_source
) -> str:
    """Digest everything that can change a verdict except the judged documents.

    That is the checker version, `checker_sources(target)` and its import
    policy, every action's static declarations, and the engine budgets.
    Module names and their docstring-free syntax trees are the only file
    inputs (`sources.semantic_text`), so the digest is the same in every
    checkout under one Python minor version, and comments, formatting, and
    docstrings never change it. The syntax tree's form can differ between
    Python versions, so an interpreter upgrade can rebind every digest.
    """
    sources = dict(checker_sources(target, reader=reader))
    policy = source_policy(target)
    actions = {} if target is None else target.actions
    document = {
        "version": CHECKER_VERSION,
        "sources": sources,
        "follow": list(policy.follow),
        "external": list(policy.external),
        "dynamic": list(policy.dynamic),
        "actions": {
            kind: action_declaration(definition)
            for kind, definition in sorted(actions.items())
        },
        "budgets": dict(SOLVER_BUDGETS),
    }
    return jsonio.sha256(_canonical(document))


def bind(
    spec: RunSpec,
    events: Sequence[Event],
    execution: Mapping[str, Any] | None,
    *,
    target: Target | None = None,
) -> Bindings:
    """Digest the canonical RunSpec, history JSONL, semantics, and execution.

    A valid spec is digested as its `to_json` text and a well-formed event
    as its canonical line. Malformed input never raises: an invalid spec
    or event is digested by a total encoding, and a non-dict execution by
    its type label, under a marker key no valid document has, so it never
    shares a digest with valid input.
    Unreadable checker source raises CheckerSourceUnavailable.
    """
    history = b"".join(_event_bytes(event) + b"\n" for event in events)
    if spec_problems(spec):
        runspec = _canonical({"$invalid-runspec": _total_json(spec)})
    else:
        runspec = to_json(spec).encode("utf-8", errors="surrogatepass")
    if execution is None:
        execution_digest = None
    elif type(execution) is dict:
        execution_digest = jsonio.sha256(_canonical(execution))
    else:
        execution_digest = jsonio.sha256(
            _canonical({"$invalid-execution": _label(execution)})
        )
    return Bindings(
        runspec=jsonio.sha256(runspec),
        history=jsonio.sha256(history),
        semantic=semantic_digest(target),
        execution=execution_digest,
        semantics_profile=(
            DEFAULT_SEMANTICS_PROFILE if target is None else target.semantics_profile
        ),
    )


def _event_bytes(event: Any) -> bytes:
    if (
        type(event) is Event
        and type(event.data) is dict
        and RESERVED_DATA_KEYS.isdisjoint(event.data)
    ):
        return _canonical(event_to_json(event))
    if type(event) is Event:
        body = [
            _total_json(event.type),
            _total_json(event.time),
            _total_json(event.data),
        ]
    else:
        body = [_label(event)]
    return _canonical({"$invalid-event": body})


def _violations(result: PropertyResult) -> Iterator[ObligationResult]:
    return (o for o in result.obligations if o.status == ObligationStatus.VIOLATED)


def lane_kind(result: PropertyResult) -> str:
    """Project one property result onto the verdict kinds."""
    attributions = {o.attribution for o in _violations(result)}
    if attributions & PRODUCT_ATTRIBUTIONS:
        return REJECT
    if attributions:
        return HARNESS_SUSPECT
    if result.error is not None or not result.obligations:
        return NOT_EVALUATED
    if any(o.status == ObligationStatus.NOT_EVALUATED for o in result.obligations):
        return NOT_EVALUATED
    return ACCEPT


def lanes(results: Sequence[PropertyResult]) -> dict[str, str]:
    """Return each target property's projected kind; core properties have no lane."""
    return {r.property_id: lane_kind(r) for r in results if not is_core(r.property_id)}


def _error_type_label(error: PropertyError) -> str:
    """Name an error type in a reason; an oversize type is named by its digest."""
    encoded = error.type.encode("utf-8", errors="surrogatepass")
    if len(encoded) <= MAX_ID_BYTES:
        return error.type
    return f"type:{jsonio.sha256(encoded)}"


def _first_violation(
    results: Sequence[PropertyResult], product: bool
) -> ObligationResult | None:
    for result in results:
        for obligation in _violations(result):
            if (obligation.attribution in PRODUCT_ATTRIBUTIONS) == product:
                return obligation
    return None


def _first_unevaluated(results: Sequence[PropertyResult]) -> ObligationResult | None:
    for result in results:
        for obligation in result.obligations:
            if obligation.status == ObligationStatus.NOT_EVALUATED:
                return obligation
    return None


def project(results: Sequence[PropertyResult]) -> tuple[str, str, str | None]:
    """Return the verdict kind, reason, and not-evaluated cause.

    A violated product or undetermined obligation rejects; else a violated
    non-product obligation is harness-suspect; else a property error, then
    any not-evaluated obligation, is not-evaluated; else a run whose target
    properties declared no obligation is not evaluated; else it accepts.
    Each reason is the first at its rank in property, rule, scope order.
    """
    for product, kind in ((True, REJECT), (False, HARNESS_SUSPECT)):
        violation = _first_violation(results, product)
        if violation is not None:
            assert violation.reason is not None
            return kind, violation.reason, None
    for result in results:
        if result.error is not None:
            marker = " (oversize)" if result.error.oversize else ""
            return (
                NOT_EVALUATED,
                f"property-error/{result.property_id}≠checker: "
                f"{_error_type_label(result.error)}{marker}",
                PROPERTY_ERROR,
            )
    unevaluated = _first_unevaluated(results)
    if unevaluated is not None:
        detail = f": {unevaluated.reason}" if unevaluated.reason else ""
        return (
            NOT_EVALUATED,
            f"blocked/{unevaluated.not_evaluated_reason}{detail}",
            BLOCKED,
        )
    if not any(r.obligations for r in results if not is_core(r.property_id)):
        return NOT_EVALUATED, _NO_OBLIGATION_REASON, NO_EVALUATION
    return ACCEPT, _ALL_ACCEPT, None


_GATE_PREFIXES = {
    INVALID_HISTORY: "invalid-history/history≠spec",
    INVALID_RUNSPEC: "invalid-runspec/spec≠bounds",
}


def gate_reason(cause: str, problems: Sequence[str]) -> str:
    """Return the reason for a structural gate: its first problem and how many more."""
    more = f" (+{len(problems) - 1} more)" if len(problems) > 1 else ""
    return f"{_GATE_PREFIXES[cause]}: {problems[0]}{more}"


def _gated(
    cause: str, problems: Sequence[str], digest: str, bindings: Bindings
) -> Verdict:
    kept = tuple(problems)
    return Verdict(
        NOT_EVALUATED,
        gate_reason(cause, kept),
        not_evaluated_cause=cause,
        report=PropertyReport((), digest, bindings, kept),
    )


def execution_problems(execution: Any) -> tuple[str, ...]:
    """Return why an execution document cannot decide history completeness.

    None means the execution is unknown. A document must be a dict whose
    ``status`` is non-empty text of at most MAX_ID_BYTES UTF-8 bytes.
    """
    if execution is None:
        return ()
    if type(execution) is not dict:
        return (f"execution must be an object, got {type_name(execution)}",)
    status = execution.get("status")
    if type(status) is not str or not status:
        return (f"execution status must be text, got {value_label(status)}",)
    if len(status.encode("utf-8", errors="surrogatepass")) > MAX_ID_BYTES:
        return (f"execution status exceeds {MAX_ID_BYTES} UTF-8 bytes",)
    return ()


class _ContractRecorder:
    """The `ContractSink` a target's effect contracts hook records into.

    It records groups, observations, and constraints.

    Each node names an invoked spec action and a window inside its
    resolution: no earlier than the invocation, and for an acked group or
    an observation no later than the completion. Every obligation a
    contract observation, requirement, or constraint supports names a rule
    of the registry. A requirement never supports a `core.input-legality`
    obligation, and an observation supports one only when it observes the
    very input it judges. Contract constraints
    name contract nodes only; their endpoints are checked once every node
    is recorded.
    """

    def __init__(self, facts: HistoryFacts, rule_ids: frozenset[str]) -> None:
        self._facts = facts
        self._rule_ids = rule_ids
        self.groups: list[effects.EffectGroup] = []
        self.observations: list[effects.Observation] = []
        self.constraints: list[effects.Constraint] = []

    def _check_refs(self, refs: Iterable[effects.ObligationRef]) -> None:
        for ref in refs:
            if ref.rule_id not in self._rule_ids:
                raise ValueError(f"contract input names unknown rule {ref.rule_id!r}")

    def group(self, group: effects.EffectGroup) -> None:
        """Record one validated effect group."""
        if not isinstance(group, effects.EffectGroup):
            raise ValueError(f"{type_name(group)} is not an EffectGroup")
        _check_node_action(
            self._facts,
            group.action,
            group.window,
            completes=group.status == effects.Status.ACKED,
        )
        self._check_refs(item.obligation for item in group.requires)
        for item in group.requires:
            if item.obligation.rule_id == CORE_INPUT_LEGALITY:
                raise ValueError(
                    f"requirement of group {group.id!r:.200} supports "
                    f"{item.obligation}; only an observation of that input can"
                )
        self.groups.append(group)

    def observe(self, observation: effects.Observation) -> None:
        """Record one validated contract observation."""
        if not isinstance(observation, effects.Observation):
            raise ValueError(f"{type_name(observation)} is not an Observation")
        _check_node_action(
            self._facts, observation.action, observation.window, completes=True
        )
        self._check_refs(observation.obligations)
        for ref in observation.obligations:
            if ref.rule_id == CORE_INPUT_LEGALITY and ref.scope != observation.action:
                raise ValueError(
                    f"observation {observation.id!r:.200} of {observation.action!r:.200} "
                    f"supports {ref}, which judges another input"
                )
        self.observations.append(observation)

    def constrain(self, constraint: effects.Constraint) -> None:
        """Record one constraint; its endpoints are checked in `effect_set`."""
        if not isinstance(constraint, effects.CONSTRAINT_TYPES):
            raise ValueError(f"{type_name(constraint)} is not a Constraint")
        self._check_refs((constraint.obligation,))
        self.constraints.append(constraint)

    def effect_set(self, pinned: effects.PinnedFacts | None) -> effects.EffectSet:
        """Return the recorded effects; every contract constraint must fit its endpoints."""
        effect_set = effects.EffectSet(
            tuple(self.groups),
            pinned,
            observations=tuple(self.observations),
            constraints=tuple(self.constraints),
        )
        nodes = _Nodes(effect_set)
        for constraint in self.constraints:
            nodes.constrain(constraint)
        return effect_set


def contract_effects(
    target: Target,
    spec: RunSpec,
    facts: HistoryFacts,
    rule_ids: Iterable[str] = (),
) -> tuple[effects.PinnedFacts | None, effects.EffectSet] | PropertyError:
    """Return the target's pinned facts and contract effects, or the error that stopped them.

    `rule_ids` are the registry rules contract inputs may support.
    """
    try:
        pinned = None if target.pinned is None else target.pinned(spec)
        sink = _ContractRecorder(facts, frozenset(rule_ids))
        if target.contracts is not None:
            target.contracts(spec, facts, pinned, sink)
        return pinned, sink.effect_set(pinned)
    except Exception as raised:  # noqa: BLE001 - kept as a typed property error
        return property_error(
            "EffectContractError", f"{type_name(raised)}: {exception_text(raised)}"
        )


def _judge(
    target: Target,
    spec: RunSpec,
    events: Sequence[Event],
    properties: Sequence[PropertyDef],
    execution_status: str | None,
) -> tuple[tuple[PropertyResult, ...], Work, tuple[effects.ProofCertificate, ...]]:
    """Build the typed facts and effects once, then evaluate every property."""
    facts = HistoryFacts.build(
        spec, tuple(events), target, execution_status=execution_status
    )
    rule_ids = (rule for definition in properties for rule in definition.rules)
    derived = contract_effects(target, spec, facts, rule_ids)
    target_error = derived if isinstance(derived, PropertyError) else None
    pinned, effect_set = (
        (None, effects.EffectSet()) if isinstance(derived, PropertyError) else derived
    )
    check_input = CheckInput(
        spec, facts, pinned, effect_set, WorkAccount(solver.HISTORY_WORK)
    )
    found = evaluate(
        properties,
        check_input,
        target_error=target_error,
        serving_paths=target.serving_paths,
    )
    return found.results, found.work, found.proofs


def _defect_text() -> str:
    """Return the exception being handled with its frames, free of host paths.

    Each frame is named by its path under this package (or its file name)
    and line, so the problem text is the same on every host.
    """
    error = sys.exc_info()[1]
    assert error is not None
    frames = [
        f"{_frame_file(frame.filename)}:{frame.lineno} in {frame.name}"
        for frame in traceback.extract_tb(error.__traceback__)
    ]
    return "\n".join([*frames, *traceback.format_exception_only(error)]).rstrip()


def _frame_file(path: str) -> str:
    marker = "/exploratory/"
    return path.rsplit(marker, 1)[1] if marker in path else os.path.basename(path)


def check(
    target: Target,
    spec: RunSpec,
    events: Sequence[Event],
    *,
    execution: Mapping[str, Any] | None = None,
) -> Verdict:
    """Validate the history's structure, then evaluate core and target properties.

    The combined core and target registry must be valid. A structural
    problem is a typed not-evaluated verdict before any property runs. A
    target that declares no property is then not evaluated at all: it
    never receives a vacuous accept, and no core property judges the run
    on its behalf. Otherwise every core
    and target property is evaluated and kept; the verdict is the
    projection of their obligations. `execution` is the containment
    execution document, when known: it is bound by digest and its status
    decides whether every action must be resolved and whether the run
    reached the target.

    No spec, event, execution, or property output raises: a spec outside
    the RunSpec bounds gates as invalid-runspec, and a malformed execution
    document (not a dict, or a status that is not non-empty text of at
    most MAX_ID_BYTES bytes) as invalid-history, since it cannot decide
    completeness. Any other exception raised while checking the history
    or judging it is a checker defect: the run gates as invalid-history
    with the exception and its package-relative frames as its problem
    (`_defect_text`). It raises only when the target's registry is invalid
    or a verdict source cannot be read (`preflight` checks both before a
    run). Every gate
    problem is kept whole, however many there are: a verdict is never
    truncated, and one past the 2 MiB operational threshold is kept whole
    and reported as `execution.verdict-oversize` when it is read.
    """
    invalid_execution = execution_problems(execution)
    status = None if execution is None or invalid_execution else execution["status"]
    properties = registry(target, status)
    digest = identity_digest(properties, target.semantics_profile)
    bindings = bind(spec, events, execution, target=target)
    invalid_spec = spec_problems(spec)
    if invalid_spec:
        return _gated(INVALID_RUNSPEC, invalid_spec, digest, bindings)
    try:
        problems = (
            *invalid_execution,
            *history_problems(spec, events, execution_status=status),
        )
        if problems:
            return _gated(INVALID_HISTORY, problems, digest, bindings)
        if not target.properties:
            return Verdict(
                NOT_EVALUATED,
                NO_PROPERTY_REASON,
                not_evaluated_cause=NO_EVALUATION,
                report=PropertyReport((), digest, bindings),
            )
        results, work, proofs = _judge(target, spec, events, properties, status)
    except Exception:  # noqa: BLE001 - a checker defect is this run's typed verdict
        return _gated(INVALID_HISTORY, (_defect_text(),), digest, bindings)
    kind, reason, cause = project(results)
    return Verdict(
        kind,
        reason,
        lanes=lanes(results),
        not_evaluated_cause=cause,
        report=PropertyReport(results, digest, bindings, proofs=proofs, work=work),
    )


def _error_document(error: PropertyError | None) -> dict[str, Any] | None:
    if error is None:
        return None
    return {
        "type": error.type,
        "message": error.message,
        "message_bytes": error.message_bytes,
        "message_sha256": error.message_sha256,
        "oversize": error.oversize,
    }


_OPTIONAL_OBLIGATION_KEYS = (
    "attribution",
    "reason",
    "not_evaluated_reason",
    "proof",
    "component",
)


def _obligation_document(obligation: ObligationResult) -> dict[str, Any]:
    document: dict[str, Any] = {
        "rule_id": obligation.rule_id,
        "scope": obligation.scope,
        "subject": {
            "kind": str(obligation.subject.kind),
            "token": obligation.subject.token,
        },
        "status": str(obligation.status),
        "witnesses": [{"role": w.role, "value": w.value} for w in obligation.witnesses],
    }
    for key in _OPTIONAL_OBLIGATION_KEYS:
        value = getattr(obligation, key)
        if value is not None:
            document[key] = str(value)
    return document


def _result_document(result: PropertyResult) -> dict[str, Any]:
    return {
        "property_id": result.property_id,
        "error": _error_document(result.error),
        "coverage": dict(result.coverage),
        "obligations": [_obligation_document(o) for o in result.obligations],
    }


def verdict_document(verdict: Verdict) -> dict[str, Any]:
    """Return the canonical `verdict.json` v1 document; findings are never stored twice."""
    report = verdict.report
    if report is None:
        raise ValueError("verdict_document needs a verdict produced by check()")
    bindings = report.bindings
    return {
        "schema_version": VERDICT_SCHEMA_VERSION,
        "identity_digest": report.identity_digest,
        "bindings": {
            "runspec": bindings.runspec,
            "history": bindings.history,
            "semantic": bindings.semantic,
            "execution": bindings.execution,
            "semantics_profile": bindings.semantics_profile,
        },
        "kind": verdict.kind,
        "reason": verdict.reason,
        "not_evaluated_cause": verdict.not_evaluated_cause,
        "lanes": dict(verdict.lanes),
        "gate": {"problems": list(report.gate)} if report.gate else None,
        "properties": [_result_document(result) for result in report.results],
        "proofs": [proof.document() for proof in report.proofs],
        "work": {
            "phases": {k: dict(v) for k, v in report.work.phases.items()},
            "components": {k: dict(v) for k, v in report.work.components.items()},
        },
    }


def encode_verdict(verdict: Verdict) -> bytes:
    """Return the exact, ASCII-safe `verdict.json` bytes; nothing is ever truncated."""
    text = json.dumps(
        verdict_document(verdict),
        sort_keys=True,
        indent=2,
        ensure_ascii=True,
        allow_nan=False,
    )
    return (text + "\n").encode("ascii")


_TOP_KEYS = frozenset(
    {
        "schema_version",
        "identity_digest",
        "bindings",
        "kind",
        "reason",
        "not_evaluated_cause",
        "lanes",
        "gate",
        "properties",
        "proofs",
        "work",
    }
)
_BINDING_KEYS = frozenset(
    {"runspec", "history", "semantic", "execution", "semantics_profile"}
)
_RESULT_KEYS = frozenset({"property_id", "error", "coverage", "obligations"})
_OBLIGATION_KEYS = frozenset({"rule_id", "scope", "subject", "status", "witnesses"})
_ERROR_KEYS = frozenset(
    {"type", "message", "message_bytes", "message_sha256", "oversize"}
)


def _object(raw: Any, keys: frozenset[str], what: str) -> dict[str, Any]:
    if type(raw) is not dict or set(raw) != keys:
        found = sorted(raw) if type(raw) is dict else type(raw).__name__
        raise ValueError(f"{what} must have exactly {sorted(keys)}, got {found}")
    return raw


def _string(raw: Any, what: str) -> str:
    if type(raw) is not str:
        raise ValueError(f"{what} must be a string, got {value_label(raw)}")
    return raw


def _digest(raw: Any, what: str) -> str:
    if type(raw) is not str or _DIGEST.fullmatch(raw) is None:
        raise ValueError(f"{what} must be a sha256 digest, got {value_label(raw)}")
    return raw


def _counts(raw: Any, what: str) -> dict[str, int]:
    if type(raw) is not dict:
        raise ValueError(f"{what} must be an object")
    return {_string(k, what): _count(v, what) for k, v in raw.items()}


def _list(raw: Any, what: str) -> list[Any]:
    if type(raw) is not list:
        raise ValueError(f"{what} must be a list")
    return raw


def _read_witness(raw: Any) -> Witness:
    body = _object(raw, frozenset({"role", "value"}), "witness")
    witness = Witness(_string(body["role"], "role"), _string(body["value"], "value"))
    validate_witness(witness)
    return witness


def _read_obligation(raw: Any) -> ObligationResult:
    if type(raw) is not dict or not (
        _OBLIGATION_KEYS
        <= set(raw)
        <= _OBLIGATION_KEYS | set(_OPTIONAL_OBLIGATION_KEYS)
    ):
        raise ValueError(
            f"obligation has keys {sorted(raw) if type(raw) is dict else raw!r:.200}"
        )
    subject_body = _object(raw["subject"], frozenset({"kind", "token"}), "subject")
    optional = {
        key: _string(raw[key], key) for key in _OPTIONAL_OBLIGATION_KEYS if key in raw
    }
    attribution = optional.pop("attribution", None)
    obligation = ObligationResult(
        attribution=None if attribution is None else Attribution(attribution),
        rule_id=_string(raw["rule_id"], "rule_id"),
        scope=_string(raw["scope"], "scope"),
        subject=Subject(
            SubjectKind(_string(subject_body["kind"], "subject kind")),
            _string(subject_body["token"], "subject token"),
        ),
        status=ObligationStatus(_string(raw["status"], "status")),
        witnesses=tuple(_read_witness(w) for w in _list(raw["witnesses"], "witnesses")),
        **optional,
    )
    validate_obligation_shape(obligation)
    return obligation


def _read_error(raw: Any, owner: str) -> PropertyError | None:
    if raw is None:
        return None
    body = _object(raw, _ERROR_KEYS, "error")
    error = PropertyError(
        _string(body["type"], "error type"),
        _string(body["message"], "error message"),
        _count(body["message_bytes"], "error message_bytes"),
        _string(body["message_sha256"], "error message_sha256"),
        body["oversize"],
    )
    validate_property_error(owner, error)
    return error


def _read_result(raw: Any) -> PropertyResult:
    body = _object(raw, _RESULT_KEYS, "property result")
    pid = _string(body["property_id"], "property_id")
    obligations = tuple(
        _read_obligation(o) for o in _list(body["obligations"], "obligations")
    )
    keys = [(o.rule_id, o.scope) for o in obligations]
    if keys != sorted(keys):
        raise ValueError(f"{pid}: obligations must be sorted")
    return PropertyResult(
        property_id=pid,
        obligations=obligations,
        coverage=_counts(body["coverage"], "coverage"),
        error=_read_error(body["error"], pid),
    )


def _read_bindings(raw: Any) -> Bindings:
    body = _object(raw, _BINDING_KEYS, "bindings")
    execution = body["execution"]
    profile = _string(body["semantics_profile"], "semantics profile")
    validate_target_metadata(profile, {})
    return Bindings(
        runspec=_digest(body["runspec"], "runspec digest"),
        history=_digest(body["history"], "history digest"),
        semantic=_digest(body["semantic"], "semantic digest"),
        execution=None if execution is None else _digest(execution, "execution digest"),
        semantics_profile=profile,
    )


def _read_work(raw: Any) -> Work:
    body = _object(raw, frozenset({"phases", "components"}), "work")
    tables = {}
    for name in ("phases", "components"):
        table = body[name]
        if type(table) is not dict:
            raise ValueError(f"work {name} must be an object")
        tables[name] = {
            _string(key, f"work {name}"): _counts(value, f"work {name}")
            for key, value in table.items()
        }
    return Work(phases=tables["phases"], components=tables["components"])


def _read_proofs(
    raw: Any, results: tuple[PropertyResult, ...]
) -> tuple[effects.ProofCertificate, ...]:
    """Parse the proofs and require every proof reference to resolve both ways."""
    proofs = tuple(effects.read_proof(item) for item in _list(raw, "proofs"))
    ids = [proof.id for proof in proofs]
    if ids != sorted(set(ids)):
        raise ValueError("proofs must be unique and sorted by id")
    violated = {
        effects.ObligationRef(o.rule_id, o.scope): o.proof
        for result in results
        for o in _violations(result)
    }
    listed = {proof.id: frozenset(proof.obligations) for proof in proofs}
    for result in results:
        for obligation in result.obligations:
            if obligation.proof is None:
                continue
            if obligation.proof not in listed:
                raise ValueError(f"obligation names unknown proof {obligation.proof!r}")
            ref = effects.ObligationRef(obligation.rule_id, obligation.scope)
            if ref not in listed[obligation.proof]:
                raise ValueError(
                    f"{ref} claims proof {obligation.proof!r}, which does not list it"
                )
    for proof in proofs:
        for ref in proof.obligations:
            if violated.get(ref, None) != proof.id:
                raise ValueError(
                    f"proof {proof.id!r} names {ref}, which it does not prove"
                )
    return proofs


def _check_projection(
    kind: Any,
    reason: str,
    cause: Any,
    stored_lanes: Any,
    results: tuple[PropertyResult, ...],
    problems: tuple[str, ...],
) -> None:
    """Require the stored projection to be exactly what the stored results imply.

    A gated verdict has a structural cause, no results, no lanes, and the
    gate's reason. A verdict with no results and no gate is the zero-property
    fallback: not-evaluated with cause `no-evaluation`, exactly
    `NO_PROPERTY_REASON`, and no lanes. Every other verdict's kind, reason,
    cause, and lanes are recomputed.
    """
    if problems:
        if cause not in _GATE_CAUSES or results:
            raise ValueError("a gate needs a structural cause and no property results")
        expected = (NOT_EVALUATED, gate_reason(cause, problems), cause, {})
    elif cause in _GATE_CAUSES:
        raise ValueError(f"cause {cause!r} needs gate problems")
    elif not results:
        expected = (NOT_EVALUATED, NO_PROPERTY_REASON, NO_EVALUATION, {})
    else:
        expected = (*project(results), lanes(results))
    if (kind, reason, cause, stored_lanes) != expected:
        raise ValueError(
            f"stored projection {(kind, reason, cause, stored_lanes)!r:.2000} "
            f"disagrees with its results {expected!r:.2000}"
        )


def _check_registry_shape(results: tuple[PropertyResult, ...]) -> None:
    """Require results shaped like a registry: core first, scoped and unique rules.

    Non-empty results start with exactly the core properties in order and
    name no other core property; every other property ID is a plain name
    outside the reserved namespaces and owns no rule in them; each
    obligation's rule is scoped to its property (a core property may own
    the rule named like itself); `(rule_id, scope)` is unique within and
    across properties.
    """
    if not results:
        return
    ids = tuple(result.property_id for result in results)
    core = len(CORE_PROPERTY_IDS)
    if ids[:core] != CORE_PROPERTY_IDS or any(is_core(pid) for pid in ids[core:]):
        raise ValueError(f"properties {list(ids)} do not start with the core in order")
    seen: set[tuple[str, str]] = set()
    for result in results:
        pid = result.property_id
        if not is_core(pid) and (
            _PLAIN_ID.fullmatch(pid) is None or pid in RESERVED_NAMESPACES
        ):
            raise ValueError(f"bad property id {pid!r:.200}")
        for obligation in result.obligations:
            rule_id = obligation.rule_id
            if not is_core(pid) and rule_id.split(".", 1)[0] in RESERVED_NAMESPACES:
                raise ValueError(f"{pid}: rule {rule_id!r:.200} is reserved")
            if not (rule_id.startswith(f"{pid}.") or (is_core(pid) and rule_id == pid)):
                raise ValueError(
                    f"{pid}: rule {rule_id!r:.200} is outside its property"
                )
            key = (rule_id, obligation.scope)
            if key in seen:
                raise ValueError(f"{rule_id}[{obligation.scope!r:.200}] appears twice")
            seen.add(key)


def _read_gate(raw: Any) -> tuple[str, ...]:
    if raw is None:
        return ()
    body = _object(raw, frozenset({"problems"}), "gate")
    problems = tuple(_string(p, "problem") for p in _list(body["problems"], "problems"))
    if not problems:
        raise ValueError("a gate needs at least one problem")
    return problems


def read_verdict_document(raw: Any) -> Verdict:
    """Parse an exact `verdict.json` v1 document back into a verdict.

    Any other schema version, a missing or unknown key, a repeated
    property, results not shaped like a registry (`_check_registry_shape`),
    an unsorted or malformed obligation, a proof reference that does not
    resolve both ways, or a projection, lane map, or cause that disagrees
    with the stored obligations raises ValueError; there is no best-effort
    or legacy branch.
    """
    body = _object(raw, _TOP_KEYS, "verdict")
    version = body["schema_version"]
    if type(version) is not int or version != VERDICT_SCHEMA_VERSION:
        raise ValueError(
            f"verdict schema_version must be 1, got {value_label(version)}"
        )
    bindings = _read_bindings(body["bindings"])
    problems = _read_gate(body["gate"])
    results = tuple(_read_result(r) for r in _list(body["properties"], "properties"))
    ids = [result.property_id for result in results]
    if len(set(ids)) != len(ids):
        raise ValueError(f"repeated property id in {ids}")
    _check_registry_shape(results)
    proofs = _read_proofs(body["proofs"], results)
    reason = _string(body["reason"], "reason")
    _check_projection(
        body["kind"],
        reason,
        body["not_evaluated_cause"],
        body["lanes"],
        results,
        problems,
    )
    report = PropertyReport(
        results,
        _digest(body["identity_digest"], "identity digest"),
        bindings,
        problems,
        proofs,
        _read_work(body["work"]),
    )
    return Verdict(
        body["kind"],
        reason,
        lanes=dict(body["lanes"]),
        not_evaluated_cause=body["not_evaluated_cause"],
        report=report,
    )


def decode_verdict(data: bytes) -> Verdict:
    """Parse `verdict.json` bytes, requiring exactly the canonical encoding.

    Repeated keys, non-finite numbers, and any byte that `encode_verdict`
    would not produce raise ValueError.
    """
    raw = jsonio.loads(data.decode("ascii"))
    verdict = read_verdict_document(raw)
    if encode_verdict(verdict) != data:
        raise ValueError("verdict.json bytes are not the canonical encoding")
    return verdict
