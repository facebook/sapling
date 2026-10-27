# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Canonical error-set task keys and the findings report of a campaign.

A run's violated obligations are read from verdict v1 (findings are derived
from them, never stored twice) and partitioned by exact subject
occurrence. The product-actionable findings of one subject (`product`,
`undetermined`) become a canonical error set through
`core.identity.canonical_errors`: sorted, deduplicated `(rule_id, finding
kind, declared identity roles)` triples whose digest is the task key. The
subject path, action IDs, instance witnesses, target, semantics profile,
digests, run, and seed never enter it; they stay as evidence. Non-product
findings (`oracle-suspect`, `harness`, `reference`) on a subject with a
product finding stay in that key's evidence; on a subject without one they
are listed under `routed` (harness or reference) in findings.json and
never keyed. The
report names, per key, every run that produced it with its RunSpec and the
command that replays it; a person deduplicates by key. Nothing is filed.
"""

from __future__ import annotations

import json
from collections.abc import Callable, Iterable, Mapping, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory.core import jsonio
from eden.integration.correctness.exploratory.core.campaign import (
    ACTION_FUNNEL,
    campaign_health,
    OBLIGATION_FUNNEL,
    REPORT_KEYS,
    REPORT_OPTIONAL_KEYS,
    REPORT_SCHEMA_VERSION,
)
from eden.integration.correctness.exploratory.core.checker import (
    bind,
    lane_kind,
    registry,
    semantic_digest,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    CONTAINMENT_TIMEOUT,
    durable_history,
    ENVIRONMENT_DEGRADED,
    HARNESS_FAILURE,
    MOUNT_WEDGED_PREFIX,
)
from eden.integration.correctness.exploratory.core.identity import (
    canonical_errors,
    CanonicalError,
    error_set_document,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    Finding,
    identity_digest,
    identity_witnesses,
    ObligationStatus,
    PRODUCT_ATTRIBUTIONS,
    PropertyResult,
    RuleDef,
    Subject,
)
from eden.integration.correctness.exploratory.core.runspec import (
    from_json,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.core.scheduler import OPERATION_TIMEOUT
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.schemas import (
    Admission,
    BOUNDS,
    Bounds,
    document_digest,
    FINDINGS_SCHEMA_VERSION,
    load_admission,
    load_verdict_file,
    require_document,
    RunPurpose,
    SchemaError,
    strict_loads,
    VERDICT_OVERSIZE,
    VerdictFile,
    write_document,
)

MAX_ROLE_NAME_BYTES = 64
MAX_ROLE_VALUE_BYTES = 256
MAX_ROLES_PER_RULE = 8
ROUTED_SCHEMA = "exploratory-routed-set-v1"
Row = Mapping[str, Any]

EXECUTION_OPERATION_TIMEOUT = "execution.operation-timeout"
EXECUTION_HARNESS_FAILURE = "execution.harness-failure"
EXECUTION_CONTAINMENT_TIMEOUT = "execution.containment-timeout"
EXECUTION_ENVIRONMENT_DEGRADED = "execution.environment-degraded"
EXECUTION_INVALID_HISTORY = "execution.invalid-history"
EXECUTION_INVALID_RUNSPEC = "execution.invalid-runspec"
EXECUTION_UNREADABLE = "execution.execution-unreadable"
EXECUTION_MISSING = "execution.missing-execution"
# Teardown had to abort the run's own FUSE connection to unwedge its mount.
EXECUTION_MOUNT_WEDGED = "execution.mount-wedged"
_STATUS_IDS = {
    HARNESS_FAILURE: EXECUTION_HARNESS_FAILURE,
    CONTAINMENT_TIMEOUT: EXECUTION_CONTAINMENT_TIMEOUT,
    ENVIRONMENT_DEGRADED: EXECUTION_ENVIRONMENT_DEGRADED,
}
_GATE_IDS = {
    "invalid-history": EXECUTION_INVALID_HISTORY,
    "invalid-runspec": EXECUTION_INVALID_RUNSPEC,
}


class Route:
    """Who a subject's findings are actionable by."""

    PRODUCT = "product"
    HARNESS = "harness"
    REFERENCE = "reference"


def route_of(attributions: Iterable[Attribution | str]) -> str:
    """Route one subject's findings.

    Any product or undetermined finding keeps the subject product-actionable,
    whatever suspects ride along. With none, findings that all blame the
    differential reference side route to the reference; any other mix of
    oracle-suspect and harness findings routes to the harness.
    """
    found = {str(a) for a in attributions}
    if found & {str(a) for a in PRODUCT_ATTRIBUTIONS}:
        return Route.PRODUCT
    if found == {str(Attribution.REFERENCE)}:
        return Route.REFERENCE
    return Route.HARNESS


@dataclass(frozen=True)
class Unclassified:
    """A product-actionable finding kept in evidence that cannot enter a key, and why."""

    property_id: str
    finding: Finding
    cause: str


RuleTable = Mapping[str, tuple[str, RuleDef]]


def rule_table(target: Target) -> dict[str, tuple[str, RuleDef]]:
    """Map every core and target rule ID to its owning property and definition."""
    return {
        rule_id: (definition.id, rule)
        for definition in registry(target)
        for rule_id, rule in definition.rules.items()
    }


def _utf8(value: str) -> int:
    return len(value.encode("utf-8", errors="surrogatepass"))


