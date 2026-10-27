# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Build the expected-results corpus and compare a checker's obligations against it.

`cases()` renders every synthetic case of the authoring source
(`families.runs()`) in memory: its RunSpec text, its history bytes, and
its expectation document, parsed here. Nothing is read from disk;
`export` writes one case out for manual replay.

Parsing is strict: an unknown or missing key, or a value of the wrong
type, raises `CorpusError`. `mismatches` lets a later diff assert the
expectations of the rules it has registered.
"""

from __future__ import annotations

import functools
import json
from collections.abc import Collection, Iterable, Mapping
from dataclasses import dataclass
from typing import Any

from eden.integration.correctness.exploratory.core.history import Event, event_from_json
from eden.integration.correctness.exploratory.core.identity import (
    CanonicalError,
    error_set_key,
)
from eden.integration.correctness.exploratory.core.runspec import from_json, RunSpec
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.test.corpus import families
from eden.integration.correctness.exploratory.test.corpus.author import (
    EXPECTATION_SCHEMA,
)
from eden.integration.correctness.exploratory.test.corpus.catalog import catalog
from eden.integration.correctness.exploratory.test.waiting import WAITING_ON

OUTCOMES = ("satisfied", "violated", "not-evaluated")
VERDICTS = ("accept", "reject", "harness-suspect", "not-evaluated")
PRODUCT_ATTRIBUTIONS = frozenset({"product", "undetermined"})
# Core properties are part of every target's registry and judge no target
# behavior: a case only they would check needs a target rule first.


class CorpusError(ValueError):
    """A rendered expectation does not have its declared shape."""


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise CorpusError(message)


def _exact(
    raw: Any, required: set[str], optional: set[str], what: str
) -> dict[str, Any]:
    _require(type(raw) is dict, f"{what}: not an object")
    keys = set(raw)
    _require(required <= keys, f"{what}: missing {sorted(required - keys)}")
    _require(
        keys <= required | optional,
        f"{what}: unknown {sorted(keys - required - optional)}",
    )
    return raw


def _text(raw: Any, what: str) -> str:
    _require(type(raw) is str and raw != "", f"{what}: not non-empty text")
    return raw


def _texts(raw: Any, what: str) -> tuple[str, ...]:
    _require(type(raw) is list, f"{what}: not a list")
    return tuple(_text(item, f"{what}[]") for item in raw)


@dataclass(frozen=True)
class ExpectedObligation:
    """One `(rule, anchor action, subject)` outcome a case declares.

    `outcome` is exact. An obligation the engine may decide or leave
    undecided (budget-dependent) instead names `outcome_in`, a set of two
    or more outcomes that never includes `violated`, so it never changes
    the declared canonical error sets; `outcome` is then empty. A violated
    obligation names one error by `identity`, plus one per
    `more_identities` entry: errors of further scopes of the same action
    that the rule judges separately (`checkout.sl-abort`'s
    `<checkout>:<part>` scopes).
    """

    rule_id: str
    anchor: str
    subject: str
    outcome: str
    reason: str
    attribution: str | None
    identity: Mapping[str, str]
    not_evaluated_reason: str | None
    outcome_in: tuple[str, ...] = ()
    more_identities: tuple[Mapping[str, str], ...] = ()

    @property
    def identities(self) -> tuple[Mapping[str, str], ...]:
        """Return the identity of every error this obligation declares."""
        return (self.identity, *self.more_identities)

    @property
    def allowed(self) -> tuple[str, ...]:
        """Return every outcome this expectation accepts."""
        return self.outcome_in or (self.outcome,)

    @property
    def key(self) -> tuple[str, str, str]:
        return (self.rule_id, self.anchor, self.subject)


def _obligation(raw: Any, what: str) -> ExpectedObligation:
    body = _exact(
        raw,
        {"rule_id", "anchor", "subject", "reason"},
        {
            "outcome",
            "outcome_in",
            "attribution",
            "identity",
            "more_identities",
            "not_evaluated_reason",
        },
        what,
    )
    _require(
        ("outcome" in body) != ("outcome_in" in body),
        f"{what}: exactly one of outcome and outcome_in",
    )
    outcome_in: tuple[str, ...] = ()
    if "outcome_in" in body:
        outcome_in = _texts(body["outcome_in"], f"{what}.outcome_in")
        _require(
            len(set(outcome_in)) == len(outcome_in) >= 2
            and set(outcome_in) <= set(OUTCOMES) - {"violated"},
            f"{what}: outcome_in must name two or more distinct non-violated outcomes",
        )
        _require(
            not set(body) & {"attribution", "identity", "not_evaluated_reason"},
            f"{what}: outcome_in pins no attribution, identity, or reason",
        )
    outcome = body.get("outcome", "")
    _require(
        outcome_in != () or outcome in OUTCOMES, f"{what}: bad outcome {outcome!r}"
    )
    violated = outcome == "violated"
    _require(("attribution" in body) == violated, f"{what}: attribution iff violated")
    _require(("identity" in body) == violated, f"{what}: identity iff violated")
    _require(
        ("not_evaluated_reason" in body) == (outcome == "not-evaluated"),
        f"{what}: not_evaluated_reason iff not-evaluated",
    )
    _require(
        "more_identities" not in body or violated,
        f"{what}: more_identities only when violated",
    )
    more = body.get("more_identities", [])
    _require(
        type(more) is list and bool(more) == ("more_identities" in body),
        f"{what}: more_identities must be a non-empty list",
    )
    identity = body.get("identity", {})
    for item in (identity, *more):
        _require(
            type(item) is dict
            and all(type(k) is str and type(v) is str and v for k, v in item.items()),
            f"{what}: identity must map roles to text",
        )
    return ExpectedObligation(
        rule_id=_text(body["rule_id"], f"{what}.rule_id"),
        anchor=_text(body["anchor"], f"{what}.anchor"),
        subject=_text(body["subject"], f"{what}.subject"),
        outcome=outcome,
        reason=_text(body["reason"], f"{what}.reason"),
        attribution=body.get("attribution"),
        identity=dict(identity),
        not_evaluated_reason=body.get("not_evaluated_reason"),
        outcome_in=outcome_in,
        more_identities=tuple(dict(item) for item in more),
    )


@dataclass(frozen=True)
class ExpectedErrorSet:
    """The canonical error set and task key one subject's product findings form."""

    subject: str
    errors: tuple[CanonicalError, ...]
    key: str


