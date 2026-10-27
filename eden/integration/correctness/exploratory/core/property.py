# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The property and verdict protocol between the checker and a target's properties.

A property (`PropertyDef`) declares `RuleDef`s and a collector that reads a
`CheckInput` and writes to a `CollectorSink`. This module defines what
verdicts are made of (`Subject`, `Witness`, `Attribution`,
`ObligationResult`, `PropertyResult`), the validators the checker applies to
every property declaration and result (`validate_registry`,
`validate_result`), the rule identity digest (`identity_digest`), and the
core not-evaluated reasons.
"""

from __future__ import annotations

import hashlib
import re
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any, Protocol, TYPE_CHECKING, TypeAlias

from .jsonio import (
    check_name,
    digest,
    MAX_ID_BYTES,
    MAX_SCALAR_BYTES,
    NAME as _NAME,
    require as _require,
)

if TYPE_CHECKING:
    from .effects import Constraint, EffectSet, ObligationRef, Observation, PinnedFacts
    from .facts import HistoryFacts
    from .spec_types import RunSpec


MAX_RUN_OBLIGATIONS = 16384
RESERVED_NAMESPACES = frozenset({"core", "execution", "lifecycle"})
IDENTITY_SCHEMA = "exploratory-rule-identity-v1"
SOLVER_UNAVAILABLE = "core.solver.unavailable"
SOLVER_BUDGET = "core.solver.budget"
SOLVER_EXPLANATION_BUDGET = "core.solver.explanation-budget"
SOLVER_HISTORY_BUDGET = "core.solver.history-budget"
SOLVER_UNKNOWN_INITIAL = "core.solver.unknown-initial"
SOLVER_INPUT_EXCLUDED = "core.solver.input-excluded"
SOLVER_WITNESS_REJECTED = "core.solver.witness-rejected"
CORE_NOT_EVALUATED_REASONS: Mapping[str, str] = {
    SOLVER_UNAVAILABLE: (
        "An Agree comparison supporting the obligation was not decided by a "
        "direct check, or no engine ran"
    ),
    SOLVER_INPUT_EXCLUDED: (
        "A constraint supporting the obligation names an observation whose "
        "supporting properties were incomplete, so the constraint was left "
        "out of the consistency engine's input"
    ),
    SOLVER_BUDGET: (
        "The consistency engine exhausted its deterministic decision budget "
        "before deciding the obligation's factor component"
    ),
    SOLVER_EXPLANATION_BUDGET: (
        "The obligation's factor component is unsatisfiable and the engine "
        "exhausted its deterministic explanation budget before attributing it"
    ),
    SOLVER_HISTORY_BUDGET: (
        "The run's solve, membership queries, and attribution trials together "
        "exhausted the history-wide engine work budget before deciding the "
        "obligation"
    ),
    SOLVER_UNKNOWN_INITIAL: (
        "An input supporting the obligation depends on a cell whose initial "
        "value no pinned fact gives"
    ),
    SOLVER_WITNESS_REJECTED: (
        "The consistency engine reported every input satisfiable, but its "
        "witness assignment does not explain them on replay "
        "(`semantics.explains`): an engine defect, so nothing it decided holds"
    ),
}
_DOTTED = re.compile(r"[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+")
_ALIAS = re.compile(r"[^\s/][^\s]*")
_ALIAS_UNSAFE = re.compile(r"[\s%]")


class SubjectKind(StrEnum):
    """What a finding is about; the run partitions findings by subject."""

    PATH = "path"
    DESCRIPTOR = "descriptor"
    CHECKOUT = "checkout"
    DAEMON = "daemon"


class Attribution(StrEnum):
    """Who a violated obligation implicates.

    `PRODUCT` and `UNDETERMINED` are product-actionable. `ORACLE_SUSPECT`
    means live observers on distinct serving paths agreed with each other
    against the contract effects; agreement on one shared serving path
    stays `UNDETERMINED`. `HARNESS` blames the recorder or the harness and
    `REFERENCE` the differential reference side. The last three are
    non-product: they project to harness-suspect and never key a task.
    """

    PRODUCT = "product"
    UNDETERMINED = "undetermined"
    ORACLE_SUSPECT = "oracle-suspect"
    HARNESS = "harness"
    REFERENCE = "reference"


PRODUCT_ATTRIBUTIONS = frozenset({Attribution.PRODUCT, Attribution.UNDETERMINED})
_SUBJECT_KINDS = frozenset(SubjectKind)
_ATTRIBUTIONS = frozenset(Attribution)


class ObligationStatus(StrEnum):
    """The result of one `(rule obligation, scope)`."""

    SATISFIED = "satisfied"
    VIOLATED = "violated"
    NOT_EVALUATED = "not-evaluated"


_STATUSES = frozenset(ObligationStatus)


@dataclass(frozen=True)
class Subject:
    """The one canonical thing a finding is about.

    A token is `<kind>:<alias>`, where the alias is replay-stable: a path or
    descriptor alias as the model names it, never a generated on-disk
    location or a history position. Checkout- and daemon-wide proofs use
    the fixed aggregate subjects.
    """

    kind: SubjectKind
    token: str


def _percent(match: re.Match[str]) -> str:
    return "".join(f"%{byte:02X}" for byte in _surrogate_bytes(match.group()))


def path_subject(path: str) -> Subject:
    """Return the PATH subject of one canonical checkout path.

    ROOT (``""``) is ``path:.``; no canonical path has a ``.`` segment, so
    that alias names nothing else. ``%`` and every whitespace character
    are percent-encoded as UTF-8, so every canonical path has a valid,
    distinct alias. A token past MAX_SCALAR_BYTES is ``path:%sha256:<hex>``
    of the path, which the encoding never produces (``%s`` is not ``%XX``).
    """
    alias = _ALIAS_UNSAFE.sub(_percent, path) if path else "."
    token = f"path:{alias}"
    if len(_surrogate_bytes(token)) > MAX_SCALAR_BYTES:
        token = "path:%sha256:" + hashlib.sha256(_surrogate_bytes(path)).hexdigest()
    return Subject(SubjectKind.PATH, token)


CHECKOUT_SUBJECT = Subject(SubjectKind.CHECKOUT, "checkout:all")
DAEMON_SUBJECT = Subject(SubjectKind.DAEMON, "daemon:all")
_AGGREGATE_SUBJECTS = {
    SubjectKind.DAEMON: DAEMON_SUBJECT,
}


def checkout_subject(action_id: str) -> Subject:
    """Return the CHECKOUT subject of one action's per-action judgment.

    Checkouts, renames, probes, and result shapes use it.
    """
    return Subject(SubjectKind.CHECKOUT, f"checkout:{action_id}")


@dataclass(frozen=True)
class Witness:
    """One typed fact attached to an obligation.

    Whether a role is identity or instance evidence is declared by the
    rule's `identity_roles`, never inferred from the value.
    """

    role: str
    value: str


@dataclass(frozen=True, kw_only=True)
class RuleDef:
    """One authoritative guarantee a property checks.

    Every declared rule is asserted. `identity_roles` name the witness
    roles that distinguish task-relevant variants; every other witness is
    instance evidence. `red_controls` are the controls that must trip this
    rule. The rule's finding kind is the last segment of its ID.
    """

    id: str
    statement: str
    subject_kind: SubjectKind
    identity_roles: tuple[str, ...] = ()
    red_controls: frozenset[str] = frozenset()

    @property
    def finding_kind(self) -> str:
        """Return the finding kind this rule emits: its ID's last segment."""
        return self.id.rsplit(".", 1)[1]


