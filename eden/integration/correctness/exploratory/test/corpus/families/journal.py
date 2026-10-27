# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `journal`: acknowledged writes in journal windows."""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import (
    NOT_EVALUATED,
    R0,
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
)


def _journal(name: str, description: str, *, listed: bool) -> Run:
    run = Run(
        "journal",
        name,
        description,
        basis=["edenfs/properties/journal.py: MustAppearIn"],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "j", "journal_window", since_sequence=0)
    run.call("w", "ok", **WRITE_OK)
    changed = (
        [{"path": R0, "existed_before": True, "existed_after": True}] if listed else []
    )
    run.call(
        "j",
        "ok",
        mount_generation=1,
        sequence_number=4,
        since_sequence=0,
        changed_paths=changed,
        truncated=False,
        lost_changes=[],
    )
    run.finish()
    inputs_ok(run, "w")
    return run


def journal_runs() -> list[Run]:
    run = _journal(
        "acked-write-in-window",
        "An acked write to r0, then a complete journal window from sequence zero that "
        "lists r0.",
        listed=True,
    )
    run.expect(
        "journal.missing-acked-write", "j", SATISFIED, "the window lists r0", path=R0
    )
    run2 = _journal(
        "acked-write-missing-from-window",
        "An acked write to r0, then a complete journal window from sequence zero that "
        "lists nothing.",
        listed=False,
    )
    run2.expect(
        "journal.missing-acked-write",
        "j",
        VIOLATED,
        "w was acked before the complete window began",
        path=R0,
    )
    return [run, run2]


JOURNAL_BASIS = "edenfs/properties/journal.py: MustAppearIn"


JOURNAL_INCOMPLETE = "journal.incomplete-window"


JOURNAL_GENERATION = "journal.generation-unestablished"


def _window_ok(
    listed: bool, *, generation: int = 1, truncated: bool = False
) -> dict[str, Any]:
    return {
        "mount_generation": generation,
        "sequence_number": 4,
        "since_sequence": 0,
        "changed_paths": (
            [{"path": R0, "existed_before": True, "existed_after": True}]
            if listed
            else []
        ),
        "truncated": truncated,
        "lost_changes": ["JOURNAL_TRUNCATED"] if truncated else [],
    }


def _journal_run(name: str, description: str) -> Run:
    run = Run("journal", name, description, basis=[JOURNAL_BASIS])
    run.act("a", "w", "write", path=R0, value_id="w")
    return run


