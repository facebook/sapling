# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `fd` property: what a read through a held descriptor may return.

A descriptor is the object its `open` resolved, not its path: a checkout,
rename, unlink, or `sl rm` that replaces the path leaves the descriptor
reading the old object, while a later `getattr` of the path sees whatever
now binds it. The contracts carries each held object's
content in its own cells (`contracts.checkout.descriptor_plans`): writes through a
path that binds the object update it, renames and `sl mv` move the
binding, and anything that replaces a bound path unbinds it. There is no
`write_fd` action, so no descriptor-bound write is modeled.

Per `read_fd` of a descriptor whose open was acked:

- `fd.read-failed`: a failure with ESTALE or EIO is a direct violation
  (`undetermined`: one serving path; no fault site targets `read_fd`);
  an `ok` read satisfies it.
- `fd.torn-read` / `fd.fabricated-value`: decided directly by the bytes
  property's classifier (`bytes.classify`), over the candidate values of
  every path the object can move through (`undetermined`): a value some
  producer of the run writes is the engine's question, never torn.
- `fd.stale-read`: otherwise one observation of the object's content cell
  expecting exactly the bytes read, inside the read's window; the engine
  decides it. A read a gap of the descriptor's plan can reach is
  NotEvaluated (`fd.unmodeled-object`): a write through a path the object
  can reach overlaps a checkout that changes that path, and the
  overlap can fall between the open's invocation and the read's completion.
"""

from __future__ import annotations

import errno
from collections.abc import Mapping

from eden.integration.correctness.exploratory.core.effects import (
    eq,
    Expect,
    ObligationRef,
    Observation,
    Value,
)
from eden.integration.correctness.exploratory.core.facts import ActionFacts, FAIL, OK
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    CoverageObligation,
    PropertyDef,
    RuleDef,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    descriptor_plans,
    DescriptorPlan,
)
from eden.integration.correctness.exploratory.edenfs.contracts.files import (
    DESCRIPTOR_RULE,
    object_cell,
)
from eden.integration.correctness.exploratory.edenfs.fixture import FILE
from eden.integration.correctness.exploratory.edenfs.properties.bytes import (
    CANDIDATE,
    Candidates,
    classify,
    FABRICATED,
    TORN,
)
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    eden_pinned,
)

FD = "fd"
READ_FAILED = f"{FD}.read-failed"
STALE_READ = DESCRIPTOR_RULE
TORN_READ = f"{FD}.torn-read"
FABRICATED_VALUE = f"{FD}.fabricated-value"
UNMODELED = f"{FD}.unmodeled-object"
READ_OBSERVED = "fd.read-observed"
READ_FD = "read_fd"
FAILURES = frozenset({errno.ESTALE, errno.EIO})
CONSUMES = frozenset(
    {(READ_FD, name) for name in ("bytes_prefix", "length", "sha256", "errno")}
)


def descriptor_subject(open_id: str) -> Subject:
    """Return the DESCRIPTOR subject of the descriptor one `open` holds."""
    return Subject(SubjectKind.DESCRIPTOR, f"descriptor:{open_id}")


def _judge_failure(
    recorded: ActionFacts, subject: Subject, sink: CollectorSink
) -> None:
    assert recorded.result is not None
    code = recorded.result.errno
    if code not in FAILURES:
        return
    name = errno.errorcode[code]
    ref = sink.obligation(
        READ_FAILED,
        recorded.id,
        subject=subject,
        witnesses=(Witness("errno", name),),
    )
    sink.violated(
        ref,
        Attribution.UNDETERMINED,
        f"bytes/fd: read-failed: {recorded.id} failed {name} through "
        f"{subject.token}; a replaced path never invalidates the opened object",
        (Witness("read-action", recorded.id),),
    )


def _candidates(candidates: Candidates, plan: DescriptorPlan) -> Mapping[str, bytes]:
    found: dict[str, bytes] = {}
    for path in sorted(plan.paths):
        found.update(candidates.of_path(path))
    return found


def _judge_bytes(
    recorded: ActionFacts,
    plan: DescriptorPlan,
    candidates: Candidates,
    subject: Subject,
    sink: CollectorSink,
) -> Observation | None:
    """Decide the direct byte rules; return the observation the engine must judge."""
    result = recorded.result
    assert result is not None
    witnesses = (Witness("read-action", recorded.id),)
    torn = sink.obligation(TORN_READ, recorded.id, subject=subject)
    fabricated = sink.obligation(FABRICATED_VALUE, recorded.id, subject=subject)
    length = result.get("length")
    shape = classify(result, _candidates(candidates, plan), candidates.anywhere)
    if shape.kind == TORN:
        described = (
            "that splice two values of its object"
            if shape.spliced
            else "of register width that no producer of the run writes"
        )
        sink.violated(
            torn,
            Attribution.UNDETERMINED,
            f"bytes/fd: torn-read: {recorded.id} returned {length} bytes through "
            f"{subject.token} {described}",
            (*witnesses, *(Witness("spliced", item) for item in shape.spliced)),
        )
        sink.satisfied(fabricated)
        return None
    sink.satisfied(torn)
    if shape.kind == FABRICATED:
        sink.violated(
            fabricated,
            Attribution.UNDETERMINED,
            f"bytes/fd: fabricated-value: {recorded.id} returned {length} bytes "
            f"through {subject.token} that no producer of the run can write",
            witnesses,
        )
        return None
    sink.satisfied(fabricated)
    stale = sink.obligation(STALE_READ, recorded.id, subject=subject)
    gap = plan.gap(recorded.window)
    if gap is not None:
        sink.not_evaluated(stale, UNMODELED, gap)
        return None
    return object_observation(recorded, plan, (stale,))


def object_observation(
    recorded: ActionFacts,
    plan: DescriptorPlan,
    obligations: tuple[ObligationRef, ...],
) -> Observation:
    """Return the observation that a `read_fd` saw its object hold the bytes it returned."""
    assert recorded.result is not None
    token = recorded.result.get("sha256")
    return Observation(
        id=f"o:{recorded.id}:fd",
        action=recorded.id,
        cells=(object_cell(plan.key, plan.path),),
        expect=Expect(((eq(Value(FILE, (token,))),),)),
        window=recorded.window,
        channel="fuse",
        obligations=obligations,
    )


def directly_legal(
    recorded: ActionFacts, plan: DescriptorPlan, candidates: Candidates
) -> bool:
    """Return whether a `read_fd`'s bytes pass the direct rules (neither torn nor fabricated)."""
    assert recorded.result is not None
    own = _candidates(candidates, plan)
    return classify(recorded.result, own, candidates.anywhere).kind == CANDIDATE