@dataclass(frozen=True)
class CoverageObligation:
    """A situation a property counts so a clean campaign can be read.

    Coverage is observability only; it never gates a result.
    """

    id: str
    description: str


@dataclass(frozen=True)
class PropertyError:
    """An exception that escaped one property's collector, or a contract violation.

    `type` and `message` are kept whole, whatever their size, and
    `message_bytes` and `message_sha256` describe the exact bytes of the
    message. `oversize` marks an error past the scalar policy bounds (type
    over MAX_ID_BYTES or message over MAX_SCALAR_BYTES): a reason names such
    a type by its digest, and a verdict that grows past the operational
    bound is flagged when it is read, never truncated.
    """

    type: str
    message: str
    message_bytes: int
    message_sha256: str
    oversize: bool


def _surrogate_bytes(text: str) -> bytes:
    return text.encode("utf-8", errors="surrogatepass")


def _error_oversize(type_name: str, message_bytes: int) -> bool:
    return (
        len(_surrogate_bytes(type_name)) > MAX_ID_BYTES
        or message_bytes > MAX_SCALAR_BYTES
    )


def property_error(type_name: str, message: str) -> PropertyError:
    """Return a PropertyError with its whole text, message digest, and marker."""
    full = _surrogate_bytes(message)
    return PropertyError(
        type_name,
        message,
        len(full),
        "sha256:" + hashlib.sha256(full).hexdigest(),
        _error_oversize(type_name, len(full)),
    )


