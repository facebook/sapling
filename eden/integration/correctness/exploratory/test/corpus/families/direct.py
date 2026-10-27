# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `direct`: rules decided without the solver (fabricated and
torn bytes, result shapes)."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    R0,
    R1,
    read_ok,
    Run,
    SATISFIED,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    attrs,
    EIO,
    inputs_ok,
    UNDETERMINED,
)


def _single_read(
    family: str,
    name: str,
    description: str,
    data: bytes | None,
    *,
    write: bool = False,
    failure: int | None = None,
    red_control: str | None = None,
) -> Run:
    run = Run(
        family,
        name,
        description,
        basis=["a direct check decides facts no legal state allows"],
        red_control=red_control,
    )
    if write:
        run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "r", "read", path=R0, channel="fuse")
    if write:
        run.call("w", "ok", **WRITE_OK)
    if failure is not None:
        run.call("r", "fail", **errno_fail(failure))
    else:
        assert data is not None
        run.call("r", "ok", **read_ok(data))
    run.finish()
    return run


def direct_runs() -> list[Run]:
    run = _single_read(
        "direct",
        "committed-read",
        "A read of r0 returns rev3's committed bytes.",
        committed(R0, TIP),
    )
    for rule, reason in (
        ("bytes.torn-read", "the bytes are one whole candidate value"),
        ("bytes.fabricated-value", "the bytes are a committed value"),
    ):
        run.expect(rule, "r", SATISFIED, reason, path=R0)
    run2 = _single_read(
        "direct",
        "fabricated-bytes",
        "A read of r0 returns 40 bytes, not register width, that no action wrote.",
        value("ghost", "x")[:40],
    )
    run2.expect(
        "bytes.fabricated-value",
        "r",
        VIOLATED,
        "no effect of the run or the initial state yields these bytes",
        path=R0,
        identity={"channel": "fuse"},
        attribution=UNDETERMINED,
    )
    torn = value("a", "w")[:32] + committed(R0, TIP)[32:]
    run3 = _single_read(
        "direct",
        "torn-read",
        "After an acked write a read returns the "
        "write's first 32 bytes followed by rev3's last 32 (the shape the "
        "torn-write red control produces).",
        torn,
        write=True,
        red_control="torn-write",
    )
    run3.expect(
        "bytes.torn-read",
        "r",
        VIOLATED,
        "the bytes splice two values and equal neither",
        path=R0,
        identity={"channel": "fuse"},
        attribution=UNDETERMINED,
    )
    junk = _single_read(
        "direct",
        "register-width-junk",
        "A read of r0 returns well-formed register bytes no action wrote: 64 "
        "non-producible bytes (`!`*64).",
        value("ghost", "x"),
    )
    partial = _single_read(
        "direct",
        "zero-filled-partial-write",
        "After an acked write a read returns the write's first 32 bytes followed by "
        "32 zero bytes: a partial write, no splice of two candidates.",
        value("a", "w")[:32] + bytes(32),
        write=True,
    )
    for torn_run, reason in (
        (junk, "64 bytes no producer of r0 writes"),
        (partial, "64 bytes that are neither candidate nor a splice of two"),
    ):
        torn_run.expect(
            "bytes.torn-read",
            "r",
            VIOLATED,
            reason,
            path=R0,
            identity={"channel": "fuse"},
            attribution=UNDETERMINED,
        )
    run4 = _single_read(
        "direct",
        "forbidden-errno",
        "A FUSE read of r0 fails EIO with no nemesis.",
        None,
        failure=EIO,
    )
    run4.bad_input("r", "EIO", "EIO is a fault errno and no nemesis caused it")
    return [run, run2, run3, junk, partial, run4]


HARNESS = "harness"


def result_shape_runs() -> list[Run]:
    runs = []
    for mixed in (False, True):
        run = Run(
            "direct",
            "getattr-mixed-thrift-fields" if mixed else "getattr-one-thrift-field",
            "A getattr records "
            + ("both a Thrift type and a Thrift error." if mixed else "a Thrift type."),
            basis=[
                "CHANNEL_SHAPES[('getattr', 'ok')] (edenfs/actions.py): one "
                "getAttributesFromFilesV2 entry is a FileAttributeDataOrErrorV2 "
                "or SourceControlTypeOrError (eden/fs/service/eden.thrift), "
                "recorded as thrift_type or thrift_error"
            ],
        )
        run.act("a", "g", "getattr", path=R0)
        extra = {"thrift_type": "file"}
        if mixed:
            extra["thrift_error"] = "rpc: timed out"
        run.call("g", "ok", **attrs(**extra))
        run.finish()
        inputs_ok(run, "g")
        if mixed:
            run.expect(
                "result-shape.channel-fields",
                "g",
                VIOLATED,
                "no backend records both fields",
                attribution=HARNESS,
                identity={"action-kind": "getattr"},
            )
        else:
            run.expect(
                "result-shape.channel-fields", "g", SATISFIED, "one FUSE variant"
            )
        runs.append(run)
    return runs


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "direct",
        "another-paths-value-is-stale-never-torn",
        "Bytes some producer of the run writes are the engine's question, whatever their width.",
        basis=[
            "edenfs/properties/bytes.py: bytes.fabricated-value, bytes.stale-read, bytes.torn-read"
        ],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "r", "read", requires=[("w", "finished")], path=R1, channel="fuse")
    run.call("w", "ok", **WRITE_OK)
    run.call("r", "ok", **read_ok(value("a", "w")))
    run.finish()
    run.expect(
        "bytes.stale-read",
        "r",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R1,
        identity={"channel": "fuse"},
    )
    found.append(run)

    run = Run(
        "direct",
        "junk-of-another-width-is-fabricated",
        "Bytes no producer anywhere writes, not of register width, are fabricated.",
        basis=["edenfs/properties/bytes.py: bytes.fabricated-value, bytes.torn-read"],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "r", "read", requires=[("w", "finished")], path=R1, channel="fuse")
    run.call("w", "ok", **WRITE_OK)
    run.call(
        "r",
        "ok",
        bytes_prefix="5245473120736565643d3030303430343034207772697465723d6120616374696f6e3d772e2e2e2e",
        length=40,
        sha256="d4539d66ef8f47fd08bc02a27f98af7988716995af14492e9351459af4260588",
    )
    run.finish()
    run.expect(
        "bytes.fabricated-value",
        "r",
        VIOLATED,
        "bytes/fuse: fabricated-value: r returned 40 bytes on tree/registers/r1.reg that no producer of the run can write",
        path=R1,
        identity={"channel": "fuse"},
        attribution="undetermined",
    )
    found.append(run)
    return found


def runs() -> list[Run]:
    return [
        *direct_runs(),
        *result_shape_runs(),
        *collector_runs(),
    ]
