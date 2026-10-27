# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Canonical symptom-set identity of product-actionable findings."""

from __future__ import annotations

from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from typing import Any

from .jsonio import digest
from .property import (
    Finding,
    identity_witnesses,
    PRODUCT_ATTRIBUTIONS,
    RuleDef,
    Subject,
)

IDENTITY_KEY_SCHEMA = "exploratory-error-set-v1"

CanonicalError = tuple[str, str, tuple[tuple[str, str], ...]]


@dataclass(frozen=True)
class ErrorSet:
    """The canonical errors of one exact subject occurrence and their key.

    `errors` are sorted and deduplicated `(rule_id, finding_kind, identity
    roles)` triples; `key` hashes them and nothing else, so the subject
    token, action IDs, instance witnesses, target, profile, and digests
    never change it. `findings` keeps the evidence behind the key.
    """

    subject: Subject
    errors: tuple[CanonicalError, ...]
    key: str
    findings: tuple[Finding, ...]


@dataclass(frozen=True)
class CanonicalErrors:
    """Every error set, in subject order, plus findings no identity classifies.

    A product-actionable finding whose rule is unknown or whose identity
    roles are missing or ambiguous is `unclassified`: kept, never keyed.
    """

    sets: tuple[ErrorSet, ...]
    unclassified: tuple[Finding, ...]


def canonical_roles(roles: Iterable[tuple[str, str]]) -> tuple[tuple[str, str], ...]:
    """Return one error's identity roles sorted by role name.

    Raises ValueError when a role name repeats: one error carries one value
    per declared role.
    """
    ordered = tuple(sorted(roles))
    names = [role for role, _value in ordered]
    if len(set(names)) != len(names):
        raise ValueError(f"duplicate identity role in {ordered!r:.200}")
    return ordered


def error_set_document(errors: Iterable[CanonicalError]) -> dict[str, Any]:
    """Return the canonical key document of one error set.

    Errors are sorted and deduplicated, and each error's roles are sorted by
    role name (`canonical_roles`, which refuses a repeated role name).
    """
    canonical = {
        (rule_id, kind, canonical_roles(roles)) for rule_id, kind, roles in errors
    }
    return {
        "schema": IDENTITY_KEY_SCHEMA,
        "errors": [
            [rule_id, kind, [list(role) for role in roles]]
            for rule_id, kind, roles in sorted(canonical)
        ],
    }


def error_set_key(errors: Iterable[CanonicalError]) -> str:
    """Return the sha256 key of one canonical error set: its document's digest."""
    return digest(error_set_document(errors))


def canonical_errors(
    findings: Iterable[Finding], rules: Mapping[str, RuleDef]
) -> CanonicalErrors:
    """Partition product-actionable findings by subject and key each error set.

    Only `product` and `undetermined` findings take part. Each finding
    becomes `(rule_id, finding kind, identity roles)`; findings of one
    subject token form one sorted, deduplicated set.
    """
    grouped: dict[Subject, tuple[set[CanonicalError], list[Finding]]] = {}
    unclassified: list[Finding] = []
    for finding in findings:
        if finding.attribution not in PRODUCT_ATTRIBUTIONS:
            continue
        rule = rules.get(finding.rule_id)
        roles = None if rule is None else identity_witnesses(rule, finding)
        if rule is None or roles is None:
            unclassified.append(finding)
            continue
        errors, evidence = grouped.setdefault(finding.subject, (set(), []))
        errors.add((rule.id, rule.finding_kind, roles))
        evidence.append(finding)
    sets = tuple(
        ErrorSet(subject, tuple(sorted(errors)), error_set_key(errors), tuple(evidence))
        for subject, (errors, evidence) in sorted(
            grouped.items(), key=lambda item: (item[0].kind, item[0].token)
        )
    )
    return CanonicalErrors(sets, tuple(unclassified))