def validate_property_error(owner: str, error: PropertyError) -> None:
    """Reject an error whose size, digest, or oversize marker disagrees with its message."""
    _require(
        isinstance(error, PropertyError)
        and type(error.type) is str
        and type(error.message) is str
        and type(error.message_bytes) is int
        and type(error.message_sha256) is str
        and type(error.oversize) is bool,
        f"{owner}: bad error of type {type(error).__name__}",
    )
    _require(bool(error.type), f"{owner}: empty error type")
    retained = _surrogate_bytes(error.message)
    _require(
        error.message_bytes == len(retained),
        f"{owner}: error message is {len(retained)} bytes, not {error.message_bytes}",
    )
    _require(
        error.message_sha256 == "sha256:" + hashlib.sha256(retained).hexdigest(),
        f"{owner}: error digest does not match its message",
    )
    _require(
        error.oversize == _error_oversize(error.type, error.message_bytes),
        f"{owner}: error oversize marker is {error.oversize}",
    )


@dataclass(frozen=True)
class Finding:
    """One violated obligation, viewed as its rule, attribution, reason, and witnesses."""

    rule_id: str
    attribution: Attribution
    reason: str
    subject: Subject
    witnesses: tuple[Witness, ...] = ()


@dataclass(frozen=True, kw_only=True)
class ObligationResult:
    """The result of one `(rule obligation, scope)`.

    A violated obligation carries its attribution and human reason, and a
    proof reference when an engine proved it. A not-evaluated obligation
    carries a declared reason ID and, in `reason`, optional detail. A
    satisfied obligation carries neither. `component` names the diagnostic
    factor component that decided it, when one did.
    """

    rule_id: str
    scope: str
    subject: Subject
    status: ObligationStatus
    witnesses: tuple[Witness, ...] = ()
    attribution: Attribution | None = None
    reason: str | None = None
    not_evaluated_reason: str | None = None
    proof: str | None = None
    component: str | None = None

    def finding(self) -> Finding:
        """Return this violated obligation as a finding."""
        _require(self.status == ObligationStatus.VIOLATED, "not a violation")
        assert self.attribution is not None and self.reason is not None
        return Finding(
            self.rule_id, self.attribution, self.reason, self.subject, self.witnesses
        )


@dataclass(frozen=True)
class PropertyResult:
    """Every obligation one property declared, with its result.

    Obligations are sorted by `(rule_id, scope)`. An incomplete result
    carries the error that stopped the collector together with every
    obligation resolved before it. Findings are derived, never stored.
    """

    property_id: str
    obligations: tuple[ObligationResult, ...] = ()
    coverage: Mapping[str, int] = field(default_factory=dict)
    error: PropertyError | None = None

    @property
    def complete(self) -> bool:
        """Return whether the collector finished without an error."""
        return self.error is None

    @property
    def findings(self) -> tuple[Finding, ...]:
        """Return every violated obligation as a finding, in obligation order."""
        return tuple(
            obligation.finding()
            for obligation in self.obligations
            if obligation.status == ObligationStatus.VIOLATED
        )


@dataclass
class WorkAccount:
    """Engine work one check may spend across every solve and membership query.

    `spent["work"]` counts what was drawn; it can pass `limit` by at most
    one component's share, drawn before the meter sees the account empty.
    """

    limit: int
    spent: dict[str, int] = field(default_factory=lambda: {"work": 0})

    def left(self) -> int:
        """Return the work still available."""
        return max(0, self.limit - self.spent["work"])