def _precheck(property_id: str, finding: Finding, table: RuleTable) -> str | None:
    """Name why a finding cannot enter a key before its roles are read, or None."""
    owner = table.get(finding.rule_id)
    if owner is None:
        return "unknown-rule"
    owning_property, rule = owner
    if owning_property != property_id:
        return "foreign-rule"
    if finding.subject.kind != rule.subject_kind:
        return "subject-kind-mismatch"
    if len(rule.identity_roles) > MAX_ROLES_PER_RULE:
        return "oversize-role"
    if any(
        _utf8(w.role) > MAX_ROLE_NAME_BYTES or _utf8(w.value) > MAX_ROLE_VALUE_BYTES
        for w in finding.witnesses
        if w.role in rule.identity_roles
    ):
        return "oversize-role"
    return None


def _role_cause(finding: Finding, rule: RuleDef) -> str:
    """Name why `identity_witnesses` refused a finding's declared roles."""
    present = {w.role for w in finding.witnesses if w.role in rule.identity_roles}
    return "missing-role" if present != set(rule.identity_roles) else "duplicate-role"


@dataclass(frozen=True)
class SubjectGroup:
    """Every violated obligation on one exact subject, by attribution.

    `errors` is the subject's canonical error set (product-actionable
    findings only) and `unclassified` the product-actionable findings that
    could not enter it; any unclassified finding leaves the set incomplete,
    so the group is never keyed. `members` keeps every finding, suspects
    included, in property and obligation order.
    """

    subject: Subject
    members: tuple[tuple[str, Finding], ...]
    errors: tuple[CanonicalError, ...]
    unclassified: tuple[Unclassified, ...]

    @property
    def route(self) -> str:
        """Return the subject's route (`route_of` its members' attributions)."""
        return route_of(finding.attribution for _pid, finding in self.members)


def subject_groups(
    results: Iterable[PropertyResult], table: RuleTable
) -> tuple[SubjectGroup, ...]:
    """Partition violated obligations by exact subject equality, in first-encounter order.

    No transitive merging, partial-role overlap, or path inference is ever
    applied: two findings share a group only when their subjects are equal.
    Each group's product-actionable findings are keyed by
    `core.identity.canonical_errors`, after the checks only the owning
    property can make (foreign rule, subject kind, role size).
    """
    members: dict[Subject, list[tuple[str, Finding]]] = {}
    for result in results:
        for finding in result.findings:
            members.setdefault(finding.subject, []).append(
                (result.property_id, finding)
            )
    rules = {rule_id: rule for rule_id, (_pid, rule) in table.items()}
    groups = []
    for subject, found in members.items():
        unclassified: list[Unclassified] = []
        keyable: list[Finding] = []
        for property_id, finding in found:
            if finding.attribution not in PRODUCT_ATTRIBUTIONS:
                continue
            cause = _precheck(property_id, finding, table)
            rule = rules.get(finding.rule_id)
            if cause is None and rule is not None:
                if identity_witnesses(rule, finding) is None:
                    cause = _role_cause(finding, rule)
            if cause is None:
                keyable.append(finding)
            else:
                unclassified.append(Unclassified(property_id, finding, cause))
        keyed_sets = canonical_errors(keyable, rules).sets
        errors = keyed_sets[0].errors if keyed_sets else ()
        groups.append(SubjectGroup(subject, tuple(found), errors, tuple(unclassified)))
    return tuple(groups)


def document_rules(document: Mapping[str, Any]) -> tuple[str, ...]:
    """Return a key document's rule IDs, sorted."""
    return tuple(sorted({rule for rule, _kind, _roles in document["errors"]}))


@dataclass(frozen=True)
class Occurrence:
    """One task key in one RunSpec: every subject that produced it and every finding."""

    task_key: str
    document: Mapping[str, Any]
    subjects: tuple[str, ...]
    members: tuple[tuple[str, Finding], ...]
    runspec_digest: str

    @property
    def rules(self) -> tuple[str, ...]:
        """Return the exact set's rule IDs, sorted."""
        return document_rules(self.document)

    @property
    def attributions(self) -> tuple[str, ...]:
        """Return every member's attribution, suspects included."""
        return tuple(str(finding.attribution) for _pid, finding in self.members)


@dataclass(frozen=True)
class RoutedGroup:
    """A subject whose findings are all non-product: listed under `routed`, never a key.

    `digest` names its route and classifiable errors under a schema no task
    key uses, so a reader of the harness or reference routes can
    deduplicate it.
    """

    subject: Subject
    route: str
    members: tuple[tuple[str, Finding], ...]
    digest: str


@dataclass(frozen=True)
class RunKeys:
    """Everything one verdict contributes to task identity."""

    occurrences: tuple[Occurrence, ...] = ()
    incomplete: tuple[SubjectGroup, ...] = ()
    routed: tuple[RoutedGroup, ...] = ()


def _routed(group: SubjectGroup, table: RuleTable) -> RoutedGroup:
    errors = set()
    for _pid, finding in group.members:
        owner = table.get(finding.rule_id)
        roles = sorted(
            {
                (w.role, w.value)
                for w in finding.witnesses
                if owner is not None and w.role in owner[1].identity_roles
            }
        )
        errors.add((finding.rule_id, str(finding.attribution), tuple(roles)))
    digest = document_digest(
        {
            "schema": ROUTED_SCHEMA,
            "route": group.route,
            "errors": [
                [r, a, [list(x) for x in roles]] for r, a, roles in sorted(errors)
            ],
        }
    )
    return RoutedGroup(group.subject, group.route, group.members, digest)


