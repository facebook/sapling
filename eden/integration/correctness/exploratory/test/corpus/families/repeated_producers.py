# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `repeated-producers`: one path produced many times (writes,
checkouts, revisits)."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    R0,
    read_ok,
    Run,
    SATISFIED,
    thrift_checkout,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    stale_read,
)


def _revisit(name: str, final: int, description: str) -> Run:
    run = Run(
        "repeated-producers",
        name,
        description,
        basis=[
            "a read of committed bytes may match any producer of those bytes",
        ],
    )
    run.act("a", "c1", "checkout", revision=1, via="thrift", mode="normal")
    run.act("a", "r1", "read", path=R0, channel="fuse")
    run.act("a", "c2", "checkout", revision=TIP, via="thrift", mode="normal")
    run.act("a", "r2", "read", path=R0, channel="fuse")
    run.call("c1", "ok", **thrift_checkout(1, conflicts=0))
    run.call("r1", "ok", **read_ok(committed(R0, 1)))
    run.call("c2", "ok", **thrift_checkout(TIP, conflicts=0))
    run.call("r2", "ok", **read_ok(committed(R0, final)))
    run.finish()
    return run


def committed_bytes_initial_and_revisited() -> Run:
    run = _revisit(
        "committed-bytes-initial-and-revisited",
        TIP,
        "rev3's r0 bytes have two producers, the initial state and the checkout back "
        "to rev3; the final read matches both, a finite OneOf, not NotEvaluated.",
    )
    stale_read(
        run,
        "r2",
        R0,
        SATISFIED,
        "c2 (or the initial state, before c1) produced rev3's bytes",
    )
    return run


def revisited_stale_read() -> Run:
    run = _revisit(
        "committed-bytes-revisited-stale-read",
        1,
        "After the acked checkout back to rev3, a read returns rev1's r0 bytes.",
    )
    stale_read(run, "r2", R0, VIOLATED, "c2 superseded c1's bytes before r2 started")
    return run


def _eleven_writes(name: str, sequential: bool, observed: int, description: str) -> Run:
    run = Run(
        "repeated-producers",
        name,
        description,
        basis=[
            "repeated producers of one value are judged exactly",
        ],
    )
    writers = [f"w{index:02d}" for index in range(11)]
    for index, action in enumerate(writers):
        run.act(f"p{index:02d}", action, "write", path=R0, value_id=action)
    run.act(
        "o",
        "r",
        "read",
        path=R0,
        channel="fuse",
        requires=[(action, "finished") for action in writers],
    )
    if sequential:
        for action in writers:
            run.call(action, "ok", **WRITE_OK)
    else:
        run.inv(*writers)
        for action in writers:
            run.done(action, "ok", **WRITE_OK)
    run.call("r", "ok", **read_ok(value(f"p{observed:02d}", writers[observed])))
    run.finish()
    return run


def eleven_overlapping_writes() -> Run:
    run = _eleven_writes(
        "eleven-overlapping-writes",
        False,
        7,
        "Eleven acked writes to r0 overlap; the read returns the eighth one's bytes. "
        "Exact search covers more than 10 operations per path.",
    )
    stale_read(
        run, "r", R0, SATISFIED, "w07 may be the last of the eleven overlapping writes"
    )
    return run


def eleven_sequential_writes_stale() -> Run:
    run = _eleven_writes(
        "eleven-sequential-writes-stale-read",
        True,
        3,
        "Eleven acked writes to r0 run one after another; the read returns the fourth.",
    )
    stale_read(run, "r", R0, VIOLATED, "w04..w10 supersede w03 before the read starts")
    return run


def _checkout_cycle(name: str, final_bytes: int, description: str) -> Run:
    run = Run(
        "repeated-producers",
        name,
        description,
        basis=["repeated producers of one value are judged exactly"],
    )
    revisions = [1 if index % 2 == 0 else TIP for index in range(11)]
    for index, revision in enumerate(revisions):
        run.act(
            "a",
            f"c{index:02d}",
            "checkout",
            revision=revision,
            via="thrift",
            mode="normal",
        )
    run.act("a", "r", "read", path=R0, channel="fuse")
    for index, revision in enumerate(revisions):
        run.call(f"c{index:02d}", "ok", **thrift_checkout(revision, conflicts=0))
    run.call("r", "ok", **read_ok(committed(R0, final_bytes)))
    run.finish()
    return run


def eleven_checkouts_revisited() -> Run:
    run = _checkout_cycle(
        "eleven-checkouts-final-revision",
        1,
        "Eleven checkouts alternate rev1/rev3 (six produce rev1's r0 bytes); the last is "
        "rev1 and the read returns rev1's bytes.",
    )
    stale_read(run, "r", R0, SATISFIED, "the last checkout produced rev1's bytes")
    return run


def eleven_checkouts_stale() -> Run:
    run = _checkout_cycle(
        "eleven-checkouts-stale-revision",
        TIP,
        "Eleven checkouts alternate rev1/rev3, ending at rev1; the read returns rev3's "
        "bytes, which five earlier checkouts and the initial state produced.",
    )
    stale_read(
        run, "r", R0, VIOLATED, "c10 (rev1) superseded every rev3 producer before r"
    )
    return run


def runs() -> list[Run]:
    return [
        committed_bytes_initial_and_revisited(),
        revisited_stale_read(),
        eleven_overlapping_writes(),
        eleven_sequential_writes_stale(),
        eleven_checkouts_revisited(),
        eleven_checkouts_stale(),
    ]
