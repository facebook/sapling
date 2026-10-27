# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Judge authored runs with `EDENFS_TARGET` and read obligations off the verdict."""

from __future__ import annotations

import dataclasses
from types import SimpleNamespace
from typing import Any, cast

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.property import (
    CheckInput,
    ObligationResult,
)
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs import contracts
from eden.integration.correctness.exploratory.edenfs.contracts import (
    checkout as contracts_checkout,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.testing import RecordingSink

COMPLETED = {"status": "completed"}


# Every rule `EDENFS_TARGET` registers: contract inputs may support any of them.
REGISTERED_RULE_IDS = frozenset(
    rule.id for p in checker.registry(EDENFS_TARGET) for rule in p.rules.values()
)


def judge(run: Any) -> checker.Verdict:
    """Return `EDENFS_TARGET`'s verdict on an authored run (`test.corpus.author.Run`)."""
    spec = from_json(run.spec_text(), EDENFS_TARGET)
    return checker.check(EDENFS_TARGET, spec, run.events, execution=COMPLETED)


def obligations_of(verdict: checker.Verdict) -> dict[tuple[str, str], ObligationResult]:
    """Return a verdict's obligations keyed by (rule, scope)."""
    assert verdict.report is not None, verdict.reason
    return {
        (item.rule_id, item.scope): item
        for result in verdict.report.results
        for item in result.obligations
    }


def status_of(verdict: checker.Verdict, rule: str, scope: str) -> str | None:
    """Return one obligation's status, or None when the verdict has no such obligation."""
    item = obligations_of(verdict).get((rule, scope))
    return None if item is None else str(item.status)


def violations_of(verdict: checker.Verdict) -> dict[tuple[str, str], str]:
    """Return the attribution of every violated obligation, keyed by (rule, scope)."""
    return {
        key: str(item.attribution)
        for key, item in obligations_of(verdict).items()
        if str(item.status) == "violated"
    }


def contract_run(
    run: Any, rules: frozenset[str] = contracts_checkout.CONTRACT_RULES
) -> RecordingSink:
    """Return what the contracts record for an authored run under `rules`.

    The parsed spec is kept on the sink as `spec`.
    """
    spec = from_json(run.spec_text(), EDENFS_TARGET)
    facts = checker.HistoryFacts.build(
        spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
    )
    sink = RecordingSink()
    sink.spec = spec
    contracts.record_contracts(spec, facts, None, sink, rules)
    return sink


def obligations_by_scope(
    verdict: checker.Verdict, rule_id: str
) -> dict[str, ObligationResult]:
    """Return one rule's obligations keyed by scope."""
    return {
        scope: item
        for (rule, scope), item in obligations_of(verdict).items()
        if rule == rule_id
    }


def statuses_by_scope(verdict: checker.Verdict, rule_id: str) -> dict[str, str]:
    """Return one rule's obligation statuses keyed by scope."""
    return {
        scope: str(item.status)
        for scope, item in obligations_by_scope(verdict, rule_id).items()
    }


def obligations_with_prefix(
    verdict: checker.Verdict, prefix: str
) -> set[tuple[str, str]]:
    """Return the (rule, scope) of every obligation whose rule starts with `prefix`."""
    return {key for key in obligations_of(verdict) if key[0].startswith(prefix)}


def judge_isolated(property_ids: tuple[str, ...], run: Any) -> checker.Verdict:
    """Return the verdict of only the named properties on an authored run.

    Effects are derived for those properties' rules alone (`contracts.make_contracts`), and only those
    properties are evaluated (the engine decides their inputs): no core gate,
    no other property's observations. `kind` and `reason` project their results.
    """
    spec = from_json(run.spec_text(), EDENFS_TARGET)
    facts = checker.HistoryFacts.build(
        spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
    )
    properties = [
        definition
        for definition in checker.registry(EDENFS_TARGET, "completed")
        if definition.id in property_ids
    ]
    assert len(properties) == len(property_ids), property_ids
    rules = [rule for definition in properties for rule in definition.rules]
    target = dataclasses.replace(
        EDENFS_TARGET, contracts=contracts.make_contracts(rules)
    )
    derived = checker.contract_effects(target, spec, facts, rules)
    assert not isinstance(derived, checker.PropertyError), derived
    found = checker.evaluate(
        properties,
        CheckInput(spec, facts, *derived),
        serving_paths=EDENFS_TARGET.serving_paths,
    )
    for result in found.results:
        assert result.error is None, result.error
    kind, reason, _cause = checker.project(found.results)
    return checker.Verdict(
        kind,
        reason,
        # The helpers here read only a report's `results` and `proofs`.
        report=cast(Any, SimpleNamespace(results=found.results, proofs=found.proofs)),
    )