@dataclass(frozen=True)
class CheckInput:
    """Everything one collector reads: the spec, typed facts, pinned facts, effects.

    All of it is shared and read-only except `work`; `effects` is what the
    target's contracts hook produced. Every engine query a collector makes draws
    from `work`, the check's history-wide budget (None: unbounded).
    """

    spec: RunSpec
    facts: HistoryFacts
    pinned: PinnedFacts | None
    effects: EffectSet
    work: WorkAccount | None = None


class CollectorSink(Protocol):
    """Where a collector declares obligations and records their evidence.

    The checker implements it; properties never do. Every call validates
    against the property's declarations and raises on a mistake.
    """

    def obligation(
        self,
        rule_id: str,
        scope: str,
        *,
        subject: Subject,
        witnesses: Iterable[Witness] = (),
    ) -> ObligationRef:
        """Declare one `(rule obligation, scope)`; repeating it returns the same ref."""
        ...  # pragma: no cover

    def observe(self, observation: Observation) -> None:
        """Record one observation supporting its obligations.

        Node IDs are unique across the run: recording an observation again
        with identical content merges its obligations. Reusing a group ID,
        or an observation ID with different content, raises within one
        property; across properties it is a contract error of each.
        """
        ...  # pragma: no cover

    def constrain(self, constraint: Constraint) -> None:
        """Record one named constraint supporting its obligation.

        Its endpoints must name nodes of the types its class requires, and
        recording the same constraint twice raises.
        """
        ...  # pragma: no cover

    def satisfied(self, ref: ObligationRef) -> None:
        """Record that a direct check satisfied the obligation."""
        ...  # pragma: no cover

    def violated(
        self,
        ref: ObligationRef,
        attribution: Attribution,
        reason: str,
        witnesses: Iterable[Witness] = (),
    ) -> None:
        """Record that a direct check violated the obligation.

        A violation is never overturned or downgraded: the most severe
        attribution recorded for the obligation is kept.
        """
        ...  # pragma: no cover

    def violated_unless_engine(
        self,
        ref: ObligationRef,
        attribution: Attribution,
        reason: str,
        witnesses: Iterable[Witness] = (),
    ) -> None:
        """Record a violation that holds once the engine satisfies the obligation.

        Without engine inputs the violation holds outright.

        The obligation's own engine inputs decide first: an engine
        violation replaces this one, and an engine that cannot decide
        leaves the obligation not evaluated. A direct violation still
        wins over both.
        """
        ...  # pragma: no cover

    def not_evaluated(
        self, ref: ObligationRef, reason_id: str, detail: str = ""
    ) -> None:
        """Record why the obligation could not be decided, by declared reason."""
        ...  # pragma: no cover

    def covered(self, obligation_id: str, count: int = 1) -> None:
        """Count observations toward one declared coverage obligation."""
        ...  # pragma: no cover


if TYPE_CHECKING:
    Collector: TypeAlias = Callable[[CheckInput, CollectorSink], None]
else:
    Collector: TypeAlias = Callable[..., None]


@dataclass(frozen=True, kw_only=True)
class PropertyDef:
    """One property: its collector, its rule table, and its declared vocabulary.

    A run whose collectors declare more than `MAX_RUN_OBLIGATIONS`
    obligations in all is refused before the engine runs, never truncated.
    `not_evaluated_reasons` map each property-scoped reason ID
    to its meaning. `consumes` lists the `(action kind, result field)`
    pairs the collector reads.
    """

    id: str
    collect: Collector
    rules: Mapping[str, RuleDef]
    coverage: tuple[CoverageObligation, ...] = ()
    not_evaluated_reasons: Mapping[str, str] = field(default_factory=dict)
    consumes: frozenset[tuple[str, str]] = frozenset()

    def __post_init__(self) -> None:
        validate_property(self)


def _utf8_len(value: str) -> int:
    return len(value.encode("utf-8"))


def _check_scalar(value: Any, what: str) -> None:
    _require(type(value) is str, f"{what} must be a string, got {value!r:.200}")
    _require(
        _utf8_len(value) <= MAX_SCALAR_BYTES,
        f"{what} exceeds {MAX_SCALAR_BYTES} UTF-8 bytes",
    )