def _error_set(raw: Any, what: str) -> ExpectedErrorSet:
    body = _exact(raw, {"subject", "errors", "key"}, set(), what)
    _require(
        type(body["errors"]) is list and len(body["errors"]) > 0, f"{what}: no errors"
    )
    errors = []
    for item in body["errors"]:
        _require(
            type(item) is list and len(item) == 3 and type(item[2]) is list,
            f"{what}: error must be [rule, kind, roles]",
        )
        roles = tuple(
            (_text(role[0], what), _text(role[1], what))
            for role in item[2]
            if type(role) is list and len(role) == 2
        )
        _require(len(roles) == len(item[2]), f"{what}: role must be [role, value]")
        errors.append((_text(item[0], what), _text(item[1], what), roles))
    return ExpectedErrorSet(
        _text(body["subject"], f"{what}.subject"),
        tuple(errors),
        _text(body["key"], f"{what}.key"),
    )


@dataclass(frozen=True)
class Expectation:
    """What one case declares: legality, verdict, obligations, canonical errors."""

    case: str
    family: str
    description: str
    basis: tuple[str, ...]
    legal: bool
    red_control: str | None
    execution: Mapping[str, Any]
    verdict: str
    obligations: tuple[ExpectedObligation, ...]
    canonical_errors: tuple[ExpectedErrorSet, ...]


def parse_expectation(raw: Any, what: str = "expectation") -> Expectation:
    """Return a case expectation, or raise `CorpusError` naming the bad field."""
    body = _exact(
        raw,
        {
            "schema",
            "case",
            "family",
            "description",
            "basis",
            "legal",
            "red_control",
            "execution",
            "verdict",
            "obligations",
            "canonical_errors",
        },
        set(),
        what,
    )
    _require(body["schema"] == EXPECTATION_SCHEMA, f"{what}: schema {body['schema']!r}")
    _require(type(body["legal"]) is bool, f"{what}: legal must be bool")
    red_control = body["red_control"]
    _require(
        red_control is None or (type(red_control) is str and red_control != ""),
        f"{what}: red_control must be null or text",
    )
    _require(body["verdict"] in VERDICTS, f"{what}: bad verdict {body['verdict']!r}")
    execution = _exact(body["execution"], {"status"}, set(), f"{what}.execution")
    _require(type(body["obligations"]) is list, f"{what}: obligations not a list")
    _require(
        type(body["canonical_errors"]) is list, f"{what}: canonical_errors not a list"
    )
    return Expectation(
        case=_text(body["case"], f"{what}.case"),
        family=_text(body["family"], f"{what}.family"),
        description=_text(body["description"], f"{what}.description"),
        basis=_texts(body["basis"], f"{what}.basis"),
        legal=body["legal"],
        red_control=red_control,
        execution=dict(execution),
        verdict=body["verdict"],
        obligations=tuple(
            _obligation(item, f"{what}.obligations[{index}]")
            for index, item in enumerate(body["obligations"])
        ),
        canonical_errors=tuple(
            _error_set(item, f"{what}.canonical_errors[{index}]")
            for index, item in enumerate(body["canonical_errors"])
        ),
    )


@dataclass(frozen=True)
class CorpusCase:
    """One synthetic case: RunSpec text, history bytes, parsed expectation."""

    name: str
    spec_text: str
    history_bytes: bytes
    expectation: Expectation

    def spec(self, target: Target) -> RunSpec:
        """Return the case's RunSpec, validated against `target`."""
        return from_json(self.spec_text, target)

    @property
    def waiting_on(self) -> str | None:
        """Return the task the case waits on: `waiting_on` in its RunSpec metadata."""
        return json.loads(self.spec_text).get("metadata", {}).get(WAITING_ON)

    def events(self) -> tuple[Event, ...]:
        """Return the history's events in recorded order."""
        return tuple(
            event_from_json(json.loads(line))
            for line in self.history_bytes.decode("utf-8").splitlines()
        )


