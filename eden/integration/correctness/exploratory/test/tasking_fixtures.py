# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Target-agnostic fixtures for the verdict, campaign, and findings-report tests.

`SCRIPTED` is a real target over the fake vocabulary whose properties
declare exactly the obligations a RunSpec's `metadata.script` lists, so
`check()` yields real verdict v1 documents (real bindings, projection,
and digests) with any mix of satisfied, violated, and not-evaluated
obligations, attributions, subjects, and property errors. Nothing here
names an EdenFS rule or target.
"""

from __future__ import annotations

import json
import time
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory.core.checker import (
    Bindings,
    check,
    CORE_PROPERTY_IDS,
    encode_verdict,
    lanes,
    project,
    PropertyReport,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    Execution,
    ExplorationResult,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    Event,
    event_line,
    InMemorySink,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    ObligationResult,
    ObligationStatus,
    path_subject,
    PropertyDef,
    PropertyResult,
    RuleDef,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import (
    from_json,
    RunSpec,
    to_json,
)
from eden.integration.correctness.exploratory.core.scheduler import run_actions
from eden.integration.correctness.exploratory.core.target import Target, TargetSources
from eden.integration.correctness.exploratory.fake.target import ACTIONS, FAKE, READ
from eden.integration.correctness.exploratory.findings import keyed

SCRIPT = "script"
UNOBSERVED = "unobserved"
# Four UTF-8 bytes that `encode_verdict` escapes as a 12-byte surrogate pair.
WIDE = "\U0001f600"


def _rule(
    rule_id: str,
    roles: tuple[str, ...] = (),
    *,
    controls: frozenset[str] = frozenset(),
    kind: SubjectKind = SubjectKind.PATH,
) -> RuleDef:
    return RuleDef(
        id=rule_id,
        statement=f"{rule_id} holds.",
        subject_kind=kind,
        identity_roles=roles,
        red_controls=controls,
    )


RULES: Mapping[str, tuple[RuleDef, ...]] = {
    "bytes": (
        _rule("bytes.torn-read", ("channel",), controls=frozenset({"torn-write"})),
        _rule("bytes.stale-read", ("channel",), controls=frozenset({"stale-write"})),
        _rule("bytes.fabricated-value", ("channel",)),
    ),
    "status": (
        _rule(
            "status.model-live-stale-status",
            ("expected", "observed"),
            controls=frozenset({"status-loss"}),
        ),
        _rule("status.sl-fuse-stale-status", ("expected", "observed")),
    ),
    "readdir": (
        _rule("readdir.duplicate-entry", controls=frozenset({"duplicate-entry"})),
    ),
    "checkout": (
        _rule("checkout.parent-mismatch", ("via",), kind=SubjectKind.CHECKOUT),
    ),
}


def _subject(token: str) -> Subject:
    if token == "checkout:all":
        return Subject(SubjectKind.CHECKOUT, token)
    return path_subject(token.removeprefix("path:"))


def _collector(property_id: str):  # noqa: ANN202 - a Collector
    def collect(check_input: CheckInput, sink: CollectorSink) -> None:
        for entry in check_input.spec.metadata.get(SCRIPT, ()):
            if entry.get("raise") == property_id:
                raise RuntimeError(f"scripted failure in {property_id}")
            rule_id = entry.get("rule", "")
            if not rule_id.startswith(f"{property_id}."):
                continue
            identity = tuple(
                Witness(r, v) for r, v in entry.get("identity", {}).items()
            )
            ref = sink.obligation(
                rule_id,
                entry["scope"],
                subject=_subject(entry["subject"]),
                witnesses=identity,
            )
            status = entry.get("status", "violated")
            wide = WIDE * entry.get("wide", 0)
            if status == "violated":
                sink.violated(
                    ref,
                    Attribution(entry.get("attribution", "product")),
                    wide or entry.get("reason", f"{rule_id} violated"),
                    tuple(
                        Witness(r, wide or v)
                        for r, v in entry.get("evidence", {}).items()
                    ),
                )
            elif status == "satisfied":
                sink.satisfied(ref)
            else:
                sink.not_evaluated(ref, f"{property_id}.{UNOBSERVED}", "scripted")

    return collect


def _property(property_id: str) -> PropertyDef:
    return PropertyDef(
        id=property_id,
        collect=_collector(property_id),
        rules={rule.id: rule for rule in RULES[property_id]},
        not_evaluated_reasons={
            f"{property_id}.{UNOBSERVED}": "The script left this obligation unjudged"
        },
        consumes=(
            frozenset({(READ, "bytes")}) if property_id == "bytes" else frozenset()
        ),
    )


PROPERTIES = tuple(_property(pid) for pid in RULES)

SCRIPTED = Target(
    name="scripted",
    actions=ACTIONS,
    sites={},
    properties=PROPERTIES,
    install_witnessed=FAKE.install_witnessed,
    environment_fields={},
    make_environment=FAKE.make_environment,
    make_backend=FAKE.make_backend,
    red_controls=frozenset(
        control
        for rules in RULES.values()
        for rule in rules
        for control in rule.red_controls
    ),
    semantics_profile="scripted-v1",
    serving_paths={"fake": "memory"},
    sources=TargetSources(implementation=__name__),
)


@dataclass(frozen=True)
class Ob:
    """One scripted obligation."""

    rule: str
    subject: str = "path:r0"
    status: str = "violated"
    attribution: str = "product"
    identity: Mapping[str, str] = field(default_factory=dict)
    evidence: Mapping[str, str] = field(default_factory=dict)
    scope: str = ""
    reason: str = ""
    wide: int = 0

    def entry(self, index: int) -> dict[str, Any]:
        """Return the script entry, scoping it uniquely when no scope is given.

        `wide` > 0 makes the reason and every evidence value `WIDE * wide`.
        """
        return {
            "wide": self.wide,
            "rule": self.rule,
            "subject": self.subject,
            "status": self.status,
            "attribution": self.attribution,
            "identity": dict(self.identity),
            "evidence": dict(self.evidence),
            "scope": self.scope or f"{self.subject}#{index}",
            "reason": self.reason or f"{self.rule} on {self.subject}",
        }


def torn(subject: str = "path:r0", channel: str = "fuse", **kwargs: Any) -> Ob:
    return Ob("bytes.torn-read", subject, identity={"channel": channel}, **kwargs)


def stale(subject: str = "path:r0", channel: str = "fuse", **kwargs: Any) -> Ob:
    return Ob("bytes.stale-read", subject, identity={"channel": channel}, **kwargs)


def status(
    subject: str = "path:r0",
    expected: str = "M",
    observed: str = "absent",
    **kwargs: Any,
) -> Ob:
    return Ob(
        "status.model-live-stale-status",
        subject,
        identity={"expected": expected, "observed": observed},
        **kwargs,
    )


def script(*obligations: Ob, raises: Sequence[str] = ()) -> list[dict[str, Any]]:
    """Return a RunSpec script: the obligations, then any property failures."""
    return [o.entry(i) for i, o in enumerate(obligations)] + [
        {"raise": pid} for pid in raises
    ]


def spec_document(
    entries: Sequence[Mapping[str, Any]] = (),
    *,
    seed: int = 7,
    target: str = "scripted",
    actions: Mapping[str, Mapping[str, Any]] | None = None,
) -> dict[str, Any]:
    """Return a fake write-then-read RunSpec document carrying a script."""
    document: dict[str, Any] = {
        "schema": "eden" + "fs-exploratory-run",
        "schema_version": 1,
        "seed": seed,
        "target": target,
        "environment": {"target": {}},
        "budgets": {
            "operation_deadline_seconds": 60,
            "cleanup_deadline_seconds": 10,
            "runner_deadline_seconds": 120,
        },
        "actors": [{"id": "a", "actions": list(actions or ("w", "r"))}],
        "actions": dict(
            actions
            or {
                "w": {
                    "kind": "write",
                    "deadline": "operation",
                    "path": "register",
                    "value_id": "written",
                },
                "r": {"kind": "read", "deadline": "operation", "path": "register"},
            }
        ),
    }
    if entries:
        document["metadata"] = {SCRIPT: list(entries)}
    return document


def make_spec(
    entries: Sequence[Mapping[str, Any]] = (),
    *,
    target: Target = SCRIPTED,
    **kwargs: Any,
) -> RunSpec:
    """Parse `spec_document` under `target`."""
    return from_json(
        json.dumps(spec_document(entries, target=target.name, **kwargs)), target
    )


COMPLETE_ATTESTATION: Mapping[str, Any] = {
    "binaries": {
        "daemon": {"sha256": "sha256:" + "a" * 64, "target": "fbcode//x:daemon"},
    },
    "runtime_source": {"digest": "sha256:" + "b" * 64, "missing": []},
    "effective_config": "sha256:" + "c" * 64,
    "fixture": "sha256:" + "d" * 64,
    "build_revision": "0" * 40,
    "missing_required": [],
    "missing_advisory": [],
    "complete": True,
}


def history_of(
    spec: RunSpec,
    target: Target = SCRIPTED,
    *,
    attestation: Mapping[str, Any] | None = COMPLETE_ATTESTATION,
    red_control: str | None = None,
) -> list[Event]:
    """Run the spec in process on the fake backend and return a completed history.

    The history starts with a run-level `target-setup` event carrying the
    attestation (when given) and ends with the scheduler's cleanup record.
    """
    environment = target.make_environment(spec, None)
    environment.setup()
    backend = target.make_backend(environment, spec, red_control)
    sink = InMemorySink()
    run_actions(spec, backend, sink, time.monotonic)
    events = list(sink.events)
    if attestation is not None:
        first = events[0].time if events else 0.0
        events.insert(
            0,
            Event("target-setup", first, {"setup": {"attestation": dict(attestation)}}),
        )
    last = events[-1].time if events else 0.0
    events.append(Event(CLEANUP, last, {"scheduler": {"status": "completed"}}))
    return events


COMPLETED_EXECUTION: Mapping[str, Any] = {
    "status": "completed",
    "reason": "",
    "events": 0,
    "returncode": 0,
    "runner_phase": None,
    "cleanup": None,
    "setup": None,
    "daemon_exit": None,
    "setup_error": None,
    "backend_error": None,
}


def judge(
    entries: Sequence[Mapping[str, Any]] = (),
    *,
    target: Target = SCRIPTED,
    execution: Mapping[str, Any] | None = COMPLETED_EXECUTION,
    **kwargs: Any,
) -> Verdict:
    """Return the real verdict of a scripted run."""
    spec = make_spec(entries, target=target, **kwargs)
    return check(
        target,
        spec,
        history_of(spec, target),
        execution=None if execution is None else dict(execution),
    )


def write_run(
    run_dir: Path,
    entries: Sequence[Mapping[str, Any]] = (),
    *,
    target: Target = SCRIPTED,
    execution: Mapping[str, Any] | None = COMPLETED_EXECUTION,
    attestation: Mapping[str, Any] | None = COMPLETE_ATTESTATION,
    red_control: str | None = None,
    **kwargs: Any,
) -> Verdict:
    """Write a whole run directory (spec, history, execution, verdict) and return its verdict."""
    spec = make_spec(entries, target=target, **kwargs)
    events = history_of(spec, target, attestation=attestation, red_control=red_control)
    run_dir.mkdir(parents=True, exist_ok=True)
    (run_dir / "run-spec.json").write_text(to_json(spec))
    with open(run_dir / "history.jsonl", "wb") as handle:
        for event in events:
            handle.write(event_line(event))
    document = None if execution is None else {**execution, "events": len(events)}
    if document is not None:
        (run_dir / "execution.json").write_text(
            json.dumps(document, sort_keys=True, indent=2) + "\n"
        )
    verdict = check(target, spec, events, execution=document)
    (run_dir / "verdict.json").write_bytes(encode_verdict(verdict))
    (run_dir / "stderr.log").write_text("")
    return verdict


def summary_line(seed: int, run_dir: Path, verdict: Verdict) -> dict[str, Any]:
    """Return the `summary.jsonl` row a campaign writes for one finished seed."""
    return {
        "seed": seed,
        "stratum": None,
        "shape_signature": None,
        "status": "completed",
        "reason": "",
        "verdict": verdict.kind,
        "verdict_reason": verdict.reason,
        "not_evaluated_cause": verdict.not_evaluated_cause,
        "findings": [],
        "verdict_bytes": (run_dir / "verdict.json").stat().st_size,
        "artifacts": str(run_dir),
    }


def keys_of(verdict: Verdict, target: Target = SCRIPTED) -> list[str]:
    """Return a verdict's task keys, in key order."""
    return [o.task_key for o in keyed(verdict, target).occurrences]


