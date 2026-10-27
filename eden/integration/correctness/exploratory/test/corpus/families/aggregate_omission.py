# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `aggregate-omission`: status and Thrift status omitting a path
a checkout or write changed."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    R0,
    R1,
    R2,
    Run,
    SATISFIED,
    sl_status,
    thrift_status,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import omitted


def aggregate_runs() -> list[Run]:
    run = Run(
        "aggregate-omission",
        "status-omits-one-dirty-path",
        "r0 and r1 are written; a complete sl status lists r1 and omits r0.",
        basis=[
            "a complete status is authoritative for every path it omits",
            "known task T288573794 shape",
        ],
    )
    run.act("a", "w0", "write", path=R0, value_id="w0")
    run.act("a", "w1", "write", path=R1, value_id="w1")
    run.act("a", "s", "status", channel="sl")
    run.call("w0", "ok", **WRITE_OK)
    run.call("w1", "ok", **WRITE_OK)
    run.call("s", "ok", **sl_status({R1: "M"}, [R1]))
    run.finish()
    omitted(run, "s", R0, VIOLATED, "r0 holds w0's bytes during s", producers="write")

    run2 = Run(
        "aggregate-omission",
        "empty-status-clean-tree",
        "No mutation; a complete sl status is empty.",
        basis=["a complete status is authoritative for every path it omits"],
    )
    run2.act("a", "s", "status", channel="sl")
    run2.call("s", "ok", **sl_status({}))
    run2.finish()
    for path in (R0, R1, R2):
        omitted(run2, "s", path, SATISFIED, "every register equals rev3")

    run3 = Run(
        "aggregate-omission",
        "empty-thrift-status-after-write",
        "r0 is written; a complete Thrift status for rev3 is empty.",
        basis=["a complete status is authoritative for every path it omits"],
    )
    run3.act("a", "w", "write", path=R0, value_id="w")
    run3.act("a", "s", "status", channel="thrift")
    run3.call("w", "ok", **WRITE_OK)
    run3.call("s", "ok", **thrift_status({}))
    run3.finish()
    omitted(
        run3,
        "s",
        R0,
        VIOLATED,
        "an empty complete result still constrains r0",
        producers="write",
    )
    return [run, run2, run3]


def runs() -> list[Run]:
    return [
        *aggregate_runs(),
    ]
