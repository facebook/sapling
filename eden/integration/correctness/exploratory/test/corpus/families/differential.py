# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `differential`: paired EdenFS and reference results."""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    R0,
    read_ok,
    Run,
    SATISFIED,
    sl_abort,
    sl_checkout,
    sl_status,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    ENOENT,
    inputs_ok,
    stale_read,
)


DIFFERENTIAL = "differential mode compares only the pairs the validator admits"


def _paired(
    name: str,
    description: str,
    *,
    primary_read: dict[str, Any],
    ref_read: dict[str, Any],
    primary_outcome: str = "ok",
    ref_outcome: str = "ok",
    ref_status: dict[str, str] | None = None,
) -> Run:
    run = Run("differential", name, description, basis=[DIFFERENTIAL], reference=True)
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "r", "read", path=R0, channel="fuse")
    run.act("a", "s", "status", channel="sl")
    run.paired("w", "ok", **WRITE_OK)
    run.paired(
        "r",
        primary_outcome,
        ref_outcome=ref_outcome,
        ref_facts=ref_read,
        **primary_read,
    )
    primary_status = sl_status({R0: "M"}, [R0])
    reference_status = (
        primary_status
        if ref_status is None
        else sl_status(ref_status, list(ref_status))
    )
    run.paired("s", "ok", ref_facts=reference_status, **primary_status)
    run.finish()
    if primary_outcome == "ok":
        inputs_ok(run, "r")
    if ref_status is None:
        run.expect(
            "differential.checkout-disagreement",
            "s",
            SATISFIED,
            "both sides report r0 M",
        )
    return run


def differential_runs() -> list[Run]:
    written = value("a", "w")
    run = _paired(
        "paired-results-agree",
        "A differential run: write, read, and sl status "
        "agree on both sides after normalization.",
        primary_read=read_ok(written),
        ref_read=read_ok(written),
    )
    run2 = _paired(
        "paired-read-disagrees",
        "The EdenFS side reads r0's committed bytes "
        "after the acked write while the reference side reads the write's.",
        primary_read=read_ok(committed(R0, TIP)),
        ref_read=read_ok(written),
    )
    stale_read(run2, "r", R0, VIOLATED, "the acked write superseded rev3's bytes")
    run2.expect(
        "differential.value-disagreement",
        "r",
        VIOLATED,
        "normalized read digests differ on independent serving paths",
        path=R0,
        identity={"action-kind": "read"},
    )
    run3 = _paired(
        "reference-side-failure",
        "The reference side's read fails ENOENT while "
        "the EdenFS side reads the write's bytes: a reference finding, never a "
        "product task key.",
        primary_read=read_ok(written),
        ref_read=errno_fail(ENOENT),
        ref_outcome="fail",
    )
    run3.expect(
        "differential.outcome-disagreement",
        "r",
        VIOLATED,
        "the reference side failed a read the EdenFS side served",
        path=R0,
        attribution="reference",
        identity={
            "action-kind": "read",
            "primary-outcome": "ok",
            "reference-outcome": "ENOENT",
        },
    )
    run4 = _paired(
        "primary-side-failure",
        "The EdenFS side's read fails ENOENT after the acked write while the "
        "reference side reads the write's bytes.",
        primary_read=errno_fail(ENOENT),
        ref_read=read_ok(written),
        primary_outcome="fail",
    )
    run4.expect(
        "differential.outcome-disagreement",
        "r",
        VIOLATED,
        "the EdenFS side failed a read the reference side served",
        path=R0,
        identity={
            "action-kind": "read",
            "primary-outcome": "ENOENT",
            "reference-outcome": "ok",
        },
    )
    run4.bad_input("r", "ENOENT", "r0 holds w's bytes at every point of the read")
    run5 = _paired(
        "paired-status-disagrees",
        "Both sides read the write's bytes, but the reference side's sl status is "
        "clean while the EdenFS side's lists r0 M.",
        primary_read=read_ok(written),
        ref_read=read_ok(written),
        ref_status={},
    )
    run5.expect(
        "differential.checkout-disagreement",
        "s",
        VIOLATED,
        "normalized status entries differ; Sapling serves both sides",
        identity={"action-kind": "status", "divergence": "value"},
        attribution="undetermined",
    )
    return [run, run2, run3, run4, run5]


