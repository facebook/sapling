# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Helpers more than one corpus family module uses."""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import (
    CHURN,
    Run,
    VIOLATED,
)


def sl_abort_rule(
    run: Run,
    anchor: str,
    outcome: str,
    reason: str,
    observed: str,
    blockers: tuple[str, ...] = ("unlisted",),
) -> None:
    """Expect `checkout.sl-abort` at `anchor`; a violation is one error per `blockers`."""
    identities = [{"outcome": observed, "blockers": b} for b in blockers]
    if outcome != VIOLATED:
        identities = [{}]
    run.expect(
        "checkout.sl-abort",
        anchor,
        outcome,
        reason,
        identity=identities[0],
        more_identities=tuple(identities[1:]),
    )


ENOENT = 2


THRIFT_NORMAL_TABLE = (
    "EdenFS Thrift checkout: which local states NORMAL applies and which it reports "
    "as conflicts (`contracts.model._applies`, `contracts.checkout._conflict_states`)"
)


SL_ABORT_OR_SUCCESS = (
    "sl goto either aborts with no effect or applies every changed path "
    "(`contracts.checkout._sl_passes`)"
)


CHECKOUT_GUARD = (
    "EdenFS refuses a checkout while another holds the checkout guard "
    "(`EdenMount::beginCheckout`)"
)


INTERRUPTED_CHECKOUT = (
    "an interrupted checkout may have applied any subset of its paths"
)


MODE_DIRT = "a changed executable bit alone makes a path dirty for checkout"


STALE_READ = "bytes.stale-read"


MODEL_LIVE = "status.model-live-stale-status"


SL_FUSE = "status.sl-fuse-stale-status"


STATUS_DIFF = "status.status-diff-mismatch"


THRIFT_MODEL = "status.thrift-model-stale-status"


UNDETERMINED = "undetermined"


def stale_read(
    run: Run, anchor: str, path: str, outcome: str, reason: str, **kw: Any
) -> None:
    channel = run.actions[anchor].get("channel", "fuse")
    identity = {"channel": channel} if outcome == VIOLATED else {}
    run.expect(STALE_READ, anchor, outcome, reason, path=path, identity=identity, **kw)


def status_rule(
    run: Run,
    rule: str,
    anchor: str,
    path: str,
    outcome: str,
    reason: str,
    *,
    observed: str = "",
    producers: str = "",
    attribution: str | None = None,
) -> None:
    """Declare one status obligation; a violation names its identity.

    The identity is what the status reported (`observed`) and the sorted
    kinds of the run's actions with an effect on the path, joined by `+`
    (`producers`, `none` for no such action). sl status and its `sl diff`
    jointly support the obligations of `status.model-live-stale-status`,
    `status.sl-fuse-stale-status`, and `status.status-diff-mismatch`: two
    outputs of one Sapling serving path, so a violation of any is
    `undetermined`.
    """
    identity = {}
    if outcome == VIOLATED:
        assert observed and producers, (run.name, anchor, path)
        identity = {"observed": observed, "producer-kinds": producers}
        if attribution is None and rule in (MODEL_LIVE, SL_FUSE, STATUS_DIFF):
            attribution = UNDETERMINED
    run.expect(
        rule,
        anchor,
        outcome,
        reason,
        path=path,
        identity=identity,
        attribution=attribution,
    )


def _status_default(run: Run, anchor: str, observed: str) -> str:
    """The rule of a status whose sl diff agrees: an omission or an `M` entry."""
    if run.actions[anchor]["channel"] != "sl":
        return THRIFT_MODEL
    return SL_FUSE if observed == "absent" else MODEL_LIVE


def omitted(
    run: Run,
    anchor: str,
    path: str,
    outcome: str,
    reason: str,
    *,
    rule: str | None = None,
    producers: str = "",
) -> None:
    """Declare the obligation of a path a complete status omits."""
    status_rule(
        run,
        rule or _status_default(run, anchor, "absent"),
        anchor,
        path,
        outcome,
        reason,
        observed="absent",
        producers=producers,
    )


def code(
    run: Run,
    anchor: str,
    path: str,
    observed: str,
    outcome: str,
    reason: str,
    *,
    rule: str | None = None,
    producers: str = "",
) -> None:
    """Declare the obligation of a path a status lists with `observed`."""
    status_rule(
        run,
        rule or _status_default(run, anchor, observed),
        anchor,
        path,
        outcome,
        reason,
        observed=observed,
        producers=producers,
    )


def inputs_ok(run: Run, *anchors: str) -> None:
    for anchor in anchors:
        run.legal_input(anchor)


EISDIR = 21


FUSE_ATTRS = {"mtime": 1.0, "mtime_history": 1.0, "size": 64}


def getattr_ok(mode: int, thrift_type: str = "file") -> dict[str, Any]:
    return {**FUSE_ATTRS, "mode": mode, "fuse_type": "file", "thrift_type": thrift_type}


EIO = 5


def attrs(fuse_type: str = "file", **thrift: str) -> dict[str, Any]:
    fields = {**FUSE_ATTRS, "mode": 0o644, "fuse_type": fuse_type}
    return {**fields, **(thrift or {"thrift_type": fuse_type})}


def getattr_fields(
    size: int = 64, mtime_ns: int | None = None, **types: str
) -> dict[str, Any]:
    fields: dict[str, Any] = {
        "mtime": 1.0,
        "mtime_history": 1.0,
        "size": size,
        "mode": 0o644,
        "fuse_type": "file",
        "thrift_type": "file",
        **types,
    }
    if mtime_ns is not None:
        fields["mtime_ns"] = mtime_ns
    return fields


SL_RM_FORCE = (
    "`sl rm --force` unlinks and marks removed each tracked matched file, forgets "
    "an added one, and exits 1 when no matched path is tracked"
)


SL_GOTO_CONTRACT = (
    "`sl goto` without --clean or --merge aborts on any local change of a path the "
    "destination changes, removals included: a path absent while the destination "
    "removes it is MISSING_REMOVED, which `actionmap_from_eden_conflicts` "
    "(eden/scm/lib/checkout/src/edenfs.rs) reports as deleted and "
    "`CheckoutPlan::check_conflicts` (lib.rs) lists; the abort lists only the "
    "changed path"
)


def process_fail(code: int, stderr: str = "x\n") -> dict[str, object]:
    return {
        "reason": f"sl: {stderr.strip()}",
        "returncode": code,
        "stdout": "",
        "stderr": stderr,
    }


# The relevant paths of a status judgment that are not registers: two
# generated files and the churn files, in path order.
OTHER_RELEVANT_PATHS = (
    "tree/a0/r1/00000.dat",
    "tree/a1/r2/00001.dat",
    CHURN,
    "tree/churn/c1",
    "tree/churn/c2",
    "tree/churn/c3",
)