def keyed(verdict: Verdict, target: Target, table: RuleTable | None = None) -> RunKeys:
    """Return each fully classified subject's key, collapsing equal keys, plus the rest.

    A subject with any unclassified product-actionable finding is returned
    in `incomplete`: retained as evidence, never keyed. A subject with only
    non-product findings is `routed`.
    """
    report = verdict.report
    if report is None:
        return RunKeys()
    table = table if table is not None else rule_table(target)
    by_key: dict[str, list[SubjectGroup]] = {}
    documents: dict[str, dict[str, Any]] = {}
    incomplete: list[SubjectGroup] = []
    routed: list[RoutedGroup] = []
    for group in subject_groups(report.results, table):
        if group.unclassified:
            incomplete.append(group)
            continue
        if not group.errors:
            routed.append(_routed(group, table))
            continue
        document = error_set_document(group.errors)
        key = document_digest(document)
        documents[key] = document
        by_key.setdefault(key, []).append(group)
    found = tuple(
        Occurrence(
            key,
            documents[key],
            tuple(sorted(g.subject.token for g in groups)),
            tuple(member for g in groups for member in g.members),
            report.bindings.runspec,
        )
        for key, groups in sorted(by_key.items())
    )
    return RunKeys(found, tuple(incomplete), tuple(routed))


def _errors_of(document: Mapping[str, Any]) -> frozenset[tuple[str, str, str]]:
    return frozenset(
        (rule, kind, json.dumps(roles, sort_keys=True))
        for rule, kind, roles in document["errors"]
    )


def related_keys(
    document: Mapping[str, Any], others: Mapping[str, Mapping[str, Any]]
) -> tuple[tuple[str, str], ...]:
    """Return other keys whose sets are strict subsets or supersets; never merged."""
    mine = _errors_of(document)
    related = []
    for key, other in sorted(others.items()):
        theirs = _errors_of(other)
        if theirs < mine:
            related.append((key, "subset"))
        elif theirs > mine:
            related.append((key, "superset"))
    return tuple(related)


@dataclass(frozen=True)
class ExecutionFinding:
    """A typed execution failure; never a product task key."""

    execution_id: str
    detail: str


def _load_json(path: Path) -> Any:
    return strict_loads(path.read_text(encoding="utf-8"))


def _timed_out_inputs(run_dir: Path, target: Target) -> list[str]:
    """Kinds of modeled inputs the durable history completed as operation timeouts."""
    spec_path, history_path = run_dir / "run-spec.json", run_dir / "history.jsonl"
    if not (spec_path.is_file() and history_path.is_file()):
        return []
    try:
        spec = from_json(spec_path.read_text(), target)
    except SpecError:
        return []
    kinds = {action.id: action.kind for action in spec.actions}
    found: list[str] = []
    for event in durable_history(history_path):
        if event.type != "info" or event.data.get("reason") != OPERATION_TIMEOUT:
            continue
        action = event.data.get("action")
        kind = kinds.get(action) if isinstance(action, str) else None
        if kind is None:
            continue
        definition = target.actions.get(kind)
        if definition is not None and definition.axis == "input" and definition.modeled:
            found.append(kind)
    return list(dict.fromkeys(found))


def execution_findings(
    run_dir: Path,
    target: Target,
    verdict_file: VerdictFile | SchemaError,
) -> tuple[ExecutionFinding, ...]:
    """Classify one run's execution and verdict problems by stable execution ID."""
    found: list[ExecutionFinding] = []
    try:
        execution = _load_json(run_dir / "execution.json")
        status = execution.get("status") if type(execution) is dict else None
        reason = execution.get("reason", "") if type(execution) is dict else ""
        backend = execution.get("backend_error") if type(execution) is dict else None
        cleanup = execution.get("cleanup") if type(execution) is dict else None
    except (OSError, SchemaError) as error:
        status, reason, backend, cleanup = None, "", None, None
        found.append(ExecutionFinding(EXECUTION_UNREADABLE, str(error)))
    if status is not None and type(status) is not str:
        found.append(
            ExecutionFinding(
                EXECUTION_UNREADABLE, f"status is {type(status).__name__}, not text"
            )
        )
        status = None
    if type(reason) is not str:
        reason = ""
    if status in _STATUS_IDS:
        detail = str(reason) if type(backend) is not str else f"{reason} ({backend})"
        found.append(ExecutionFinding(_STATUS_IDS[status], detail))
    if type(cleanup) is str and cleanup.startswith(MOUNT_WEDGED_PREFIX):
        found.append(
            ExecutionFinding(
                EXECUTION_MOUNT_WEDGED, cleanup.removeprefix(MOUNT_WEDGED_PREFIX)
            )
        )
    if isinstance(verdict_file, SchemaError):
        found.append(ExecutionFinding(verdict_file.decision, str(verdict_file)))
    else:
        if verdict_file.oversize:
            found.append(
                ExecutionFinding(
                    VERDICT_OVERSIZE,
                    f"{verdict_file.encoded_bytes} bytes, {verdict_file.sha256}",
                )
            )
        cause = verdict_file.verdict.not_evaluated_cause
        if cause in _GATE_IDS:
            found.append(
                ExecutionFinding(_GATE_IDS[cause], verdict_file.verdict.reason)
            )
    for kind in _timed_out_inputs(run_dir, target):
        found.append(ExecutionFinding(EXECUTION_OPERATION_TIMEOUT, kind))
    return tuple(found)


@dataclass(frozen=True)
class ControlOutcome:
    """One red control's result for every rule it maps, by rule."""

    control: str
    runspec_digest: str | None
    per_rule: Mapping[str, str]


def rule_gaps(verdict: Verdict, target: Target, rule_id: str) -> tuple[str, ...]:
    """Return why one rule may have missed a violation: its gaps, scoped to the rule.

    The owning property raising or never running, or any not-evaluated
    obligation of the rule itself; another rule's gaps never count.
    """
    owner = rule_table(target).get(rule_id)
    report = verdict.report
    if owner is None:
        return ("unmapped",)
    if report is None or not report.results:
        return (owner[0],)
    result = next((r for r in report.results if r.property_id == owner[0]), None)
    if result is None or result.error is not None:
        return (owner[0],)
    return tuple(
        f"{o.rule_id}@{o.scope}"
        for o in result.obligations
        if o.rule_id == rule_id and o.status == ObligationStatus.NOT_EVALUATED
    )