REFERENCE_BASIS = "a disagreement the reference side's failure explains is `reference`"


def reference_failure_runs() -> list[Run]:
    """The reference side's sl goto fails; the status after it is not compared."""
    run = Run(
        "differential",
        "reference-update-failure",
        "sl goto succeeds on EdenFS and fails on the reference side: a reference "
        "finding; the sl status after it is not compared, since the two checkouts "
        "diverged.",
        basis=[REFERENCE_BASIS],
        reference=True,
    )
    run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
    run.act("a", "s", "status", channel="sl")
    run.paired(
        "c", "ok", ref_outcome="fail", ref_facts=sl_abort([R0]), **sl_checkout(2)
    )
    run.paired("s", "ok", ref_facts=sl_status({R0: "M"}, [R0]), **sl_status({}, []))
    run.finish()
    inputs_ok(run, "s")
    run.expect(
        "differential.checkout-disagreement",
        "c",
        VIOLATED,
        "the reference side failed an sl goto EdenFS served",
        attribution="reference",
        identity={"action-kind": "checkout", "divergence": "outcome"},
    )
    return [run]


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "differential",
        "agreeing-mutation-keeps-later-pairs-compared",
        "An agreeing mutation keeps later pairs compared.",
        basis=[
            "edenfs/properties/differential.py: differential.checkout-disagreement, differential.outcome-disagreement, differential.value-disagreement"
        ],
        reference=True,
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "r", "read", path=R0, channel="fuse")
    run.act("a", "s", "status", channel="sl")
    run.inv("w")
    run.tick(2)
    run.done(
        "w",
        "ok",
        reference_result={
            "schema_version": 1,
            "outcome": "ok",
            "started": 1.2,
            "completed": 1.3,
            "facts": {"bytes_written": 64},
        },
        bytes_written=64,
    )
    run.inv("r")
    run.tick(2)
    run.done(
        "r",
        "ok",
        reference_result={
            "schema_version": 1,
            "outcome": "ok",
            "started": 1.6,
            "completed": 1.7,
            "facts": {
                "bytes_prefix": "5245473120736565643d3030303430343034207772697465723d636f6d6d69743320616374696f6e3d616462626231346461666336313032622e2e2e2e2e2e2e",
                "length": 64,
                "sha256": "d74873eaa0dab90ec0cd324e50e0ef57d8583117df1f9eddea2f6a5cc608b116",
            },
        },
        bytes_prefix="5245473120736565643d3030303430343034207772697465723d6120616374696f6e3d772e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e",
        length=64,
        sha256="d430b702fb6c125ef89070ab25c6a997143237b4f3d7527b853cf9446496abc5",
    )
    run.inv("s")
    run.tick(2)
    run.done(
        "s",
        "ok",
        reference_result={
            "schema_version": 1,
            "outcome": "ok",
            "started": 2.0,
            "completed": 2.1,
            "facts": {
                "status": [],
                "diff_paths": [],
                "parent": "a3f6e06271c645843ea208d8bd3a4d6786ce3876",
            },
        },
        status=[{"path": R0, "code": "M"}],
        diff_paths=[R0],
        parent="a3f6e06271c645843ea208d8bd3a4d6786ce3876",
    )
    run.finish()
    run.expect(
        "differential.checkout-disagreement",
        "s",
        VIOLATED,
        "differential/eden≠reference: s status observed different compared fields",
        identity={"action-kind": "status", "divergence": "value"},
        attribution="undetermined",
    )
    run.expect(
        "differential.outcome-disagreement",
        "w",
        SATISFIED,
        "no outcome disagreement",
        path=R0,
    )
    run.expect(
        "differential.value-disagreement",
        "r",
        VIOLATED,
        "differential/eden≠reference: r read observed different bytes_prefix, length, sha256 on the two sides; the EdenFS side's value is legal",
        path=R0,
        identity={"action-kind": "read"},
        attribution="reference",
    )
    found.append(run)
    return found


def runs() -> list[Run]:
    return [
        *differential_runs(),
        *reference_failure_runs(),
        *collector_runs(),
    ]
