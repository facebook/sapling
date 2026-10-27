# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `result-shape` property: a decoded result has its channel's exact fields.

A kind's result schema accepts the union of every channel's fields
(`edenfs.actions.CHANNEL_SHAPES`); a FUSE result
carrying Thrift's fields, or the reverse, decodes yet says something no
backend records. Each decoded `ok`/`fail` result of a kind and outcome
with per-channel variants is judged directly; a mismatch is the
recorder's fault (`harness`), never a product task key.
"""

from __future__ import annotations

from collections.abc import Mapping

from eden.integration.correctness.exploratory.core.facts import FAIL, OK
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    checkout_subject,
    CollectorSink,
    PropertyDef,
    RuleDef,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    channel_shape_problem,
    CHANNEL_SHAPES,
)

RESULT_SHAPE = "result-shape"
CHANNEL_FIELDS = f"{RESULT_SHAPE}.channel-fields"
_KINDS = frozenset(kind for kind, _outcome in CHANNEL_SHAPES)


def collect_result_shape(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every decoded result whose kind and outcome have channel variants."""
    spec = check_input.spec
    for recorded in check_input.facts.actions.values():
        if recorded.result is None or recorded.outcome not in (OK, FAIL):
            continue
        outcome = str(recorded.outcome)
        if (recorded.kind, outcome) not in CHANNEL_SHAPES:
            continue
        ref = sink.obligation(
            CHANNEL_FIELDS,
            recorded.id,
            subject=checkout_subject(recorded.id),
            witnesses=(Witness("action-kind", recorded.kind),),
        )
        problem = channel_shape_problem(
            spec.action(recorded.id), outcome, dict(recorded.result.fields)
        )
        if problem is None:
            sink.satisfied(ref)
        else:
            sink.violated(
                ref,
                Attribution.HARNESS,
                f"result-shape/recorder: {recorded.id}: {problem}",
            )


RESULT_SHAPE_RULES: Mapping[str, RuleDef] = {
    CHANNEL_FIELDS: RuleDef(
        id=CHANNEL_FIELDS,
        statement="A decoded result has exactly one of its channel's recorded field "
        "sets (`edenfs.actions.CHANNEL_SHAPES`), not a mix across channels "
        "(harness attribution).",
        subject_kind=SubjectKind.CHECKOUT,
        identity_roles=("action-kind",),
    ),
}

RESULT_SHAPE_PROPERTY = PropertyDef(
    id=RESULT_SHAPE,
    collect=collect_result_shape,
    rules=RESULT_SHAPE_RULES,
)