def _check_name(value: Any, what: str) -> None:
    check_name(value, what, MAX_ID_BYTES)


def _check_scoped(owner: str, value: Any, what: str) -> None:
    _require(
        type(value) is str
        and value.startswith(f"{owner}.")
        and _NAME.fullmatch(value[len(owner) + 1 :]) is not None
        and _utf8_len(value) <= MAX_ID_BYTES,
        f"{what} {value!r:.200} must be {owner}.<name>",
    )


def check_scope(scope: Any) -> None:
    """Reject an obligation scope that is not non-empty text of at most MAX_ID_BYTES."""
    _require(
        type(scope) is str
        and bool(scope)
        and len(_surrogate_bytes(scope)) <= MAX_ID_BYTES,
        f"bad obligation scope {scope!r:.200}",
    )


def is_core(property_id: str) -> bool:
    """Return whether an ID names a core property shared by every target."""
    return property_id.startswith("core.")


def validate_subject(subject: Subject) -> None:
    """Reject a subject that is not one canonical `<kind>:<alias>` token."""
    _require(
        isinstance(subject, Subject) and subject.kind in _SUBJECT_KINDS,
        f"bad subject {subject!r:.200}",
    )
    _check_scalar(subject.token, "subject token")
    aggregate = _AGGREGATE_SUBJECTS.get(subject.kind)
    if aggregate is not None:
        _require(
            subject == aggregate,
            f"{subject.kind} subject must be {aggregate.token!r}, got {subject.token!r:.200}",
        )
        return
    prefix, separator, alias = subject.token.partition(":")
    _require(
        prefix == subject.kind
        and bool(separator)
        and _ALIAS.fullmatch(alias) is not None,
        f"subject token {subject.token!r:.200} must be {subject.kind}:<alias>",
    )


def validate_witness(witness: Witness) -> None:
    """Reject a witness with a malformed role or an oversized value."""
    _require(isinstance(witness, Witness), f"bad witness {witness!r:.200}")
    _check_name(witness.role, "witness role")
    _check_scalar(witness.value, f"witness {witness.role!r} value")


def validate_rule(rule: RuleDef) -> None:
    """Reject a rule whose ID, statement, roles, or controls are malformed."""
    _require(
        type(rule.id) is str
        and _DOTTED.fullmatch(rule.id) is not None
        and _utf8_len(rule.id) <= MAX_ID_BYTES,
        f"bad rule id {rule.id!r:.200}",
    )
    _check_scalar(rule.statement, f"{rule.id}: statement")
    _require(bool(rule.statement.strip()), f"{rule.id}: empty statement")
    _require(rule.subject_kind in _SUBJECT_KINDS, f"{rule.id}: bad subject kind")
    _require(
        type(rule.identity_roles) is tuple, f"{rule.id}: identity roles are a tuple"
    )
    for role in rule.identity_roles:
        _check_name(role, f"{rule.id}: identity role")
    _require(
        len(set(rule.identity_roles)) == len(rule.identity_roles),
        f"{rule.id}: repeated identity role",
    )
    for control in rule.red_controls:
        _check_name(control, f"{rule.id}: red control")