def red_control_outcome(
    directory: Path, target: Target, campaign: str | None = None
) -> ControlOutcome:
    """Judge one `run --red-control NAME` artifact dir per mapped rule.

    A rule passes only when the run completed, its verdict is readable and
    still bound to its run files and the current semantic and identity
    digests, its admission says it is a red-control run for `campaign`,
    and that rule has a violated obligation. A rule that did not fire is
    `incomplete-...` when its own obligations (or its property) were not
    evaluated, else `did-not-fire`; one rule's result never depends on
    another rule's gaps.
    """
    control = directory.name
    rules = target_control_rules(target).get(control)
    if rules is None:
        return ControlOutcome(control, None, {})
    try:
        status = _load_json(directory / "execution.json").get("status")
    except (OSError, SchemaError, AttributeError):
        status = "missing-execution"
    try:
        verdict_file = load_verdict_file(directory / "verdict.json")
    except (OSError, SchemaError) as error:
        cause = error.decision if isinstance(error, SchemaError) else "unreadable"
        return ControlOutcome(control, None, dict.fromkeys(sorted(rules), cause))
    verdict = verdict_file.verdict
    report = verdict.report
    digest = None if report is None else report.bindings.runspec
    failure = _control_failure(directory, target, verdict, status, campaign)
    if failure is not None or report is None:
        # A completed run without a report fails its bindings (`binding-report`).
        cause = failure or "binding-report"
        return ControlOutcome(control, digest, dict.fromkeys(sorted(rules), cause))
    fired = {f.rule_id for r in report.results for f in r.findings}
    per_rule = {}
    for rule in sorted(rules):
        gaps = () if rule in fired else rule_gaps(verdict, target, rule)
        if rule in fired:
            per_rule[rule] = "pass"
        elif gaps:
            per_rule[rule] = "incomplete-" + ",".join(gaps)
        else:
            per_rule[rule] = "did-not-fire"
    return ControlOutcome(control, digest, per_rule)


def _control_failure(
    directory: Path,
    target: Target,
    verdict: Verdict,
    status: object,
    campaign: str | None,
) -> str | None:
    """Name why a control execution does not speak for this campaign, or None."""
    if status != COMPLETED:
        return f"not-completed-{status}"
    mismatched = binding_mismatches(directory, target, verdict)
    if mismatched:
        return "binding-" + ",".join(mismatched)
    try:
        admission = load_admission(directory)
    except SchemaError as error:
        return error.decision
    if admission.run_purpose != RunPurpose.RED_CONTROL:
        return f"purpose-{admission.run_purpose}"
    if campaign is None or admission.campaign_id != campaign:
        return "unbound-campaign"
    return None


def target_control_rules(target: Target) -> dict[str, frozenset[str]]:
    """Return each red control of the target's full registry and the rules it must trip."""
    mapping: dict[str, set[str]] = {}
    for definition in registry(target):
        for rule in definition.rules.values():
            for control in rule.red_controls:
                mapping.setdefault(control, set()).add(rule.id)
    return {c: frozenset(r) for c, r in sorted(mapping.items())}


EXECUTION_BINDING_MISMATCH = "execution.binding-mismatch"
EXECUTION_SUMMARY_MISMATCH = "execution.summary-mismatch"
EXECUTION_RUN_UNREADABLE = "execution.run-unreadable"


@dataclass(frozen=True)
class RunRecord:
    """One campaign run: its artifacts, read once, and what they establish."""

    seed: int
    stratum: tuple[str, ...] | None
    run_dir: Path
    admission: Admission | None
    admission_error: str | None
    verdict_file: VerdictFile | SchemaError
    execution_status: str
    execution: tuple[ExecutionFinding, ...]
    occurrences: tuple[Occurrence, ...]
    incomplete_groups: tuple[SubjectGroup, ...]
    routed: tuple[RoutedGroup, ...] = ()


REPORT_DECISION = "schema:campaign-report"
FINDINGS_DECISION = "schema:findings"
FINDINGS_NAME = "findings.json"


def read_report(report_path: Path) -> Row:
    """Read an exact campaign report document whose every field has its written type.

    A field of any other type is `schema:campaign-report`, so no consumer
    (health checks, telemetry, campaign records) ever sees a value it cannot use.
    """
    raw = strict_loads(report_path.read_text(encoding="utf-8"), REPORT_DECISION)
    body = require_document(
        raw,
        version=REPORT_SCHEMA_VERSION,
        required=REPORT_KEYS,
        what="campaign report",
        decision=REPORT_DECISION,
        optional=REPORT_OPTIONAL_KEYS,
    )
    problem = report_field_problem(body)
    if problem is not None:
        raise SchemaError(REPORT_DECISION, problem)
    return body


def _count(value: Any) -> bool:
    return type(value) is int and value >= 0


def _counts(value: Any) -> bool:
    return type(value) is dict and all(
        type(k) is str and _count(v) for k, v in value.items()
    )


def _witness_table(value: Any) -> bool:
    return type(value) is dict and all(
        type(k) is str and type(v) is dict and _counts(v) and "witnessed" in v
        for k, v in value.items()
    )


def _text_or_none(value: Any) -> bool:
    return value is None or type(value) is str


def _work(value: Any) -> bool:
    """A work summary: run count, per-phase total/max counters, component maxima."""
    if type(value) is not dict or set(value) != {"runs", "phases", "components"}:
        return False
    phases = value["phases"]
    return (
        _count(value["runs"])
        and _counts(value["components"])
        and type(phases) is dict
        and all(
            type(phase) is str
            and type(counters) is dict
            and all(
                type(name) is str
                and type(entry) is dict
                and set(entry) == {"total", "max"}
                and _counts(entry)
                for name, entry in counters.items()
            )
            for phase, counters in phases.items()
        )
    )