@functools.cache
def cases() -> tuple[CorpusCase, ...]:
    """Return every history case, sorted by name, rendered once per process."""
    found: dict[str, CorpusCase] = {}
    for run in families.runs():
        name = f"{run.family}/{run.name}"
        _require(name not in found, f"two corpus cases named {name!r}")
        found[name] = CorpusCase(
            name,
            run.spec_text(),
            run.history_bytes(),
            parse_expectation(run.expectation(), name),
        )
    return tuple(found[name] for name in sorted(found))


def case(name: str) -> CorpusCase:
    """Return one history case by `<family>/<case>` name."""
    matches = [item for item in cases() if item.name == name]
    _require(len(matches) == 1, f"no corpus case {name!r}")
    return matches[0]


def rules() -> Mapping[str, Any]:
    """Return the rule and not-evaluated reason catalog."""
    return catalog()


@dataclass(frozen=True)
class ObservedObligation:
    """One obligation a checker reached, mapped to the corpus's anchor convention."""

    rule_id: str
    anchor: str
    subject: str
    outcome: str
    attribution: str | None = None
    not_evaluated_reason: str | None = None
    identity: Mapping[str, str] | None = None


def _error_sets(
    items: Iterable[tuple[str, CanonicalError]],
) -> dict[str, frozenset[CanonicalError]]:
    grouped: dict[str, set[CanonicalError]] = {}
    for subject, error in items:
        grouped.setdefault(subject, set()).add(error)
    return {subject: frozenset(errors) for subject, errors in grouped.items()}


def mismatches(
    expectation: Expectation,
    observed: Iterable[ObservedObligation],
    *,
    registered: Collection[str],
    verdict: str | None = None,
) -> tuple[str, ...]:
    """Return how observed obligations disagree with a case, for registered rules only.

    Four checks, each restricted to rules in `registered`: every
    declared obligation is observed
    with its outcome (and attribution or not-evaluated reason), or with
    one outcome of its `outcome_in`; every observed obligation a case
    does not declare is satisfied; a legal case has no
    product-actionable violation; and the product-actionable violations
    form exactly the declared canonical error sets.
    When the observed `verdict` kind is given (a case judged in full), it
    must be the declared one: an expected `accept` that came back
    `not-evaluated` is a mismatch, however its obligations look.
    """
    seen = list(observed)
    by_key: dict[tuple[str, str, str], list[ObservedObligation]] = {}
    for item in seen:
        by_key.setdefault((item.rule_id, item.anchor, item.subject), []).append(item)
    problems: list[str] = []
    if verdict is not None and verdict != expectation.verdict:
        problems.append(f"verdict: expected {expectation.verdict}, observed {verdict}")
    for expected in expectation.obligations:
        if expected.rule_id not in registered:
            continue
        found = by_key.get(expected.key, [])
        if not any(
            item.outcome in expected.allowed
            and (
                expected.outcome != "violated"
                or item.attribution == expected.attribution
            )
            and (
                bool(expected.outcome_in)
                or item.not_evaluated_reason == expected.not_evaluated_reason
            )
            for item in found
        ):
            got = [
                (item.outcome, item.attribution, item.not_evaluated_reason)
                for item in found
            ]
            problems.append(
                f"{expected.key}: expected {list(expected.allowed)}, observed {got}"
            )
    declared = {expected.key for expected in expectation.obligations}
    for item in seen:
        key = (item.rule_id, item.anchor, item.subject)
        if (
            item.rule_id in registered
            and key not in declared
            and item.outcome != "satisfied"
        ):
            problems.append(f"{key}: undeclared, observed {item.outcome}")
    product = [
        item
        for item in seen
        if item.rule_id in registered
        and item.outcome == "violated"
        and item.attribution in PRODUCT_ATTRIBUTIONS
    ]
    if expectation.legal and product:
        problems.append(
            f"legal case has product violations {[p.rule_id for p in product]}"
        )
    observed_sets = _error_sets(
        (
            item.subject,
            (
                item.rule_id,
                item.rule_id.rsplit(".", 1)[1],
                tuple(sorted((item.identity or {}).items())),
            ),
        )
        for item in product
    )
    declared_sets = _error_sets(
        (error_set.subject, error)
        for error_set in expectation.canonical_errors
        for error in error_set.errors
        if error[0] in registered
    )
    if observed_sets != declared_sets:
        problems.append(
            "canonical errors differ: "
            f"expected {sorted((s, error_set_key(e)) for s, e in declared_sets.items())}, "
            f"observed {sorted((s, error_set_key(e)) for s, e in observed_sets.items())}"
        )
    return tuple(problems)