def validate_property(definition: PropertyDef) -> None:
    """Reject malformed property, rule, coverage, reason, and consumption declarations.

    A target property has a plain ID outside the reserved namespaces; a core
    property is `core.<name>`. Every rule, coverage obligation, and
    not-evaluated reason is scoped by its property ID; a core property may
    also own the rule named exactly like itself.
    """
    pid = definition.id
    if is_core(pid):
        _check_scoped("core", pid, "core property")
    else:
        _check_name(pid, "property id")
        _require(pid not in RESERVED_NAMESPACES, f"property id {pid!r} is reserved")
    _require(callable(definition.collect), f"{pid}: collect is not callable")
    _require(bool(definition.rules), f"{pid}: declares no rules")
    for rule_id, rule in definition.rules.items():
        _require(rule.id == rule_id, f"{pid}: rule key {rule_id!r} names {rule.id!r}")
        _require(
            rule_id.startswith(f"{pid}.") or (is_core(pid) and rule_id == pid),
            f"{pid}: rule {rule_id!r} is outside its property",
        )
        validate_rule(rule)
    obligations = [obligation.id for obligation in definition.coverage]
    _require(len(set(obligations)) == len(obligations), f"{pid}: repeated obligation")
    for obligation in definition.coverage:
        _check_scoped(pid, obligation.id, "coverage obligation")
        _check_scalar(obligation.description, f"{obligation.id}: description")
    for reason, meaning in definition.not_evaluated_reasons.items():
        _check_scoped(pid, reason, "not-evaluated reason")
        _check_scalar(meaning, f"{reason}: meaning")
    _require(
        type(definition.consumes) is frozenset,
        f"{pid}: consumes is a frozenset of (action kind, field) pairs",
    )
    for pair in definition.consumes:
        _require(
            type(pair) is tuple
            and len(pair) == 2
            and all(type(part) is str and bool(part) for part in pair),
            f"{pid}: bad consumed pair {pair!r:.200}",
        )


def _validate_status_fields(obligation: ObligationResult, where: str) -> None:
    status = obligation.status
    violated = status == ObligationStatus.VIOLATED
    _require(
        (obligation.attribution is not None) == violated,
        f"{where}: exactly a violated obligation carries an attribution",
    )
    _require(
        (obligation.not_evaluated_reason is not None)
        == (status == ObligationStatus.NOT_EVALUATED),
        f"{where}: exactly a not-evaluated obligation carries a reason ID",
    )
    _require(
        status != ObligationStatus.SATISFIED or obligation.reason is None,
        f"{where}: a satisfied obligation carries no reason",
    )
    _require(
        obligation.proof is None or violated, f"{where}: only a violation has a proof"
    )
    if violated:
        _require(
            obligation.attribution in _ATTRIBUTIONS,
            f"{where}: bad attribution {obligation.attribution!r:.200}",
        )
        _check_scalar(obligation.reason, f"{where}: reason")
    elif obligation.reason is not None:
        _check_scalar(obligation.reason, f"{where}: detail")
    reason_id = obligation.not_evaluated_reason
    if reason_id is not None:
        _require(
            type(reason_id) is str
            and _DOTTED.fullmatch(reason_id) is not None
            and _utf8_len(reason_id) <= MAX_ID_BYTES,
            f"{where}: bad not-evaluated reason {reason_id!r:.200}",
        )


def validate_obligation_shape(obligation: ObligationResult) -> None:
    """Reject an obligation result that is malformed on its own.

    The rule ID, scope, subject, and witnesses are well-formed, and the
    obligation carries exactly its status's fields.
    """
    _require(
        isinstance(obligation, ObligationResult),
        f"bad obligation {obligation!r:.200}",
    )
    _require(
        type(obligation.rule_id) is str
        and _DOTTED.fullmatch(obligation.rule_id) is not None
        and _utf8_len(obligation.rule_id) <= MAX_ID_BYTES,
        f"bad obligation rule id {obligation.rule_id!r:.200}",
    )
    check_scope(obligation.scope)
    where = f"{obligation.rule_id}[{obligation.scope!r:.200}]"
    _require(obligation.status in _STATUSES, f"{where}: bad status")
    validate_subject(obligation.subject)
    _validate_status_fields(obligation, where)
    for optional in (obligation.proof, obligation.component):
        if optional is not None:
            check_scope(optional)
    _require(type(obligation.witnesses) is tuple, f"{where}: witnesses are a tuple")
    for witness in obligation.witnesses:
        validate_witness(witness)


def validate_obligation(definition: PropertyDef, obligation: ObligationResult) -> None:
    """Reject an obligation result that disagrees with its property's declarations.

    Beyond `validate_obligation_shape`, it names one of the property's
    rules with that rule's subject kind and any reason ID is declared by
    the property or the core.
    """
    validate_obligation_shape(obligation)
    rule = definition.rules.get(obligation.rule_id)
    _require(
        rule is not None,
        f"{definition.id}: obligation names unknown rule {obligation.rule_id!r:.200}",
    )
    assert rule is not None
    where = f"{rule.id}[{obligation.scope!r:.200}]"
    _require(
        obligation.subject.kind == rule.subject_kind,
        f"{where}: subject {obligation.subject.token!r:.200} is not a {rule.subject_kind}",
    )
    reason_id = obligation.not_evaluated_reason
    _require(
        reason_id is None
        or reason_id in definition.not_evaluated_reasons
        or reason_id in CORE_NOT_EVALUATED_REASONS,
        f"{where}: undeclared not-evaluated reason {reason_id!r:.200}",
    )