def _strata(value: Any) -> bool:
    return type(value) is dict and type(value.get("missing")) is list


_REPORT_FIELD_TYPES: dict[str, Callable[[Any], bool]] = {
    "strata": _strata,
    "seeds": _count,
    "completed": _count,
    "verdicts": _counts,
    "harness_failures": _count,
    "environment_degraded": _count,
    "rate": lambda v: type(v) in (int, float),
    "properties": lambda v: type(v) is dict
    and all(type(k) is str and type(x) is dict for k, x in v.items()),
    "rejects": lambda v: type(v) is list and all(type(x) is dict for x in v),
    "excluded_sites": lambda v: type(v) is list,
    "leaked_run_dirs": lambda v: type(v) is list,
    "sites": _witness_table,
    "input_families": _witness_table,
    "under_witnessed_families": lambda v: type(v) is list,
    "actions": lambda v: type(v) is list
    and all(
        type(row) is dict
        and all(type(row.get(k)) is str for k in ("family", "mode"))
        and all(_count(row.get(k)) for k in ACTION_FUNNEL)
        for row in v
    ),
    "obligations": lambda v: type(v) is list
    and all(
        type(row) is dict
        and all(type(row.get(k)) is str for k in ("family", "channel", "mode"))
        and all(_count(row.get(k)) for k in OBLIGATION_FUNNEL)
        for row in v
    ),
    "work": _work,
    "not_evaluated_causes": _counts,
    "stopped": lambda v: v is None
    or (type(v) is dict and type(v.get("reason")) is str),
    "campaign_id": _text_or_none,
    "daemon_identity": _text_or_none,
    "daemon_sha256": _text_or_none,
    "daemon_source_commit": _text_or_none,
    "daemon_log": _text_or_none,
    "fatal_eden_bug": lambda v: v is None or type(v) is bool,
    "binaries": lambda v: type(v) is dict
    and all(
        type(b) is dict
        and set(b) == {"path", "sha256"}
        and all(_text_or_none(b[k]) for k in b)
        for b in v.values()
    ),
    "diagnostics": lambda v: type(v) is dict,
}


def report_field_problem(body: Mapping[str, Any]) -> str | None:
    """Name the first report field whose type the campaign never writes, or None."""
    for name, valid in _REPORT_FIELD_TYPES.items():
        if name in body and not valid(body[name]):
            return f"{name} is malformed ({type(body[name]).__name__})"
    return None


def load_campaign(report_path: Path, artifact_dir: Path) -> tuple[Row, list[Row]]:
    """Read the campaign report and its `summary.jsonl` rows; neither is modified.

    A summary line that does not parse is kept as a row naming the problem,
    so one bad line never aborts the pass.
    """
    report = read_report(report_path)
    rows: list[Row] = []
    with open(artifact_dir / "summary.jsonl", encoding="utf-8") as summary:
        for index, line in enumerate(summary):
            if not line.strip():
                continue
            try:
                row = strict_loads(line, "execution.summary-unreadable")
            except SchemaError as error:
                rows.append({"_line": index, "_error": str(error)})
                continue
            rows.append(
                row
                if type(row) is dict
                else {"_line": index, "_error": "not an object"}
            )
    return report, rows


def binding_mismatches(
    run_dir: Path, target: Target, verdict: Verdict
) -> tuple[str, ...]:
    """Recompute a verdict's bindings from the run's files and the current checker.

    Names every binding (`runspec`, `history`, `execution`, `semantic`,
    `identity`) that differs, `unreadable-<exception type>` when a run
    file could not be read, or `report` when the verdict has none. The
    history is the durable prefix containment bound, whatever tail follows.
    The `semantics_profile` binding is not compared on its own, but the
    identity digest covers the profile, so a profile rename is an
    `identity` mismatch.
    """
    report = verdict.report
    if report is None:
        return ("report",)
    try:
        spec = from_json(
            (run_dir / "run-spec.json").read_text(encoding="utf-8"), target
        )
        events = durable_history(run_dir / "history.jsonl")
        execution = jsonio.loads(
            (run_dir / "execution.json").read_text(encoding="utf-8")
        )
    except (OSError, SpecError, ValueError) as error:
        return (f"unreadable-{type(error).__name__}",)
    fresh = bind(spec, events, execution, target=target)
    found = [
        name
        for name in ("runspec", "history", "execution", "semantic")
        if getattr(fresh, name) != getattr(report.bindings, name)
    ]
    if report.identity_digest != current_identity(target):
        found.append("identity")
    return tuple(found)


def current_identity(target: Target) -> str:
    """Return the identity digest a verdict judged by the current checker carries."""
    return identity_digest(registry(target), target.semantics_profile)