def journal_gap_runs() -> list[Run]:
    """Incomplete, overlapping, indeterminate, cumulative, and cross-generation windows."""
    runs = []
    run = _journal_run(
        "truncated-window-not-evaluated",
        "An acked write to r0, then a journal window from sequence zero that is "
        "truncated and omits r0: completeness cannot be judged.",
    )
    run.act("a", "j", "journal_window", since_sequence=0)
    run.call("w", "ok", **WRITE_OK)
    run.call("j", "ok", **_window_ok(False, truncated=True))
    run.finish()
    inputs_ok(run, "w")
    run.expect(
        "journal.missing-acked-write",
        "j",
        NOT_EVALUATED,
        "the only window after w's ack is truncated",
        path=R0,
        not_evaluated_reason=JOURNAL_INCOMPLETE,
    )
    runs.append(run)

    run = _journal_run(
        "write-overlaps-window",
        "A write to r0 is acked after a complete window began; the window omits r0, "
        "which is legal: the journal append need not precede the window. A later "
        "FUSE read returns the write's bytes.",
    )
    run.act("b", "j", "journal_window", since_sequence=0)
    run.act("b", "r", "read", path=R0, channel="fuse", requires=[("w", "finished")])
    run.inv("w", "j")
    run.done("j", "ok", **_window_ok(False))
    run.done("w", "ok", **WRITE_OK)
    run.call("r", "ok", **read_ok(value("a", "w")))
    run.finish()
    stale_read(run, "r", R0, SATISFIED, "w's bytes")
    runs.append(run)

    run = _journal_run(
        "indeterminate-write-omitted",
        "A write to r0 times out (info) before a complete window that omits r0, and "
        "a later read shows the write happened. An unacknowledged write's effects "
        "stay possible until target-closed, so no window began after them and the "
        "write owes the journal nothing. The ACKED "
        "filter itself (an indeterminate write whose window a daemon exit closes) "
        "is exercised by agreement_test.",
    )
    run.act("b", "j", "journal_window", since_sequence=0, requires=[("w", "finished")])
    run.act("b", "r", "read", path=R0, channel="fuse")
    run.inv("w")
    run.wait(120)
    run.done("w", "info", reason="operation-timeout")
    run.call("j", "ok", **_window_ok(False))
    run.call("r", "ok", **read_ok(value("a", "w")))
    run.finish()
    stale_read(run, "r", R0, SATISFIED, "w happened")
    runs.append(run)

    run = _journal_run(
        "later-window-drops-path",
        "An acked write to r0, then two complete windows from sequence zero: the "
        "first lists r0, the second omits it.",
    )
    run.act("a", "j1", "journal_window", since_sequence=0)
    run.act("a", "j2", "journal_window", since_sequence=0)
    run.call("w", "ok", **WRITE_OK)
    run.call("j1", "ok", **_window_ok(True))
    run.call("j2", "ok", **_window_ok(False))
    run.finish()
    run.expect(
        "journal.missing-acked-write",
        "j1",
        VIOLATED,
        "w was acked before j2 began, and j2 omits r0",
        path=R0,
    )
    runs.append(run)

    run = _journal_run(
        "generation-changed-before-window",
        "An acked write to r0, a journal cursor peek in generation 1 after the ack, "
        "then a complete generation-2 window that omits r0: the write ran before "
        "the peek, so it is outside generation 2 and the window owes it nothing.",
    )
    run.act("a", "k", "journal_window")
    run.act("a", "j", "journal_window", since_sequence=0)
    run.call("w", "ok", **WRITE_OK)
    run.call("k", "ok", mount_generation=1, sequence_number=4)
    run.call("j", "ok", **_window_ok(False, generation=2))
    run.finish()
    run.expect(
        "journal.missing-acked-write",
        "j",
        SATISFIED,
        "k recorded generation 1 after w's ack; j reads generation 2",
        path=R0,
    )
    runs.append(run)

    run = Run(
        "journal",
        "generation-changed-after-peek-unestablished",
        "A journal cursor peek in generation 1, then an acked write to r0, then a "
        "complete generation-2 window that omits r0: the change to generation 2 may "
        "fall after the write's ack, so its generation is unestablished.",
        basis=[JOURNAL_BASIS],
    )
    run.act("a", "k", "journal_window")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "j", "journal_window", since_sequence=0)
    run.call("k", "ok", mount_generation=1, sequence_number=4)
    run.call("w", "ok", **WRITE_OK)
    run.call("j", "ok", **_window_ok(False, generation=2))
    run.finish()
    inputs_ok(run, "w")
    run.expect(
        "journal.missing-acked-write",
        "j",
        NOT_EVALUATED,
        "a remount between w's ack and j is legal",
        path=R0,
        not_evaluated_reason=JOURNAL_GENERATION,
    )
    runs.append(run)

    run = Run(
        "journal",
        "generation-established-by-same-generation-peek",
        "Journal cursor peeks in generations 1 and 2, then an acked write to r0, "
        "then a complete generation-2 window that omits r0: the generation-2 peek "
        "finished before the write began, so the window must list r0.",
        basis=[JOURNAL_BASIS],
    )
    run.act("a", "k1", "journal_window")
    run.act("a", "k2", "journal_window")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "j", "journal_window", since_sequence=0)
    run.call("k1", "ok", mount_generation=1, sequence_number=4)
    run.call("k2", "ok", mount_generation=2, sequence_number=1)
    run.call("w", "ok", **WRITE_OK)
    run.call("j", "ok", **_window_ok(False, generation=2))
    run.finish()
    run.expect(
        "journal.missing-acked-write",
        "j",
        VIOLATED,
        "k2 recorded generation 2 before w began; j omits r0",
        path=R0,
    )
    runs.append(run)
    return runs


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "journal",
        "complete-window-decides-past-an-incomplete-one-1",
        "A complete window decides past an incomplete one (a truncated then a complete window).",
        basis=["edenfs/properties/journal.py: journal.missing-acked-write"],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "j1", "journal_window", since_sequence=0)
    run.act("a", "j2", "journal_window", since_sequence=0)
    run.call("w", "ok", **WRITE_OK)
    run.call(
        "j1",
        "ok",
        mount_generation=1,
        sequence_number=9,
        since_sequence=0,
        changed_paths=[],
        truncated=True,
        lost_changes=[],
    )
    run.call(
        "j2",
        "ok",
        mount_generation=1,
        sequence_number=9,
        since_sequence=0,
        changed_paths=[{"path": R0, "existed_before": True, "existed_after": True}],
        truncated=False,
        lost_changes=[],
    )
    run.finish()
    run.expect(
        "journal.missing-acked-write",
        "j1",
        SATISFIED,
        "no missing acked write",
        path=R0,
    )
    found.append(run)

    run = Run(
        "journal",
        "complete-window-decides-past-an-incomplete-one-2",
        "A complete window decides past an incomplete one (a truncated then a complete window).",
        basis=["edenfs/properties/journal.py: journal.missing-acked-write"],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "j1", "journal_window", since_sequence=0)
    run.act("a", "j2", "journal_window", since_sequence=0)
    run.call("w", "ok", **WRITE_OK)
    run.call(
        "j1",
        "ok",
        mount_generation=1,
        sequence_number=9,
        since_sequence=0,
        changed_paths=[],
        truncated=True,
        lost_changes=[],
    )
    run.call(
        "j2",
        "ok",
        mount_generation=1,
        sequence_number=9,
        since_sequence=0,
        changed_paths=[],
        truncated=False,
        lost_changes=[],
    )
    run.finish()
    run.expect(
        "journal.missing-acked-write",
        "j1",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={},
    )
    found.append(run)
    return found


def runs() -> list[Run]:
    return [
        *journal_runs(),
        *journal_gap_runs(),
        *collector_runs(),
    ]