def collect_fd(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every completed `read_fd` through an acked open, in spec order."""
    spec = check_input.spec
    pinned = eden_pinned(spec, check_input.pinned)
    plans = descriptor_plans(spec, check_input.facts, pinned)
    candidates = Candidates(spec, pinned)
    for recorded in check_input.facts.actions_of(READ_FD):
        plan = plans.get(recorded.payload["fd"])
        if plan is None or recorded.result is None or recorded.completed is None:
            continue
        subject = descriptor_subject(plan.open_id)
        if recorded.outcome == FAIL:
            _judge_failure(recorded, subject, sink)
            continue
        if recorded.outcome != OK:
            continue
        sink.satisfied(sink.obligation(READ_FAILED, recorded.id, subject=subject))
        sink.covered(READ_OBSERVED)
        observation = _judge_bytes(recorded, plan, candidates, subject, sink)
        if observation is not None:
            sink.observe(observation)


FD_RULES: Mapping[str, RuleDef] = {
    rule.id: rule
    for rule in (
        RuleDef(
            id=READ_FAILED,
            statement="A read through a held descriptor never fails ESTALE or EIO; "
            "replacing the path does not invalidate the opened object.",
            subject_kind=SubjectKind.DESCRIPTOR,
            identity_roles=("errno",),
        ),
        RuleDef(
            id=STALE_READ,
            statement="A held descriptor's read equals its object's content at some "
            "point inside the read's window; the object is the one the open resolved, "
            "which renames move and replacements of its path do not change.",
            subject_kind=SubjectKind.DESCRIPTOR,
        ),
        RuleDef(
            id=TORN_READ,
            statement="A held descriptor returns a whole candidate value of the "
            "run, never a splice of two values its object can hold or other "
            "bytes of register width.",
            subject_kind=SubjectKind.DESCRIPTOR,
        ),
        RuleDef(
            id=FABRICATED_VALUE,
            statement="A held descriptor returns only bytes some producer of the run "
            "can write.",
            subject_kind=SubjectKind.DESCRIPTOR,
        ),
    )
}

FD_PROPERTY = PropertyDef(
    id=FD,
    collect=collect_fd,
    rules=FD_RULES,
    coverage=(CoverageObligation(READ_OBSERVED, "Held-descriptor reads judged"),),
    not_evaluated_reasons={
        UNMODELED: "A held-descriptor read a write overlapping a checkout "
        "of a path its object can reach may have changed",
    },
    consumes=CONSUMES,
)
