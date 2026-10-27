# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `not-evaluated`: histories the engine leaves undecided, with
the finding it still reports."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    NOT_EVALUATED,
    R0,
    R1,
    read_ok,
    Run,
    SATISFIED,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    inputs_ok,
    stale_read,
    UNDETERMINED,
)


BUDGET_BASIS = [
    "a proven violation survives unrelated NotEvaluated in the same and in a "
    "different factor component; every planted contradiction is Violated",
    "budget exhaustion is NotEvaluated for that component's undecided obligations only",
]


UNDECIDED = (
    "budget-dependent: the engine may decide this satisfiable read or run out of "
    "decision budget, but never violates it"
)


def _budget(name: str, description: str, *, violation: str | None) -> Run:
    """24 overlapping acked writes to r0 and 12 reads inside them, each satisfiable.

    `violation` adds a read of rev1's committed bytes, which nothing in the run
    produces, of r0 (same factor component) or r1 (another component); None
    instead plants a contradiction between two reads after every write.
    """
    run = Run("not-evaluated", name, description, basis=BUDGET_BASIS)
    writers = [f"w{index:02d}" for index in range(24)]
    for index, action in enumerate(writers):
        run.act(f"p{index:02d}", action, "write", path=R0, value_id=action)
    holders = [] if violation is None else [f"h{index:02d}" for index in range(12)]
    if violation is None:
        after = [(action, "finished") for action in writers]
        run.act("o", "ra", "read", path=R0, channel="fuse", requires=after)
        run.act("o", "rb", "read", path=R0, channel="fuse")
    else:
        for action in holders:
            run.act("o", action, "read", path=R0, channel="fuse")
        run.act("q", "rx", "read", path=violation, channel="fuse")
    run.inv(*writers)
    if violation is not None:
        for index, action in enumerate(holders):
            source = 2 * index
            run.call(action, "ok", **read_ok(value(f"p{source:02d}", writers[source])))
            inputs_ok(run, action)
            stale_read(run, action, R0, SATISFIED, UNDECIDED)
            run.obligations[-1].outcome_in = (SATISFIED, NOT_EVALUATED)
        run.call("rx", "ok", **read_ok(committed(violation, 1)))
        inputs_ok(run, "rx")
        stale_read(
            run,
            "rx",
            violation,
            VIOLATED,
            "rev1's bytes are a committed value no action of the run produces: zero "
            "candidate producers, decided in propagation",
        )
    for action in writers:
        run.done(action, "ok", **WRITE_OK)
    if violation is None:
        run.call("ra", "ok", **read_ok(value("p05", "w05")))
        run.call("rb", "ok", **read_ok(value("p09", "w09")))
        for anchor in ("ra", "rb"):
            stale_read(
                run,
                anchor,
                R0,
                VIOLATED,
                "no write can fall between ra and rb, so they must read one value; "
                "a planted contradiction is Violated, never NotEvaluated",
            )
    run.finish()
    inputs_ok(run, *writers)
    return run


def _unported(name: str, description: str, *, readdir: bool) -> Run:
    run = Run(
        "not-evaluated",
        name,
        description,
        basis=[
            "a STATE errno failure without a contract observation is "
            "NotEvaluated(core.input-legality.state-unobserved)",
            "the contracts decides what an sl exit status or an errno-less Thrift "
            "failure means",
        ],
    )
    if readdir:
        run.act("a", "d", "readdir", path="tree/registers", channel="thrift")
        run.call("d", "fail", reason="thrift: listing failed", source="thrift")
    else:
        run.act("a", "m", "sl_rm", path=R0)
        run.call(
            "m",
            "fail",
            reason="sl_rm: abort: failed",
            returncode=1,
            stdout="",
            stderr="abort: failed\n",
        )
    run.act("a", "rq", "read", path=R1, channel="fuse")
    # Not register width: register-width junk would be `bytes.torn-read`.
    run.call("rq", "ok", **read_ok(value("ghost", "x")[:40]))
    run.finish()
    if readdir:
        run.expect(
            "core.input-legality",
            "d",
            NOT_EVALUATED,
            "a Thrift readdir failure without a recorded type has no derived "
            "observation",
            subject=run.input_subject("d"),
            not_evaluated_reason="core.input-legality.state-unobserved",
        )
    run.expect(
        "bytes.fabricated-value",
        "rq",
        VIOLATED,
        "no effect of the run or the initial state yields rq's bytes",
        path=R1,
        identity={"channel": "fuse"},
        attribution=UNDETERMINED,
    )
    return run


def not_evaluated_runs() -> list[Run]:
    return [
        _budget(
            "budget-planted-contradiction",
            "24 overlapping writes to r0, then two reads after all of them that "
            "return different writes' bytes with nothing in between.",
            violation=None,
        ),
        _budget(
            "budget-violation-same-component",
            "24 overlapping writes to r0 with 12 satisfiable reads inside them, and a "
            "read of r0 returning rev1's bytes in the same factor component.",
            violation=R0,
        ),
        _budget(
            "budget-violation-other-component",
            "The same 24 writes and 12 reads, and a read of r1 returning rev1's bytes "
            "in another factor component.",
            violation=R1,
        ),
        _unported(
            "thrift-readdir-failure-unported",
            "A Thrift readdir fails without an "
            "errno; a fabricated read of r1 is still Violated.",
            readdir=True,
        ),
    ]


def runs() -> list[Run]:
    return [
        *not_evaluated_runs(),
    ]