def validate_result(definition: PropertyDef, result: PropertyResult) -> None:
    """Reject a result that disagrees with its property's declarations.

    Obligations are valid, unique, and sorted by `(rule_id, scope)`;
    coverage IDs are declared; any error is well-formed.
    """
    pid = definition.id
    _require(
        result.property_id == pid, f"result for {result.property_id!r} under {pid!r}"
    )
    if result.error is not None:
        validate_property_error(pid, result.error)
    keys = []
    for obligation in result.obligations:
        validate_obligation(definition, obligation)
        keys.append((obligation.rule_id, obligation.scope))
    _require(
        keys == sorted(set(keys)),
        f"{pid}: obligations must be unique and sorted by (rule_id, scope)",
    )
    declared = {obligation.id for obligation in definition.coverage}
    for obligation, count in result.coverage.items():
        _require(obligation in declared, f"{pid}: undeclared coverage {obligation!r}")
        _require(type(count) is int and count >= 0, f"{pid}: bad coverage count")


def identity_witnesses(
    rule: RuleDef, finding: Finding
) -> tuple[tuple[str, str], ...] | None:
    """Return the finding's declared identity roles and values, sorted by role.

    Returns None when a declared role is missing or carries more than one
    value: such a finding is retained but cannot be classified.
    """
    values: dict[str, set[str]] = {}
    for witness in finding.witnesses:
        if witness.role in rule.identity_roles:
            values.setdefault(witness.role, set()).add(witness.value)
    if set(values) != set(rule.identity_roles):
        return None
    if any(len(found) != 1 for found in values.values()):
        return None
    return tuple(sorted((role, next(iter(found))) for role, found in values.items()))


def control_rules(
    properties: Iterable[PropertyDef],
) -> dict[str, frozenset[str]]:
    """Return every red control mapped to the rules it must trip."""
    mapping: dict[str, set[str]] = {}
    for definition in properties:
        for rule in definition.rules.values():
            for control in rule.red_controls:
                mapping.setdefault(control, set()).add(rule.id)
    return {control: frozenset(rules) for control, rules in sorted(mapping.items())}


def validate_registry(
    properties: Iterable[PropertyDef], red_controls: frozenset[str]
) -> None:
    """Reject repeated property or rule IDs and a control set that is not the rules' union."""
    definitions = tuple(properties)
    ids = [definition.id for definition in definitions]
    _require(len(set(ids)) == len(ids), f"repeated property id in {ids}")
    rule_ids = [rule for definition in definitions for rule in definition.rules]
    _require(len(set(rule_ids)) == len(rule_ids), "repeated rule id")
    union = frozenset(control_rules(definitions))
    _require(
        red_controls == union,
        f"red controls {sorted(red_controls)} are not the union of rule "
        f"controls {sorted(union)}",
    )


def _rule_identity(rule: RuleDef) -> dict[str, Any]:
    return {
        "id": rule.id,
        "subject_kind": str(rule.subject_kind),
        "identity_roles": sorted(rule.identity_roles),
        "red_controls": sorted(rule.red_controls),
    }


def identity_digest(properties: Iterable[PropertyDef], semantics_profile: str) -> str:
    """Return the SHA-256 of the canonical rule identity table under one profile.

    The digest covers rule IDs, subject kinds, identity roles, and the
    red-control mapping. Statements, collectors, bounds, coverage,
    and property order do not enter it.
    """
    rules = sorted(
        (
            _rule_identity(rule)
            for definition in properties
            for rule in definition.rules.values()
        ),
        key=lambda item: item["id"],
    )
    document = {
        "schema": IDENTITY_SCHEMA,
        "semantics_profile": semantics_profile,
        "rules": rules,
    }
    return digest(document)
