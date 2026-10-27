# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Authoring source of the synthetic expected-results corpus.

Every case is synthetic: a RunSpec and a history written by this module,
never captured from a live run, plus the obligation outcomes the
checker must reach. `loader.cases()` renders them in memory;
`export` writes one case to disk for manual replay.

Expectations are stated at the rule level from the EdenFS and Sapling
behavior each case's `basis` names; nothing here runs a checker.
"""

from __future__ import annotations

import hashlib
import json
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from typing import Any

from eden.integration.correctness.exploratory.core.checker import input_subject
from eden.integration.correctness.exploratory.core.history import Event, event_line
from eden.integration.correctness.exploratory.core.identity import error_set_key
from eden.integration.correctness.exploratory.core.property import path_subject
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_committed,
    encode_value,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import VOCAB_TARGET
from eden.integration.correctness.repo import GeneratedRepo, TypeChange, WideDir

EXPECTATION_SCHEMA = "eden-correctness-corpus-expectation-v1"
SEED = 40404
TIP = 3
R0 = "tree/registers/r0.reg"
R1 = "tree/registers/r1.reg"
R2 = "tree/registers/r2.reg"
REGISTERS = (R0, R1, R2)
CHURN = "tree/churn/c0"
MOVED = "tree/registers/moved.reg"
NEW_DIR = "tree/registers/d"
WRITE_OK = {"bytes_written": 64}
SL_ABORT_ADVICE = (
    "(commit, shelve, goto --clean to discard all your changes, "
    "or goto --merge to merge them)"
)
IN_PROGRESS = "another checkout operation is still in progress"

SATISFIED = "satisfied"
VIOLATED = "violated"
NOT_EVALUATED = "not-evaluated"
PRODUCT = "product"


def node(revision: int) -> str:
    """Return the synthetic 40-hex commit node this corpus records for a revision."""
    return hashlib.sha256(f"corpus-revision-{revision}".encode()).hexdigest()[:40]


def committed(path: str, revision: int) -> bytes:
    """Return the bytes the replacement fixture commits for one register."""
    return encode_committed(SEED, path, revision)


def value(writer: str, value_id: str) -> bytes:
    """Return the bytes one write of `writer` (its owning actor) stores."""
    return encode_value(SEED, writer, value_id)


def read_ok(data: bytes) -> dict[str, Any]:
    """Return the facts a successful read of `data` records."""
    return {
        "bytes_prefix": data[:128].hex(),
        "length": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def status_entries(entries: Mapping[str, str]) -> list[dict[str, str]]:
    return [{"path": path, "code": entries[path]} for path in sorted(entries)]


def sl_status(entries: Mapping[str, str], diff: Iterable[str] = ()) -> dict[str, Any]:
    return {
        "status": status_entries(entries),
        "diff_paths": sorted(diff),
        "parent": node(TIP),
    }


def thrift_status(entries: Mapping[str, str], revision: int = TIP) -> dict[str, Any]:
    return {"status": status_entries(entries), "parent": node(revision)}


def thrift_checkout(revision: int, conflicts: int) -> dict[str, Any]:
    return {
        "revision": revision,
        "node": node(revision),
        "via": "thrift",
        "conflicts": conflicts,
    }


def sl_checkout(revision: int, via: str = "goto") -> dict[str, Any]:
    return {
        "revision": revision,
        "node": node(revision),
        "via": via,
        "parent": node(revision),
    }


def sl_abort(paths: Iterable[str], via: str = "goto") -> dict[str, Any]:
    listed = list(paths)
    stderr = (
        f"abort: {len(listed)} conflicting file changes:\n"
        + "".join(f" {path}\n" for path in listed)
        + SL_ABORT_ADVICE
        + "\n"
    )
    return {
        "reason": f"{via}: {stderr.strip()}",
        "returncode": 255,
        "stdout": "",
        "stderr": stderr,
    }


def thrift_rejected() -> dict[str, Any]:
    return {
        "reason": IN_PROGRESS,
        "source": "thrift",
        "error_type": "CHECKOUT_IN_PROGRESS",
    }


def errno_fail(code: int, reason: str = "syscall failed") -> dict[str, Any]:
    return {"reason": reason, "errno": code}


@dataclass
class Obligation:
    rule_id: str
    anchor: str
    subject: str
    outcome: str
    reason: str
    identity: Mapping[str, str] = field(default_factory=dict)
    attribution: str | None = None
    not_evaluated_reason: str | None = None
    outcome_in: tuple[str, ...] = ()
    # Identities of further errors of the same rule at the anchor: other
    # scopes of one action the rule judges separately (`checkout.sl-abort`).
    more_identities: tuple[Mapping[str, str], ...] = ()

    def document(self) -> dict[str, Any]:
        doc: dict[str, Any] = {
            "rule_id": self.rule_id,
            "anchor": self.anchor,
            "subject": self.subject,
            "reason": self.reason,
        }
        if self.outcome_in:
            doc["outcome_in"] = list(self.outcome_in)
            return doc
        doc["outcome"] = self.outcome
        if self.outcome == VIOLATED:
            doc["attribution"] = self.attribution or PRODUCT
            doc["identity"] = dict(sorted(self.identity.items()))
            if self.more_identities:
                doc["more_identities"] = [
                    dict(sorted(item.items())) for item in self.more_identities
                ]
        if self.outcome == NOT_EVALUATED:
            doc["not_evaluated_reason"] = self.not_evaluated_reason
        return doc


class Run:
    """One synthetic case: its workload, its history, and its expectations."""

    def __init__(
        self,
        family: str,
        name: str,
        description: str,
        *,
        basis: Iterable[str] = (),
        reference: bool = False,
        registers: Iterable[str] = REGISTERS,
        red_control: str | None = None,
        type_changes: Iterable[Mapping[str, Any]] = (),
        wide_dir: Mapping[str, Any] | None = None,
    ) -> None:
        self.family = family
        self.red_control = red_control
        self.type_changes = [dict(item) for item in type_changes]
        self.wide_dir = None if wide_dir is None else dict(wide_dir)
        self.name = name
        self.description = description
        self.basis = list(basis)
        self.reference = reference
        self.registers = list(registers)
        self.actors: dict[str, list[str]] = {}
        self.actions: dict[str, dict[str, Any]] = {}
        self.events: list[Event] = []
        self.obligations: list[Obligation] = []
        # Extra RunSpec metadata, such as a declaration the validators read.
        self.metadata: dict[str, Any] = {}
        self.clock = 10
        self._owner: dict[str, str] = {}
        self._spec: Any = None

    # Workload.
    def act(
        self,
        actor: str,
        action_id: str,
        kind: str,
        *,
        requires: Iterable[tuple[str, str]] = (),
        deadline: str = "operation",
        **payload: Any,
    ) -> str:
        assert action_id not in self.actions, action_id
        body: dict[str, Any] = {"kind": kind, "deadline": deadline, **payload}
        if requires:
            body["requires"] = [list(item) for item in requires]
        self.actions[action_id] = body
        self.actors.setdefault(actor, []).append(action_id)
        self._owner[action_id] = actor
        return action_id

    # History.
    def tick(self, tenths: int = 1) -> float:
        self.clock += tenths
        return self.clock / 10

    def wait(self, seconds: int) -> None:
        self.clock += seconds * 10

    def inv(self, *action_ids: str) -> None:
        for action_id in action_ids:
            self.events.append(
                Event(
                    "invoke",
                    self.tick(),
                    {"action": action_id, "actor": self._owner[action_id]},
                )
            )

    def done(self, action_id: str, outcome: str, **facts: Any) -> None:
        self.events.append(
            Event(
                outcome,
                self.tick(),
                {"action": action_id, "actor": self._owner[action_id], **facts},
            )
        )

    def call(self, action_id: str, outcome: str, **facts: Any) -> None:
        self.inv(action_id)
        self.done(action_id, outcome, **facts)

    def paired(
        self,
        action_id: str,
        outcome: str,
        ref_outcome: str | None = None,
        ref_facts: Mapping[str, Any] | None = None,
        **facts: Any,
    ) -> None:
        """Invoke and complete one shared action carrying its reference side."""
        self.inv(action_id)
        start = self.tick()
        end = self.tick()
        side = {
            "schema_version": 1,
            "outcome": ref_outcome or outcome,
            "started": start,
            "completed": end,
            "facts": dict(facts if ref_facts is None else ref_facts),
        }
        self.done(action_id, outcome, reference_result=side, **facts)

    def run_event(self, type: str, **data: Any) -> None:
        self.events.append(Event(type, self.tick(), data))

    def fault_removed(self, fault_id: str, hit: bool = True) -> None:
        self.run_event(
            "cleanup-removed",
            fault_id=fault_id,
            leak=False,
            blocked_keys=[],
            removed=0,
            cleanup_retries=0,
            hit=hit,
            target_completed=True,
            consumed=None,
            miss=not hit,
        )

    def finish(self) -> None:
        self.run_event(
            "target-closed",
            host_oom_after_close={},
            reference=None,
            close_errors=[],
        )
        self.run_event("cleanup", scheduler={"status": "completed", "reason": None})

    # Expectations.
    def spec(self) -> Any:
        if self._spec is None:
            self._spec = from_json(self.spec_text(), VOCAB_TARGET)
        return self._spec

    def layout(self) -> GeneratedRepo:
        """Return the generated layout this case's repository describes."""
        return GeneratedRepo(
            SEED,
            64,
            TIP + 1,
            wide_dir=None if self.wide_dir is None else WideDir(**self.wide_dir),
            type_changes=tuple(TypeChange(**change) for change in self.type_changes),
        )

    def input_subject(self, action_id: str) -> str:
        spec = self.spec()
        return input_subject(VOCAB_TARGET, spec, spec.action(action_id)).token

    def expect(
        self,
        rule_id: str,
        anchor: str,
        outcome: str,
        reason: str,
        *,
        path: str | None = None,
        subject: str | None = None,
        **kwargs: Any,
    ) -> None:
        if subject is None:
            subject = (
                path_subject(path).token
                if path is not None
                else f"checkout:{anchor.split(':')[0]}"
            )
        self.obligations.append(
            Obligation(rule_id, anchor, subject, outcome, reason, **kwargs)
        )

    def legal_input(self, anchor: str, reason: str = "the input succeeded") -> None:
        self.expect(
            "core.input-legality",
            anchor,
            SATISFIED,
            reason,
            subject=self.input_subject(anchor),
        )

    def bad_input(
        self, anchor: str, errno_name: str, reason: str, **kwargs: Any
    ) -> None:
        kind = self.actions[anchor]["kind"]
        self.expect(
            "core.input-legality",
            anchor,
            VIOLATED,
            reason,
            subject=self.input_subject(anchor),
            identity={"input-kind": kind, "errno": errno_name},
            **kwargs,
        )

    # Rendering.
    def spec_document(self) -> dict[str, Any]:
        profile: dict[str, Any] = {"materialize": False, "scatter_dirty": 0}
        if self.reference:
            profile["reference_clone"] = True
        if self.wide_dir is not None:
            profile["wide_dir"] = self.wide_dir
        repository: dict[str, Any] = {
            "file_count": 64,
            "commit_count": TIP + 1,
            "registers": self.registers,
        }
        if self.type_changes:
            repository["type_changes"] = self.type_changes
        return {
            "schema": "edenfs-exploratory-run",
            "schema_version": 1,
            "seed": SEED,
            "target": "edenfs",
            "environment": {
                "target": {
                    "repository": repository,
                    "channel": "fuse",
                    "case_sensitive": True,
                    "fixture_source": "backing",
                    "setup_profile": profile,
                }
            },
            "budgets": {
                "operation_deadline_seconds": 120,
                "cleanup_deadline_seconds": 30,
                "runner_deadline_seconds": 600,
            },
            "actors": [
                {"id": actor, "actions": ids} for actor, ids in self.actors.items()
            ],
            "actions": self.actions,
            "metadata": {
                "corpus_case": f"{self.family}/{self.name}",
                "origin": "synthetic",
                **self.metadata,
            },
        }

    def spec_text(self) -> str:
        return json.dumps(self.spec_document(), sort_keys=True, indent=2) + "\n"

    def history_bytes(self) -> bytes:
        return b"".join(event_line(event) for event in self.events)

    def canonical_errors(self) -> list[dict[str, Any]]:
        grouped: dict[str, set[tuple[str, str, tuple[tuple[str, str], ...]]]] = {}
        for item in self.obligations:
            if (
                item.outcome_in
                or item.outcome != VIOLATED
                or (item.attribution or PRODUCT)
                not in (
                    PRODUCT,
                    "undetermined",
                )
            ):
                continue
            for identity in (item.identity, *item.more_identities):
                error = (
                    item.rule_id,
                    item.rule_id.rsplit(".", 1)[1],
                    tuple(sorted(identity.items())),
                )
                grouped.setdefault(item.subject, set()).add(error)
        return [
            {
                "subject": subject,
                "errors": [
                    [rule, kind, [list(role) for role in roles]]
                    for rule, kind, roles in sorted(errors)
                ],
                "key": error_set_key(errors),
            }
            for subject, errors in sorted(grouped.items())
        ]

    def verdict(self) -> str:
        outcomes = [
            (o.outcome, o.attribution or PRODUCT)
            for o in self.obligations
            if not o.outcome_in
        ]
        if any(o == VIOLATED and a in (PRODUCT, "undetermined") for o, a in outcomes):
            return "reject"
        if any(o == VIOLATED for o, _a in outcomes):
            return "harness-suspect"
        if any(o == NOT_EVALUATED for o, _a in outcomes):
            return "not-evaluated"
        return "accept"

    def expectation(self) -> dict[str, Any]:
        errors = self.canonical_errors()
        return {
            "schema": EXPECTATION_SCHEMA,
            "case": f"{self.family}/{self.name}",
            "family": self.family,
            "description": self.description,
            "basis": self.basis,
            "legal": not errors,
            "red_control": self.red_control,
            "execution": {"status": "completed"},
            "verdict": self.verdict(),
            "obligations": [
                item.document()
                for item in sorted(
                    self.obligations, key=lambda o: (o.rule_id, o.anchor, o.subject)
                )
            ],
            "canonical_errors": errors,
        }


@dataclass(frozen=True)
class SpecCase:
    """A RunSpec-only case: differential validation admits or rejects it."""

    name: str
    description: str
    spec: Mapping[str, Any]
    admitted: bool
    error: str = ""