def _within(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
    except ValueError:
        return False
    return True


def _broken_run(index: int, row: Row, cause: str, detail: str) -> RunRecord:
    seed = row.get("seed")
    return RunRecord(
        seed=seed if type(seed) is int else -1 - index,
        stratum=None,
        run_dir=Path(str(row.get("artifacts", ""))),
        admission=None,
        admission_error="purpose:unreadable-run",
        verdict_file=SchemaError(cause, detail),
        execution_status="unreadable",
        execution=(ExecutionFinding(cause, detail),),
        occurrences=(),
        incomplete_groups=(),
    )


def load_run(
    row: Row,
    target: Target,
    artifact_dir: Path,
    index: int = 0,
    bounds: Bounds = BOUNDS,
) -> RunRecord:
    """Read one run's admission, execution, and verdict and derive its keys.

    The summary row must name a run inside `artifact_dir` whose
    `execution.json` status it repeats; the verdict's bindings are
    recomputed from the run's files. Any problem is a typed execution
    finding on this run, never an aborted pass.
    """
    if "_error" in row:
        return _broken_run(
            index, row, "execution.summary-unreadable", str(row["_error"])
        )
    artifacts, seed = row.get("artifacts"), row.get("seed")
    if type(artifacts) is not str or type(seed) is not int:
        return _broken_run(
            index, row, EXECUTION_SUMMARY_MISMATCH, "seed or artifacts missing"
        )
    malformed = _summary_problem(row)
    if malformed is not None:
        return _broken_run(index, row, EXECUTION_SUMMARY_MISMATCH, malformed)
    run_dir = artifact_dir / artifacts
    if not _within(run_dir, artifact_dir):
        return _broken_run(
            index,
            row,
            EXECUTION_SUMMARY_MISMATCH,
            f"{artifacts} is outside {artifact_dir}",
        )
    try:
        return _load_run(row, run_dir, seed, target, bounds)
    except (OSError, SchemaError, SpecError) as error:
        return _broken_run(
            index, row, EXECUTION_RUN_UNREADABLE, f"{type(error).__name__}: {error}"
        )


def _summary_problem(row: Row) -> str | None:
    """Name a summary-row field whose type the campaign never writes, or None."""
    status, stratum = row.get("status"), row.get("stratum")
    if status is not None and type(status) is not str:
        return f"status is {type(status).__name__}, not text"
    if stratum is not None and (
        type(stratum) is not list or any(type(part) is not str for part in stratum)
    ):
        return f"stratum is {type(stratum).__name__}, not a list of text"
    return None


def _execution_status(run_dir: Path) -> str | None:
    """Return the run's execution status text, or None when it is unreadable or not text."""
    try:
        status = _load_json(run_dir / "execution.json").get("status")
    except (OSError, SchemaError, AttributeError):
        return None
    return status if type(status) is str else None


def _load_run(
    row: Row, run_dir: Path, seed: int, target: Target, bounds: Bounds
) -> RunRecord:
    try:
        admission, admission_error = load_admission(run_dir), None
    except SchemaError as error:
        admission, admission_error = None, error.decision
    try:
        verdict_file: VerdictFile | SchemaError = load_verdict_file(
            run_dir / "verdict.json", bounds
        )
    except SchemaError as error:
        verdict_file = error
    run_keys = RunKeys()
    extra: list[ExecutionFinding] = []
    status = _execution_status(run_dir)
    if status != row.get("status"):
        extra.append(
            ExecutionFinding(
                EXECUTION_SUMMARY_MISMATCH,
                f"summary {row.get('status')!r} != execution {status!r}",
            )
        )
    if isinstance(verdict_file, VerdictFile):
        run_keys = keyed(verdict_file.verdict, target)
        mismatched = binding_mismatches(run_dir, target, verdict_file.verdict)
        if mismatched:
            extra.append(
                ExecutionFinding(EXECUTION_BINDING_MISMATCH, ",".join(mismatched))
            )
    stratum = row.get("stratum")
    return RunRecord(
        seed=seed,
        stratum=None if stratum is None else tuple(stratum),
        run_dir=run_dir,
        admission=admission,
        admission_error=admission_error,
        verdict_file=verdict_file,
        execution_status=status if type(status) is str else EXECUTION_MISSING,
        execution=(*execution_findings(run_dir, target, verdict_file), *extra),
        occurrences=run_keys.occurrences,
        incomplete_groups=run_keys.incomplete,
        routed=run_keys.routed,
    )


def check_lines(
    report: Row, runs: Sequence[RunRecord], controls: Sequence[ControlOutcome] | None
) -> list[str]:
    """PASS/FAIL per campaign-health check, then provenance and each red control."""

    def line(ok: bool, rule: str, detail: str = "") -> str:
        return f"{'PASS' if ok else 'FAIL'} {rule}" + (f": {detail}" if detail else "")

    unproven = [
        run.seed
        for run in runs
        if run.admission is None
        or run.admission.daemon is None
        or not run.admission.daemon.binary_sha256
        or not run.admission.daemon.source_commit
    ]
    lines = [
        line(check.ok, f"health {check.name}", "" if check.ok else check.detail)
        for check in campaign_health(report)
    ]
    lines.append(
        line(
            not unproven,
            "provenance on every seed",
            f"seeds {unproven}" if unproven else "",
        )
    )
    if controls is None:
        lines.append("red-controls: not supplied")
    else:
        for outcome in controls:
            if not outcome.per_rule:
                lines.append(f"FAIL red-control {outcome.control}: unmapped")
            for rule, result in outcome.per_rule.items():
                lines.append(
                    line(
                        result == "pass",
                        f"red-control {outcome.control} {rule}",
                        "" if result == "pass" else result,
                    )
                )
    return lines


@dataclass
class _KeyEvidence:
    document: Mapping[str, Any]
    occurrences: list[tuple[RunRecord, Occurrence]] = field(default_factory=list)


def _collect(runs: Sequence[RunRecord]) -> dict[str, _KeyEvidence]:
    keys: dict[str, _KeyEvidence] = {}
    for run in runs:
        for occurrence in run.occurrences:
            keys.setdefault(
                occurrence.task_key, _KeyEvidence(occurrence.document)
            ).occurrences.append((run, occurrence))
    return dict(sorted(keys.items()))


def _property_states(run: RunRecord) -> dict[str, str]:
    if (
        not isinstance(run.verdict_file, VerdictFile)
        or run.verdict_file.verdict.report is None
    ):
        return {}
    states = {}
    for result in run.verdict_file.verdict.report.results:
        counts = dict.fromkeys(ObligationStatus, 0)
        for obligation in result.obligations:
            counts[obligation.status] += 1
        tally = ", ".join(f"{status} {count}" for status, count in counts.items())
        error = "; error" if result.error is not None else ""
        states[result.property_id] = f"{lane_kind(result)}: {tally}{error}"
    return states


def report_campaign_id(report: Row, report_path: Path) -> str:
    """Return the campaign a report names, or one derived from its path."""
    return report.get("campaign_id") or document_digest({"report": str(report_path)})


def execution_lines(runs: Sequence[RunRecord]) -> list[str]:
    """Execution findings (never keyed) by stable ID and detail."""
    groups: dict[tuple[str, str], list[int]] = {}
    for run in runs:
        for finding in run.execution:
            detail = (
                finding.detail
                if finding.execution_id == EXECUTION_OPERATION_TIMEOUT
                else ""
            )
            groups.setdefault((finding.execution_id, detail), []).append(run.seed)
    lines = []
    for (execution_id, detail), seeds in sorted(groups.items()):
        lines.append(
            f"EXECUTION {execution_id}{f' {detail}' if detail else ''} seeds {seeds}"
        )
    return lines


def replay_command(target_name: str, run_dir: str) -> str:
    """Return the code-owned command that re-runs a run directory's exact RunSpec."""
    return (
        "buck2 run @fbcode//mode/opt "
        "fbcode//eden/integration/correctness/exploratory:explore -- replay "
        f"--target {target_name} --run {run_dir} --artifacts <new dir>"
    )


def _verdict_keys(run_dir: Path, target: Target) -> tuple[str, list[str]]:
    """Return a run's verdict kind and task keys, or why its verdict is unreadable."""
    try:
        verdict = load_verdict_file(run_dir / "verdict.json").verdict
    except SchemaError as error:
        return error.decision, []
    return verdict.kind, [o.task_key for o in keyed(verdict, target).occurrences]


def replay(
    run_dir: Path,
    artifacts: Path,
    target: Target,
    run: Callable[[RunSpec, Path], None],
) -> list[str]:
    """Re-run `run_dir`'s exact RunSpec into `artifacts` once and compare task keys.

    A person reads the comparison: nothing votes, and nothing is written
    outside `artifacts`.
    """
    spec = from_json((run_dir / "run-spec.json").read_text(encoding="utf-8"), target)
    run(spec, artifacts)
    before_kind, before = _verdict_keys(run_dir, target)
    after_kind, after = _verdict_keys(artifacts, target)
    lines = [f"original {run_dir}: {before_kind}", f"replay {artifacts}: {after_kind}"]
    for key in sorted(set(before) | set(after)):
        where = (
            "both"
            if key in before and key in after
            else "original only"
            if key in before
            else "replay only"
        )
        lines.append(f"KEY {key}: {where}")
    return lines


def _obligations_of(run: RunRecord, subjects: Sequence[str]) -> list[dict[str, Any]]:
    """Return every violated obligation of `run` on `subjects`: the evidence and proofs."""
    if not isinstance(run.verdict_file, VerdictFile):
        return []
    report = run.verdict_file.verdict.report
    if report is None:
        return []
    return [
        {
            "property_id": result.property_id,
            "rule_id": obligation.rule_id,
            "scope": obligation.scope,
            "subject": obligation.subject.token,
            "attribution": str(obligation.attribution),
            "reason": obligation.reason,
            "proof": obligation.proof,
            "witnesses": [[w.role, w.value] for w in obligation.witnesses],
        }
        for result in report.results
        for obligation in result.obligations
        if obligation.status == ObligationStatus.VIOLATED
        and obligation.subject.token in subjects
    ]


def _finding_entry(
    key: str,
    evidence: _KeyEvidence,
    others: Mapping[str, Mapping[str, Any]],
    target: Target,
) -> dict[str, Any]:
    """Return one key's report entry: its error set and, per run, how to replay it."""
    runs = []
    for run, occurrence in sorted(evidence.occurrences, key=lambda pair: pair[0].seed):
        verdict = run.verdict_file
        runs.append(
            {
                "seed": run.seed,
                "run_dir": str(run.run_dir),
                "runspec_digest": occurrence.runspec_digest,
                "verdict": verdict.verdict.kind
                if isinstance(verdict, VerdictFile)
                else None,
                "replay": replay_command(target.name, str(run.run_dir)),
                "daemon": None
                if run.admission is None or run.admission.daemon is None
                else {
                    "binary_sha256": run.admission.daemon.binary_sha256,
                    "source_commit": run.admission.daemon.source_commit,
                },
                "subjects": list(occurrence.subjects),
                "properties": _property_states(run),
                "obligations": _obligations_of(run, occurrence.subjects),
            }
        )
    return {
        "task_key": key,
        "errors": [list(error) for error in evidence.document["errors"]],
        "rules": list(document_rules(evidence.document)),
        "related": [
            {"task_key": k, "relation": r}
            for k, r in related_keys(evidence.document, others)
        ],
        "runs": runs,
    }


def finding_markdown(entry: Mapping[str, Any], target_name: str) -> str:
    """Render one report entry for a person: what was observed and how to replay it."""
    first = entry["runs"][0]
    observed = "\n".join(
        f"- `{o['rule_id']}` ({o['attribution']}) on `{o['subject']}` at `{o['scope']}`:"
        f" {o['reason']}"
        + (f" (proof {o['proof']})" if o["proof"] else "")
        + "; witnesses "
        + (", ".join(f"`{role}={value}`" for role, value in o["witnesses"]) or "none")
        for o in first["obligations"]
    )
    states = "\n".join(
        f"- `{pid}`: {state}" for pid, state in sorted(first["properties"].items())
    )
    related = (
        "\n".join(f"- {r['relation']}: `{r['task_key']}`" for r in entry["related"])
        or "- none"
    )
    seeds = [run["seed"] for run in entry["runs"]]
    daemon = first["daemon"]
    built = (
        "unattested"
        if daemon is None
        else f"`{daemon['binary_sha256']}` built from `{daemon['source_commit']}`"
    )
    return (
        f"# [{target_name}] {' + '.join(entry['rules'])}\n\n"
        f"Task key: `{entry['task_key']}` (seeds {seeds})\n\n"
        f"Daemon: {built}\n\n"
        f"## Observed in seed {first['seed']} ({first['verdict']})\n\n"
        f"RunSpec `{first['runspec_digest']}` in `{first['run_dir']}`\n\n{observed}\n\n"
        f"## Properties\n\n{states}\n\n## Related keys\n\n{related}\n\n"
        f"## Replay\n\n```\n{first['replay']}\n```\n\n"
        "## Error set\n\n```\n"
        + json.dumps(entry["errors"], indent=2, sort_keys=True)
        + "\n```\n"
    )


def write_findings(
    report_path: Path,
    artifact_dir: Path,
    out_dir: Path,
    target: Target,
    control_dirs: Sequence[Path] | None = None,
    bounds: Bounds = BOUNDS,
) -> list[str]:
    """Report a campaign's findings under `out_dir` and return the printed lines.

    Writes `findings.json` (every product-actionable error set with, per run,
    its RunSpec, replay command, verdict, and violated obligations; routed
    harness and reference subjects; execution findings; red-control
    outcomes) and one `finding-NN.md` per key. Nothing outside `out_dir`
    changes and no store is contacted.
    """
    report, rows = load_campaign(report_path, artifact_dir)
    runs = [
        load_run(row, target, artifact_dir, index, bounds)
        for index, row in enumerate(rows)
    ]
    campaign = report_campaign_id(report, report_path)
    controls = (
        None
        if control_dirs is None
        else [red_control_outcome(Path(d), target, campaign) for d in control_dirs]
    )
    if controls is not None:
        supplied = {c.control for c in controls}
        controls += [
            ControlOutcome(name, None, dict.fromkeys(sorted(rules), "missing"))
            for name, rules in target_control_rules(target).items()
            if name not in supplied
        ]
    keys = _collect(runs)
    documents = {key: evidence.document for key, evidence in keys.items()}
    out_dir.mkdir(parents=True, exist_ok=True)
    entries = []
    for number, (key, evidence) in enumerate(keys.items(), 1):
        entry = _finding_entry(
            key, evidence, {k: d for k, d in documents.items() if k != key}, target
        )
        entry["body"] = f"finding-{number:02d}.md"
        (out_dir / entry["body"]).write_text(
            finding_markdown(entry, target.name), encoding="utf-8"
        )
        entries.append(entry)
    document = {
        "schema_version": FINDINGS_SCHEMA_VERSION,
        "campaign_id": campaign,
        "target": target.name,
        "semantics_profile": target.semantics_profile,
        "identity_digest": current_identity(target),
        "semantic_digest": semantic_digest(target),
        "controls": {c.control: dict(c.per_rule) for c in controls or ()},
        "findings": entries,
        "routed": [
            {
                "seed": run.seed,
                "subject": group.subject.token,
                "route": group.route,
                "digest": group.digest,
                "rules": sorted({f.rule_id for _pid, f in group.members}),
                "attributions": sorted(
                    {str(f.attribution) for _pid, f in group.members}
                ),
            }
            for run in runs
            for group in run.routed
        ],
        "execution": [
            {"seed": run.seed, "execution_id": f.execution_id, "detail": f.detail}
            for run in runs
            for f in run.execution
        ],
        "unclassified": [
            {
                "seed": run.seed,
                "subject": g.subject.token,
                "property_id": u.property_id,
                "rule_id": u.finding.rule_id,
                "cause": u.cause,
            }
            for run in runs
            for g in run.incomplete_groups
            for u in g.unclassified
        ],
    }
    write_document(out_dir / FINDINGS_NAME, document)
    return [
        *check_lines(report, runs, controls),
        *(
            f"FINDING {' + '.join(e['rules'])} [{e['task_key']}]"
            f" seeds {[r['seed'] for r in e['runs']]}: {e['runs'][0]['replay']}"
            for e in entries
        ),
        *execution_lines(runs),
        *(
            f"ROUTED {r['route'].upper()} seed {r['seed']} on {r['subject']}:"
            f" {' + '.join(r['rules'])} ({', '.join(r['attributions'])})"
            for r in document["routed"]
        ),
        *(
            f"UNCLASSIFIED seed {u['seed']} {u['rule_id']} on {u['subject']}: {u['cause']}"
            for u in document["unclassified"]
        ),
        f"findings: {len(entries)} written to {out_dir}",
    ]


# The keys `write_findings` writes beside `schema_version`.
FINDINGS_KEYS = (
    "campaign_id",
    "target",
    "semantics_profile",
    "identity_digest",
    "semantic_digest",
    "controls",
    "findings",
    "routed",
    "execution",
    "unclassified",
)


def read_findings(path: Path) -> Row:
    """Read a `findings.json` this module wrote: exactly its schema version and keys."""
    raw = strict_loads(path.read_text(encoding="utf-8"), FINDINGS_DECISION)
    return require_document(
        raw,
        version=FINDINGS_SCHEMA_VERSION,
        required=FINDINGS_KEYS,
        what=f"{path}",
        decision=FINDINGS_DECISION,
        optional=frozenset(),
    )
