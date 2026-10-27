# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The rule and reason vocabulary corpus expectations may name.

A rule's ID, subject kind, identity roles, and red controls stay stable:
known tasks' keys depend on them. `new_contract`, where present, states
the product behavior a rule checks and where it comes from. `corpus_test`
requires every registered rule to be catalogued with the code's identity,
subject kind, and red controls. Renaming a rule means updating this
catalog and every expectation that names it in the same diff. Statements
are not repeated here: `catalog` reads each rule's and reason's statement
from its declaration.

Identity values are computed from typed facts when the obligation is
declared, never from a solver result; `identity` says how, per role.
"""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.property import (
    CORE_NOT_EVALUATED_REASONS,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET

CATALOG_SCHEMA = "eden-correctness-corpus-rules-v2"

_STATUS_OBSERVED = (
    "the code the status lists for the path, or `absent` when it omits it"
)
_STATUS_IDENTITY = {
    "observed": _STATUS_OBSERVED,
    "producer-kinds": "the sorted kinds of the run's actions whose declared footprint "
    "writes the path, joined by `+` (`unlink+write`), or `none`; derived from the "
    "RunSpec alone, never from timing or outcomes",
}
_CHANNEL = {"channel": "the observing action's channel (`fuse`, `thrift`)"}


def _rule(
    subject_kind: str,
    roles: tuple[str, ...],
    stage: str,
    *,
    red_controls: tuple[str, ...] = (),
    identity: dict[str, str] | None = None,
    new_contract: str | None = None,
) -> dict[str, Any]:
    entry: dict[str, Any] = {
        "subject_kind": subject_kind,
        "identity_roles": list(roles),
        "identity": dict(identity or {}),
        "red_controls": sorted(red_controls),
        "stage": stage,
    }
    if new_contract is not None:
        entry["new_contract"] = new_contract
    return entry


RULES: dict[str, dict[str, Any]] = {
    # core -----------------------------------------------------------------
    "core.daemon-exit": _rule(
        "daemon",
        ("exit-class",),
        "direct",
        identity={"exit-class": "the recorded daemon exit class"},
    ),
    "core.activity": _rule(
        "checkout",
        (),
        "direct",
    ),
    "core.input-legality": _rule(
        "path",
        ("input-kind", "errno"),
        "direct+solver",
        identity={
            "input-kind": "the failed action's kind",
            "errno": "the recorded errno's symbolic name, or `none`",
        },
    ),
    "core.result-schema": _rule(
        "checkout",
        ("action-kind",),
        "direct",
        identity={"action-kind": "the action's kind"},
        new_contract="every recorded result decodes against its kind's strict "
        "`ResultSchema`; one recorder rule reports a result that does not",
    ),
    "result-shape.channel-fields": _rule(
        "checkout",
        ("action-kind",),
        "direct",
        identity={"action-kind": "the action's kind"},
        new_contract="a kind's result schema accepts every channel's fields, so "
        "strict decoding cannot reject a FUSE result carrying Thrift's fields",
    ),
    # bytes ----------------------------------------------------------------
    "bytes.stale-read": _rule(
        "path",
        ("channel",),
        "solver",
        identity=_CHANNEL,
    ),
    "bytes.torn-read": _rule(
        "path",
        ("channel",),
        "direct",
        red_controls=("torn-write",),
        identity=_CHANNEL,
    ),
    "bytes.fabricated-value": _rule(
        "path",
        ("channel",),
        "direct",
        identity=_CHANNEL,
    ),
    # status ---------------------------------------------------------------
    "status.model-live-stale-status": _rule(
        "path",
        ("observed", "producer-kinds"),
        "solver",
        identity=_STATUS_IDENTITY,
    ),
    "status.sl-model-stale-status": _rule(
        "path",
        ("observed", "producer-kinds"),
        "solver",
        identity=_STATUS_IDENTITY,
    ),
    "status.sl-fuse-stale-status": _rule(
        "path",
        ("observed", "producer-kinds"),
        "solver",
        red_controls=("common-status-loss",),
        identity=_STATUS_IDENTITY,
    ),
    "status.status-diff-mismatch": _rule(
        "path",
        ("observed", "producer-kinds"),
        "solver",
        red_controls=("diff-loss", "status-loss"),
        identity=_STATUS_IDENTITY,
    ),
    "status.thrift-model-stale-status": _rule(
        "path",
        ("observed", "producer-kinds"),
        "solver",
        identity=_STATUS_IDENTITY,
        new_contract="Thrift status is a serving path separate from sl status, with "
        "its own result shape; its probes are judged like sl status probes",
    ),
    # journal and cross-channel -------------------------------------------
    "journal.missing-acked-write": _rule(
        "path",
        (),
        "solver",
    ),
    "cross-channel.byte-disagreement": _rule(
        "path",
        (),
        "direct",
    ),
    # namespace, existence, manifest, listing, checkout -------------------
    "namespace.precondition": _rule(
        "path",
        ("input-kind",),
        "solver",
        identity={"input-kind": "the mutation's kind"},
        new_contract="an acked namespace mutation's preconditions held when it "
        "applied; the contracts states them as requirements",
    ),
    "existence.directory-read": _rule(
        "path",
        ("observed",),
        "solver",
        identity={"observed": "the read's outcome: `ok` or the errno name"},
    ),
    "existence.missing-after-create": _rule(
        "path",
        (),
        "solver",
    ),
    "existence.present-after-remove": _rule(
        "path",
        ("observation-kind",),
        "solver",
        identity={"observation-kind": "the observing action's kind"},
    ),
    "existence.mkdir-type": _rule(
        "path",
        ("fuse-type", "thrift-type"),
        "direct+solver",
        identity={
            "fuse-type": "getattr's fuse_type",
            "thrift-type": "getattr's thrift_type",
        },
    ),
    "existence.rename-atomicity": _rule(
        "checkout",
        ("state",),
        "solver",
        identity={"state": "`both-present` or `both-absent`"},
    ),
    "manifest.checkout-mismatch": _rule(
        "checkout",
        (),
        "solver",
    ),
    "listing.channel-disagreement": _rule(
        "path",
        (),
        "direct",
    ),
    "checkout.parent-before-paths": _rule(
        "checkout",
        ("via",),
        "solver",
        identity={"via": "the checkout's `via`"},
        new_contract="EdenFS moves the parent before it changes any path",
    ),
    "checkout.sl-abort": _rule(
        "checkout",
        ("outcome", "blockers"),
        "solver",
        identity={
            "outcome": "`succeeded` or `aborted`",
            "blockers": "the input class the engine found contradicted: `not-blocking` "
            "(a listed path does not block, whatever else is listed) or `unlisted` (a "
            "successful update's changed paths, a `--continue`'s interrupted-goto "
            "state)",
        },
        new_contract="Sapling's DRY_RUN conflict check (`edenfs_noconflict_checkout`, "
        "eden/scm/lib/checkout/src/edenfs.rs)",
    ),
    "checkout.conflict-count": _rule(
        "checkout",
        ("mode",),
        "direct+solver",
        identity={"mode": "the checkout's mode"},
        new_contract="EdenFS's conflict typing in both checkout modes",
    ),
    "checkout.in-progress-rejection": _rule(
        "checkout",
        ("via",),
        "solver",
        identity={"via": "the rejected checkout's `via`"},
        new_contract="EdenFS's checkout guard (`EdenMount::beginCheckout`)",
    ),
    "checkout.interrupted-refusal": _rule(
        "checkout",
        ("action-kind",),
        "direct",
        identity={"action-kind": "the kind of the action that was served"},
        new_contract="EdenFS's interrupted-checkout state",
    ),
    # attributes, fd, readdir, differential --------------------------------
    "attributes.type-disagreement": _rule(
        "path",
        ("fuse-type", "thrift-type"),
        "direct",
        identity={
            "fuse-type": "getattr's fuse_type",
            "thrift-type": "getattr's thrift_type",
        },
    ),
    "attributes.type-mismatch": _rule(
        "path",
        ("fuse-type",),
        "solver",
        identity={"fuse-type": "getattr's fuse_type"},
        new_contract="a getattr's FUSE type is the type of the path's visible "
        "binding; FUSE against Thrift is `attributes.type-disagreement`",
    ),
    "attributes.mode-revert": _rule(
        "path",
        (),
        "solver",
    ),
    "attributes.size-mismatch": _rule(
        "path",
        (),
        "solver",
    ),
    "attributes.mtime-decreased": _rule(
        "path",
        (),
        "direct",
    ),
    "fd.read-failed": _rule(
        "descriptor",
        ("errno",),
        "direct",
        identity={"errno": "the recorded errno's symbolic name"},
    ),
    "fd.stale-read": _rule(
        "descriptor",
        (),
        "solver",
    ),
    "fd.torn-read": _rule(
        "descriptor",
        (),
        "direct",
    ),
    "fd.fabricated-value": _rule(
        "descriptor",
        (),
        "direct",
    ),
    "readdir.duplicate-entry": _rule(
        "path",
        (),
        "direct",
    ),
    "readdir.unknown-entry": _rule(
        "path",
        (),
        "direct",
    ),
    "readdir.missing-entry": _rule(
        "path",
        (),
        "direct",
    ),
    "differential.outcome-disagreement": _rule(
        "path",
        ("action-kind", "primary-outcome", "reference-outcome"),
        "direct",
        identity={
            "action-kind": "the action's kind",
            "primary-outcome": "`ok` or the EdenFS side's errno name",
            "reference-outcome": "`ok` or the reference side's errno name",
        },
    ),
    "differential.value-disagreement": _rule(
        "path",
        ("action-kind",),
        "direct",
        identity={"action-kind": "the action's kind"},
    ),
    "differential.checkout-disagreement": _rule(
        "checkout",
        ("action-kind", "divergence"),
        "direct",
        identity={
            "action-kind": "the action's kind",
            "divergence": "`outcome` or `value`",
        },
    ),
}

NOT_EVALUATED_REASONS = (
    "core.input-legality.state-unobserved",
    "journal.incomplete-window",
    "journal.generation-unestablished",
    "checkout.conflict-count-unmodeled",
    "fd.unmodeled-object",
    "differential.primary-undecided",
    "readdir.membership-undecided",
    "core.solver.budget",
    "core.solver.explanation-budget",
    "core.solver.history-budget",
)

ATTRIBUTIONS = ("product", "undetermined", "oracle-suspect", "harness", "reference")


def catalog() -> dict[str, Any]:
    """Return the catalog with each statement read from its rule or reason declaration."""
    properties = checker.registry(EDENFS_TARGET)
    statements = {
        rule.id: rule.statement for p in properties for rule in p.rules.values()
    }
    reasons = dict(CORE_NOT_EVALUATED_REASONS)
    for definition in properties:
        reasons.update(definition.not_evaluated_reasons)
    return {
        "schema": CATALOG_SCHEMA,
        "rules": {
            rule_id: {**entry, "statement": statements[rule_id]}
            for rule_id, entry in RULES.items()
        },
        "not_evaluated_reasons": {
            reason: {"statement": reasons[reason]} for reason in NOT_EVALUATED_REASONS
        },
    }
