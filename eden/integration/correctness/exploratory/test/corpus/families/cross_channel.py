# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `cross-channel`: FUSE and Thrift reads of one path."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
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
    stale_read,
)


def cross_channel_runs() -> list[Run]:
    runs = []
    for agree in (True, False):
        run = Run(
            "cross-channel",
            "fuse-thrift-reads-agree" if agree else "fuse-thrift-reads-disagree",
            "An acked write to r0, then a FUSE read and a Thrift read with sync "
            "timeout 0 that "
            + ("both return its bytes." if agree else "return different bytes."),
            basis=[
                "`FuseChannel::co_waitForPendingWrites` returns at once "
                "(eden/fs/fuse/FuseChannel.h): on Linux the sync timeout has "
                "nothing to wait for"
            ],
        )
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "f", "read", path=R0, channel="fuse")
        run.act("a", "t", "read", path=R0, channel="thrift", sync_timeout_seconds=0)
        written = value("a", "w")
        run.call("w", "ok", **WRITE_OK)
        run.call("f", "ok", **read_ok(written))
        run.call("t", "ok", **read_ok(written if agree else committed(R0, TIP)))
        run.finish()
        if agree:
            stale_read(run, "t", R0, SATISFIED, "w's bytes")
        else:
            run.expect(
                "cross-channel.byte-disagreement",
                "t",
                VIOLATED,
                "no producer of r0 runs between the two reads",
                path=R0,
            )
            stale_read(run, "t", R0, VIOLATED, "w superseded rev3's bytes before t")
        runs.append(run)
    return runs


CROSS_CHANNEL_BASIS = (
    "edenfs/properties/cross_channel.py: only reads no writer of the path can "
    "separate are compared, along a spanning tree of each comparable class"
)


def _cross_channel_run(name: str, description: str) -> Run:
    run = Run("cross-channel", name, description, basis=[CROSS_CHANNEL_BASIS])
    run.act("a", "w", "write", path=R0, value_id="w")
    return run


def cross_channel_gap_runs() -> list[Run]:
    """Writers that separate reads, and a spanning tree over three reads."""
    written = value("a", "w")
    initial = committed(R0, TIP)
    runs = []
    run = _cross_channel_run(
        "write-between-reads",
        "A FUSE read returns r0's initial bytes, an acked write follows, then a "
        "Thrift read returns the write's bytes: the write separates the reads, so "
        "they are not compared.",
    )
    run.act("b", "f", "read", path=R0, channel="fuse")
    run.act("b", "t", "read", path=R0, channel="thrift", sync_timeout_seconds=0)
    run.call("f", "ok", **read_ok(initial))
    run.call("w", "ok", **WRITE_OK)
    run.call("t", "ok", **read_ok(written))
    run.finish()
    stale_read(run, "t", R0, SATISFIED, "w's bytes")
    runs.append(run)

    run = _cross_channel_run(
        "write-overlaps-reads",
        "A write to r0 is in flight across a FUSE read of the initial bytes and a "
        "Thrift read of the write's bytes: the reads are not comparable.",
    )
    run.act("b", "f", "read", path=R0, channel="fuse", requires=[("w", "started")])
    run.act("b", "t", "read", path=R0, channel="thrift", sync_timeout_seconds=0)
    run.inv("w")
    run.call("f", "ok", **read_ok(initial))
    run.call("t", "ok", **read_ok(written))
    run.done("w", "ok", **WRITE_OK)
    run.finish()
    stale_read(run, "t", R0, SATISFIED, "w may apply before t")
    runs.append(run)

    run = _cross_channel_run(
        "three-reads-fuse-disagrees",
        "An acked write to r0, then a FUSE read and a Thrift read of its bytes and a "
        "second FUSE read of rev3's bytes: the tree edge t-f2 disagrees.",
    )
    run.act("a", "f1", "read", path=R0, channel="fuse")
    run.act("a", "t", "read", path=R0, channel="thrift", sync_timeout_seconds=0)
    run.act("a", "f2", "read", path=R0, channel="fuse")
    run.call("w", "ok", **WRITE_OK)
    run.call("f1", "ok", **read_ok(written))
    run.call("t", "ok", **read_ok(written))
    run.call("f2", "ok", **read_ok(initial))
    run.finish()
    stale_read(run, "f2", R0, VIOLATED, "w superseded rev3's bytes before f2")
    run.expect(
        "cross-channel.byte-disagreement",
        "f2",
        VIOLATED,
        "no producer of r0 runs between t and f2",
        path=R0,
    )
    runs.append(run)
    return runs


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "cross-channel",
        "comparability-ignores-other-paths-and-the-reference-side",
        "Comparability ignores other paths and the reference side.",
        basis=["edenfs/properties/cross_channel.py: cross-channel.byte-disagreement"],
    )
    run.act("a", "f", "read", path=R0, channel="fuse")
    run.act("a", "w", "write", path=R1, value_id="w")
    run.act("a", "t", "read", path=R0, channel="thrift", sync_timeout_seconds=0)
    run.call("f", "ok", **read_ok(committed(R0, 3)))
    run.call("w", "ok", **WRITE_OK)
    run.call("t", "ok", **read_ok(committed(R0, 3)))
    run.finish()
    run.expect(
        "cross-channel.byte-disagreement",
        "t",
        SATISFIED,
        "no byte disagreement",
        path=R0,
    )
    found.append(run)
    return found


def runs() -> list[Run]:
    return [
        *cross_channel_runs(),
        *cross_channel_gap_runs(),
        *collector_runs(),
    ]