def subjects_of(verdict: Verdict, target: Target = SCRIPTED) -> dict[str, list[str]]:
    """Return each task key's subjects."""
    return {o.task_key: list(o.subjects) for o in keyed(verdict, target).occurrences}


def obligations(verdict: Verdict) -> Iterable[Any]:
    """Every obligation of a verdict, in property order."""
    assert verdict.report is not None
    return (o for r in verdict.report.results for o in r.obligations)


DIGEST = "sha256:" + "0" * 64


def synthetic_verdict(
    *results: PropertyResult,
    bindings: Bindings | None = None,
    identity: str = DIGEST,
) -> Verdict:
    """Return a verdict over the given target results, shaped like a registry.

    The core properties lead with no obligations; the projection is
    recomputed, so the verdict round-trips through `decode_verdict`.
    """
    full = (*(PropertyResult(pid) for pid in CORE_PROPERTY_IDS), *results)
    kind, reason, cause = project(full)
    report = PropertyReport(
        full, identity, bindings or Bindings(DIGEST, DIGEST, DIGEST, DIGEST)
    )
    return Verdict(
        kind, reason, lanes=lanes(full), not_evaluated_cause=cause, report=report
    )


def satisfied_bytes() -> PropertyResult:
    """One satisfied byte obligation: an accepting target result."""
    return PropertyResult(
        "bytes",
        (
            ObligationResult(
                rule_id="bytes.torn-read",
                scope="r",
                subject=path_subject("r0"),
                status=ObligationStatus.SATISFIED,
                witnesses=(Witness("channel", "fuse"),),
            ),
        ),
    )


def findings_of(verdict: Verdict) -> Iterable[Any]:
    """Every finding (violated obligation) of a verdict, in property order."""
    assert verdict.report is not None
    return (f for r in verdict.report.results for f in r.findings)


def exploration_result(
    status: str,
    verdict: str,
    reason: str,
    artifacts: str,
    *,
    execution_reason: str | None = None,
    returncode: int = 0,
    setup: dict[str, Any] | None = None,
) -> ExplorationResult:
    return ExplorationResult(
        Execution(status, execution_reason or status, 0, returncode, setup=setup),
        Verdict(verdict, reason),
        artifacts,
    )


def register_actions(**reads: str) -> dict[str, dict[str, str]]:
    """A write of `register` followed by one read per keyword, of the path given."""
    actions = {
        "w": {
            "kind": "write",
            "deadline": "operation",
            "path": "register",
            "value_id": "written",
        }
    }
    for action_id, path in reads.items():
        actions[action_id] = {"kind": "read", "deadline": "operation", "path": path}
    return actions
