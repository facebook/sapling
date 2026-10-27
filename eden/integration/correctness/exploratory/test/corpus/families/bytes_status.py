# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `bytes-status`: reads and status (sl and Thrift) against the
visible tree."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    CHURN,
    committed,
    MOVED,
    NOT_EVALUATED,
    R0,
    R1,
    R2,
    read_ok,
    Run,
    SATISFIED,
    sl_status,
    thrift_checkout,
    thrift_status,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    code,
    MODE_DIRT,
    omitted,
    OTHER_RELEVANT_PATHS,
    SL_FUSE,
    stale_read,
    STATUS_DIFF,
    status_rule,
    THRIFT_NORMAL_TABLE,
    UNDETERMINED,
)


FORCE_WRITE_OVERLAP = "a FORCE checkout overlapping a write"


SL_MODEL = "status.sl-model-stale-status"


def diff_agrees(run: Run, anchor: str, path: str, reason: str) -> None:
    run.expect(STATUS_DIFF, anchor, SATISFIED, reason, path=path)


FORCE_WITH_WRITE = (
    "FORCE with a concurrent write reports the path's conflict optionally"
)


KNOWN = "test/corpus/known/{task}/run-spec.json (the reproducer of task {task})"


def _force_write(
    name: str, read_bytes: str, status: dict[str, str], description: str, conflicts: int
) -> Run:
    run = Run(
        "bytes-status",
        name,
        description,
        basis=[FORCE_WRITE_OVERLAP, THRIFT_NORMAL_TABLE + " (FORCE)", FORCE_WITH_WRITE],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="force")
    run.act("b", "w", "write", path=R0, value_id="w")
    run.act(
        "o",
        "r",
        "read",
        path=R0,
        channel="fuse",
        requires=[("c", "finished"), ("w", "finished")],
    )
    run.act("o", "s", "status", channel="sl")
    run.inv("c", "w")
    run.done("w", "ok", **WRITE_OK)
    run.done("c", "ok", **thrift_checkout(2, conflicts=conflicts))
    data = value("b", "w") if read_bytes == "write" else committed(R0, 2)
    run.call("r", "ok", **read_ok(data))
    run.call("s", "ok", **sl_status(status, status))
    run.finish()
    return run


def force_checkout_write_cycle() -> Run:
    run = _force_write(
        "force-checkout-write-cycle",
        "write",
        {},
        "A FORCE checkout to rev2 overlaps an acked write to r0. A later read returns "
        "the write's unique bytes, so the write follows every reset of r0 by the "
        "checkout; the later status omits r0, so some reset (its r0 visit, or the "
        "write landing in the replaced inode) follows the write. The per-path order "
        "is cyclic. The conflict count (0) is legal either way: the write lands after "
        "the visit, kept or lost.",
        0,
    )
    stale_read(
        run,
        "r",
        R0,
        VIOLATED,
        "the read needs w after every reset of r0; the status needs a reset after w; "
        "the core's status claim has two Sapling observers (status and diff)",
        attribution=UNDETERMINED,
    )
    omitted(
        run,
        "s",
        R0,
        VIOLATED,
        "a clean r0 needs the checkout to supersede the write; the read forbids it",
        producers="checkout+write",
    )
    return run


def force_checkout_write_read_and_dirty_status() -> Run:
    run = _force_write(
        "force-checkout-write-consistent-dirty",
        "write",
        {R0: "M"},
        "Legal twin of the 5.1 cycle: the read returns the write's bytes and status "
        "reports r0 modified, both explained by the write after the checkout's r0 effect.",
        0,
    )
    code(run, "s", R0, "M", SATISFIED, "w's bytes differ from rev2's r0")
    return run


def force_checkout_write_superseded() -> Run:
    run = _force_write(
        "force-checkout-write-superseded",
        "committed",
        {},
        "Legal twin of the 5.1 cycle: the read returns rev2's r0 bytes and status is "
        "clean, both explained by the checkout superseding the overlapping write "
        "(r0 was dirty at its visit, so FORCE reported one conflict).",
        1,
    )
    omitted(run, "s", R0, SATISFIED, "r0 equals rev2's committed bytes")
    return run


def force_checkout_write_lost() -> Run:
    run = _force_write(
        "force-checkout-write-lost",
        "committed",
        {},
        "Legal twin of the 5.1 cycle: FORCE reports no conflict, the read returns "
        "rev2's r0 bytes, and status is clean: the write's open resolved the inode "
        "FORCE replaced, so its bytes landed in the unlinked inode.",
        0,
    )
    omitted(run, "s", R0, SATISFIED, "r0 equals rev2's committed bytes")
    return run


def _write_after_observed_reset(kept: bool) -> Run:
    run = Run(
        "bytes-status",
        f"force-checkout-write-after-observed-reset-{'kept' if kept else 'lost'}",
        "A FORCE checkout to rev2 overlaps a write `a` to a dirty r0. Read r1 returns "
        "rev2's bytes, so the checkout's reset already replaced r0. Write `b` starts "
        "after r1, so it reaches the new inode: no reset of r0 can supersede it, and "
        "read r2 after everything must return b's bytes."
        + ("" if kept else " Here r2 returns rev2's bytes: b is lost."),
        basis=[
            FORCE_WRITE_OVERLAP,
            "an op invoked after a read that saw the reset follows the reset "
            "(`contracts.checkout._supersede_groups`, keyed per checkout and op rank)",
        ],
    )
    run.act("w0", "w0", "write", path=R0, value_id="z")
    run.act("w1", "a", "write", path=R0, value_id="a", requires=[("w0", "finished")])
    run.act(
        "c",
        "c",
        "checkout",
        revision=TIP,
        via="thrift",
        mode="force",
        requires=[("w0", "finished")],
    )
    run.act("o", "r1", "read", path=R0, channel="fuse", requires=[("a", "finished")])
    run.act("w2", "b", "write", path=R0, value_id="b", requires=[("r1", "finished")])
    run.act(
        "o",
        "r2",
        "read",
        path=R0,
        channel="fuse",
        requires=[("a", "finished"), ("b", "finished"), ("c", "finished")],
    )
    run.call("w0", "ok", **WRITE_OK)
    run.inv("a", "c")
    run.done("a", "ok", **WRITE_OK)
    run.call("r1", "ok", **read_ok(committed(R0, TIP)))
    run.call("b", "ok", **WRITE_OK)
    run.done("c", "ok", **thrift_checkout(TIP, conflicts=1))
    run.call("r2", "ok", **read_ok(value("w2", "b") if kept else committed(R0, TIP)))
    run.finish()
    if kept:
        stale_read(run, "r2", R0, SATISFIED, "b follows the reset")
    else:
        stale_read(run, "r2", R0, VIOLATED, "b follows the reset r1 observed")
        stale_read(
            run,
            "r1",
            R0,
            VIOLATED,
            "found-proof attribution: r1 is in the proof with r2 (its kept twin "
            "shows r1 alone is legal)",
        )
    return run


def _dirty_status(
    name: str,
    status: dict[str, str],
    diff: list[str],
    description: str,
    *,
    basis: list[str],
    read: bool = True,
    red_control: str | None = None,
) -> Run:
    run = Run("bytes-status", name, description, basis=basis, red_control=red_control)
    run.act("a", "w", "write", path=R0, value_id="w")
    if read:
        run.act("a", "r", "read", path=R0, channel="fuse")
    run.act("a", "s", "status", channel="sl")
    run.call("w", "ok", **WRITE_OK)
    if read:
        run.call("r", "ok", **read_ok(value("a", "w")))
    run.call("s", "ok", **sl_status(status, diff))
    run.finish()
    return run


def dirty_bytes_clean_status() -> Run:
    run = _dirty_status(
        "dirty-bytes-clean-status",
        {},
        [],
        "An acked write's bytes are read back, then a complete sl status and sl diff "
        "both omit r0 (the shape the common-status-loss red control produces).",
        basis=[KNOWN.format(task="T288573794")],
        red_control="common-status-loss",
    )
    omitted(
        run,
        "s",
        R0,
        VIOLATED,
        "r0 holds w's bytes, which differ from rev3's, at every point of s",
        producers="write",
    )
    return run


def dirty_bytes_dirty_status() -> Run:
    run = _dirty_status(
        "dirty-bytes-dirty-status",
        {R0: "M"},
        [R0],
        "Legal twin: the acked write's bytes are read back and sl status reports r0 M.",
        basis=[KNOWN.format(task="T288573794")],
    )
    diff_agrees(run, "s", R0, "status and diff both list r0")
    return run


def status_loss() -> Run:
    run = _dirty_status(
        "status-omits-path-diff-lists",
        {},
        [R0],
        "After an acked write to r0, sl diff lists r0 while sl status omits it (the "
        "shape the status-loss red control produces).",
        basis=["status.status-diff-mismatch (edenfs/properties/status.py)"],
        read=False,
        red_control="status-loss",
    )
    status_rule(
        run,
        STATUS_DIFF,
        "s",
        R0,
        VIOLATED,
        "diff lists r0 as content-changed; status has no entry for it",
        observed="absent",
        attribution=UNDETERMINED,
        producers="write",
    )
    return run


def diff_loss() -> Run:
    run = _dirty_status(
        "status-lists-path-diff-omits",
        {R0: "M"},
        [],
        "After an acked write to r0, sl status lists r0 M while sl diff omits it, "
        "claiming a mode-only change (the shape the diff-loss red control produces).",
        basis=["status.status-diff-mismatch (edenfs/properties/status.py)"],
        read=False,
        red_control="diff-loss",
    )
    status_rule(
        run,
        STATUS_DIFF,
        "s",
        R0,
        VIOLATED,
        "an M entry diff omits says r0's content equals rev3's, but w's bytes differ",
        observed="M",
        producers="write",
    )
    return run


def mode_only_status() -> Run:
    run = Run(
        "bytes-status",
        "mode-only-status-without-diff",
        "Legal twin of the diff-loss shape: r0's exec bit is set, sl status lists r0 M "
        "and sl diff omits it, a mode-only change.",
        basis=[
            MODE_DIRT,
            "status.status-diff-mismatch (sl diff omits a mode-only change)",
        ],
    )
    run.act("a", "m", "chmod", path=R0, mode=0o755)
    run.act("a", "s", "status", channel="sl")
    run.call("m", "ok", path=R0, mode=0o755)
    run.call("s", "ok", **sl_status({R0: "M"}, []))
    run.finish()
    run.expect(
        STATUS_DIFF,
        "s",
        SATISFIED,
        "r0's content equals rev3's and its mode differs",
        path=R0,
    )
    return run


def known_status_shapes() -> list[Run]:
    run = Run(
        "bytes-status",
        "known-T288573794-shape",
        "An acked write to r0, then a complete sl status and sl diff that both omit "
        "r0: the shape of task T288573794's reproducer.",
        basis=[KNOWN.format(task="T288573794")],
    )
    run.act("writer", "w0", "write", path=R0, value_id="w0")
    run.act("obs", "s1", "status", channel="sl", requires=[("w0", "finished")])
    run.call("w0", "ok", **WRITE_OK)
    run.call("s1", "ok", **sl_status({}, []))
    run.finish()
    omitted(run, "s1", R0, VIOLATED, "r0 holds w0's bytes during s1", producers="write")

    run2 = Run(
        "bytes-status",
        "known-T287852915-shape",
        "An acked write to r0, a complete sl status and sl diff that both omit r0, then "
        "a FUSE read of r0 with no writer in between: the shape of task T287852915's "
        "reproducer.",
        basis=[KNOWN.format(task="T287852915")],
    )
    run2.act("writer", "w0", "write", path=R0, value_id="w0")
    run2.act("obs", "s1", "status", channel="sl", requires=[("w0", "finished")])
    run2.act(
        "obs", "t0", "read", path=R0, channel="fuse", requires=[("s1", "finished")]
    )
    run2.call("w0", "ok", **WRITE_OK)
    run2.call("s1", "ok", **sl_status({}, []))
    run2.call("t0", "ok", **read_ok(value("writer", "w0")))
    run2.finish()
    omitted(
        run2,
        "s1",
        R0,
        VIOLATED,
        "r0 holds w0's bytes during s1, as t0 confirms",
        rule=SL_FUSE,
        producers="write",
    )
    return [run, run2]


def status_reports_clean_path_modified() -> Run:
    run = Run(
        "bytes-status",
        "status-reports-clean-path-modified",
        "No mutation runs; sl status and sl diff report r0 modified.",
        basis=["sl status compares the working copy with the parent manifest"],
    )
    run.act("a", "s", "status", channel="sl")
    run.call("s", "ok", **sl_status({R0: "M"}, [R0]))
    run.finish()
    code(
        run,
        "s",
        R0,
        "M",
        VIOLATED,
        "r0 holds rev3's committed bytes at every point",
        producers="none",
    )
    return run


def type_changed_status() -> list[Run]:
    """A status judges a path that is a directory at some revision (review2 C r9)."""
    runs = []
    for listed in (True, False):
        run = Run(
            "bytes-status",
            f"type-changed-path-status-{'lists' if listed else 'omits'}-write",
            "r2 is a directory at rev3 and a file at rev2. A Thrift NORMAL checkout to "
            "rev2, an acked write to r2, then a Thrift status for rev2 that "
            + ("lists r2 modified." if listed else "omits r2."),
            basis=[THRIFT_NORMAL_TABLE],
            type_changes=[{"path": R2, "at_revision": TIP}],
        )
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.act("a", "w", "write", path=R2, value_id="w", requires=[("c", "finished")])
        run.act(
            "a",
            "s",
            "status",
            channel="thrift",
            revision=2,
            requires=[("w", "finished")],
        )
        run.call("c", "ok", **thrift_checkout(2, conflicts=0))
        run.call("w", "ok", **WRITE_OK)
        run.call("s", "ok", **thrift_status({R2: "MODIFIED"} if listed else {}, 2))
        run.finish()
        run.expect(
            "checkout.conflict-count",
            "c",
            NOT_EVALUATED,
            "conflict typing for a path the checkout retypes is not modeled",
            identity={"mode": "normal"},
            not_evaluated_reason="checkout.conflict-count-unmodeled",
        )
        if listed:
            code(run, "s", R2, "M", SATISFIED, "r2 holds w's bytes, not rev2's")
        else:
            omitted(
                run,
                "s",
                R2,
                VIOLATED,
                "r2 holds w's bytes, which differ from rev2's, at every point of s",
                producers="checkout+write",
            )
        runs.append(run)
    return runs


KNOWN_FUSE = (
    "status.sl-fuse-stale-status; test/corpus/known/T287852915/run-spec.json (the "
    "reproducer of task T287852915)"
)


def tracked_status_runs() -> list[Run]:
    runs = []
    for listed in ("R", "!"):
        run = Run(
            "bytes-status",
            "sl-rm-status-removed" if listed == "R" else "sl-rm-status-missing",
            f"r0 is removed through sl rm; sl status lists r0 `{listed}` and sl diff "
            "lists it.",
            basis=["sl rm leaves the path `R` in sl status"],
        )
        run.act("a", "m", "sl_rm", path=R0)
        run.act("a", "s", "status", channel="sl")
        run.call("m", "ok", path=R0)
        run.call("s", "ok", **sl_status({R0: listed}, [R0]))
        run.finish()
        if listed == "R":
            code(
                run,
                "s",
                R0,
                "R",
                SATISFIED,
                "r0 is absent and tracked removed",
                rule=SL_MODEL,
            )
        else:
            code(
                run,
                "s",
                R0,
                "!",
                VIOLATED,
                "sl diff never lists a missing path, so a listed r0 needs an `R` "
                "entry, and sl rm recorded r0 removed",
                rule=STATUS_DIFF,
                producers="sl_rm",
            )
        runs.append(run)
    run = Run(
        "bytes-status",
        "clean-status-confirmed-by-fuse",
        "No mutation; sl status and sl diff omit r0 and a later FUSE read returns "
        "rev3's bytes, agreeing with them: the omission files under "
        "status.sl-fuse-stale-status and holds.",
        basis=[KNOWN_FUSE],
    )
    run.act("a", "s", "status", channel="sl")
    run.act("a", "t", "read", path=R0, channel="fuse")
    run.call("s", "ok", **sl_status({}, []))
    run.call("t", "ok", **read_ok(committed(R0, TIP)))
    run.finish()
    stale_read(run, "t", R0, SATISFIED, "the initial state holds rev3's bytes")
    runs.append(run)
    return runs


def collector_runs() -> list[Run]:  # noqa: C901 - corpus data, one case after another
    found: list[Run] = []

    run = Run(
        "bytes-status",
        "thrift-status-accepts-getscmstatus-code-names",
        "Thrift status accepts getscmstatus code names.",
        basis=["edenfs/properties/status.py: status.thrift-model-stale-status"],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act(
        "a", "s", "status", requires=[("w", "finished")], channel="thrift", revision=3
    )
    run.call("w", "ok", **WRITE_OK)
    run.call("s", "ok", **thrift_status({R0: "MODIFIED"}))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.thrift-model-stale-status",
            "s",
            SATISFIED,
            "no thrift model stale status",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "entries-no-action-names-are-judged",
        "sl status lists an untracked path nothing created: every listed path is "
        "judged, named by an action or not.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.model-live-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "s", "status", requires=[("w", "finished")], channel="sl")
    run.call("w", "ok", **WRITE_OK)
    run.call("s", "ok", **sl_status({"not/a/fixture/path": "?", R0: "M"}, diff=[R0]))
    run.finish()
    code(
        run,
        "s",
        "not/a/fixture/path",
        "?",
        VIOLATED,
        "no revision and no action binds not/a/fixture/path",
        rule=SL_MODEL,
        producers="none",
    )
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "revision-qualified-status-has-no-diff-comparison",
        "`sl diff` compares with `.`, so a `--rev` status is not compared with it.",
        basis=["edenfs/properties/status.py: status.sl-model-stale-status"],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act("a", "s", "status", requires=[("m", "finished")], channel="sl", revision=0)
    run.call("m", "ok", path=R0)
    run.call("s", "ok", **sl_status({R0: "R"}, diff=[]))
    run.finish()
    run.expect(
        "status.sl-model-stale-status",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path="tree/a0/r1/00000.dat",
        identity={"observed": "absent", "producer-kinds": "none"},
    )
    run.expect(
        "status.sl-model-stale-status",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path="tree/a1/r2/00001.dat",
        identity={"observed": "absent", "producer-kinds": "none"},
    )
    run.expect(
        "status.sl-model-stale-status",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R1,
        identity={"observed": "absent", "producer-kinds": "none"},
    )
    run.expect(
        "status.sl-model-stale-status",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R2,
        identity={"observed": "absent", "producer-kinds": "none"},
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "write-between-status-and-diff-is-legal",
        "A write between status and diff is legal.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("b", "s", "status", channel="sl")
    run.inv("s", "w")
    run.done("w", "ok", **WRITE_OK)
    run.done("s", "ok", **sl_status({}, diff=[R0]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "sl-rm-between-status-and-diff-is-legal",
        "An sl rm between status and diff is legal.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act("b", "s", "status", channel="sl")
    run.inv("s", "m")
    run.done("m", "ok", path=R0)
    run.done("s", "ok", **sl_status({}, diff=[R0]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "diff-cannot-list-a-write-that-never-happened",
        "The violating twin: no mutation can explain the listing.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("b", "s", "status", channel="sl")
    run.call("s", "ok", **sl_status({}, diff=[R0]))
    run.finish()
    for path in OTHER_RELEVANT_PATHS:
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    run.expect(
        "status.status-diff-mismatch",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"observed": "absent", "producer-kinds": "none"},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "concurrent-writer-cannot-explain-an-impossible-omission",
        "Not quiet, so the engine decides: every state during s holds a write.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "w0", "write", path=R0, value_id="w0")
    run.act("a", "w1", "write", requires=[("w0", "finished")], path=R0, value_id="w1")
    run.act("b", "s", "status", requires=[("w0", "finished")], channel="sl")
    run.call("w0", "ok", **WRITE_OK)
    run.inv("s", "w1")
    run.done("w1", "ok", **WRITE_OK)
    run.done("s", "ok", **sl_status({}, diff=[R0]))
    run.finish()
    for path in OTHER_RELEVANT_PATHS:
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    run.expect(
        "status.status-diff-mismatch",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"observed": "absent", "producer-kinds": "write"},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "added-path-diff-omits-is-a-mismatch-even-with-a-writer",
        "Not quiet (a rename onto the added path overlaps s): the engine still rejects.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_mv", source=R0, destination=MOVED)
    run.act(
        "a", "n", "rename", requires=[("m", "finished")], source=R1, destination=MOVED
    )
    run.act("b", "s", "status", requires=[("m", "finished")], channel="sl")
    run.call("m", "ok", source=R0, destination=MOVED)
    run.inv("s", "n")
    run.done("n", "ok", source=R1, destination=MOVED)
    run.done("s", "ok", **sl_status({MOVED: "A", R0: "R"}, diff=[]))
    run.finish()
    for path in OTHER_RELEVANT_PATHS:
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    run.expect(
        "status.status-diff-mismatch",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=MOVED,
        identity={"observed": "A", "producer-kinds": "rename+sl_mv"},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "diff-against-a-stale-sapling-parent-is-left-open",
        "After a Thrift checkout, sl status reads EdenFS's parent and diff Sapling's `.`.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "s", "status", requires=[("c", "finished")], channel="sl")
    run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call("s", "ok", **sl_status({}, diff=[R0]))
    run.finish()
    for path in (
        "tree/a0/000/00002.dat",
        "tree/a0/r1/00000.dat",
        "tree/a0/r3/00002.dat",
        "tree/a1/r2/00001.dat",
        CHURN,
        "tree/churn/c1",
        "tree/churn/c2",
        "tree/churn/c3",
        R0,
        R1,
        R2,
    ):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "added-path-diff-omits-is-a-mismatch",
        "An added path diff omits is a mismatch.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_mv", source=R0, destination=MOVED)
    run.act("a", "s", "status", requires=[("m", "finished")], channel="sl")
    run.call("m", "ok", source=R0, destination=MOVED)
    run.call("s", "ok", **sl_status({MOVED: "A", R0: "R"}, diff=[]))
    run.finish()
    for path in OTHER_RELEVANT_PATHS:
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    run.expect(
        "status.status-diff-mismatch",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=MOVED,
        identity={"observed": "A", "producer-kinds": "sl_mv"},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "thrift-status-reports-a-mode-only-change-thrift-mode",
        "Thrift status reports a mode only change (chmod then Thrift status).",
        basis=["edenfs/properties/status.py: status.thrift-model-stale-status"],
    )
    run.act("a", "m", "chmod", path=R0, mode=493)
    run.act(
        "a", "s", "status", requires=[("m", "finished")], channel="thrift", revision=3
    )
    run.call("m", "ok", path=R0, mode=493)
    run.call("s", "ok", **thrift_status({R0: "MODIFIED"}))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.thrift-model-stale-status",
            "s",
            SATISFIED,
            "no thrift model stale status",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "thrift-status-reports-a-mode-only-change-thrift-mode-clean",
        "Thrift status reports a mode only change (chmod then a clean Thrift status).",
        basis=["edenfs/properties/status.py: status.thrift-model-stale-status"],
    )
    run.act("a", "m", "chmod", path=R0, mode=493)
    run.act(
        "a", "s", "status", requires=[("m", "finished")], channel="thrift", revision=3
    )
    run.call("m", "ok", path=R0, mode=493)
    run.call("s", "ok", **thrift_status({}))
    run.finish()
    for path in OTHER_RELEVANT_PATHS:
        run.expect(
            "status.thrift-model-stale-status",
            "s",
            SATISFIED,
            "no thrift model stale status",
            path=path,
        )
    run.expect(
        "status.thrift-model-stale-status",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"observed": "absent", "producer-kinds": "chmod"},
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "removed-path-that-reappears-is-r-1",
        "Sapling lists a removed path `R` whether or not its file is back (`status.rs:123`); `?` is illegal, also when the status races the rename, where no direct comparison applies (sl rm r0, rename r1 onto r0, sl status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act("a", "n", "rename", requires=[("m", "finished")], source=R1, destination=R0)
    run.act("b", "s", "status", requires=[("n", "finished")], channel="sl")
    run.call("m", "ok", path=R0)
    run.call("n", "ok", source=R1, destination=R0)
    run.call("s", "ok", **sl_status({R0: "R", R1: "!"}, diff=[R0]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "removed-path-that-reappears-is-r-2",
        "Sapling lists a removed path `R` whether or not its file is back (`status.rs:123`); `?` is illegal, also when the status races the rename, where no direct comparison applies (sl rm r0, rename r1 onto r0, sl status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act("a", "n", "rename", requires=[("m", "finished")], source=R1, destination=R0)
    run.act("b", "s", "status", requires=[("n", "finished")], channel="sl")
    run.call("m", "ok", path=R0)
    run.call("n", "ok", source=R1, destination=R0)
    run.call("s", "ok", **sl_status({R0: "?", R1: "!"}, diff=[]))
    run.finish()
    run.expect(
        "status.sl-model-stale-status",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"observed": "?", "producer-kinds": "rename+sl_rm"},
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "removed-path-that-reappears-is-r-3",
        "Sapling lists a removed path `R` whether or not its file is back (`status.rs:123`); `?` is illegal, also when the status races the rename, where no direct comparison applies (sl rm r0, rename r1 onto r0, sl status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act("a", "n", "rename", requires=[("m", "finished")], source=R1, destination=R0)
    run.act("b", "s", "status", requires=[("n", "finished")], channel="sl")
    run.call("m", "ok", path=R0)
    run.call("n", "ok", source=R1, destination=R0)
    run.call("s", "ok", **sl_status({R0: "?", R1: "!"}, diff=[R0]))
    run.finish()
    for path in OTHER_RELEVANT_PATHS:
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    run.expect(
        "status.status-diff-mismatch",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"observed": "?", "producer-kinds": "rename+sl_rm"},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "removed-path-that-reappears-is-r-4",
        "Sapling lists a removed path `R` whether or not its file is back (`status.rs:123`); `?` is illegal, also when the status races the rename, where no direct comparison applies (sl rm r0, rename r1 onto r0, sl status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act("a", "n", "rename", requires=[("m", "finished")], source=R1, destination=R0)
    run.act("b", "s", "status", requires=[("m", "finished")], channel="sl")
    run.call("m", "ok", path=R0)
    run.inv("n", "s")
    run.done("n", "ok", source=R1, destination=R0)
    run.done("s", "ok", **sl_status({R0: "R", R1: "!"}, diff=[R0]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "removed-path-that-reappears-is-r-5",
        "Sapling lists a removed path `R` whether or not its file is back (`status.rs:123`); `?` is illegal, also when the status races the rename, where no direct comparison applies (sl rm r0, rename r1 onto r0, sl status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act("a", "n", "rename", requires=[("m", "finished")], source=R1, destination=R0)
    run.act("b", "s", "status", requires=[("m", "finished")], channel="sl")
    run.call("m", "ok", path=R0)
    run.inv("n", "s")
    run.done("n", "ok", source=R1, destination=R0)
    run.done("s", "ok", **sl_status({R0: "?", R1: "!"}, diff=[]))
    run.finish()
    run.expect(
        "status.sl-model-stale-status",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"observed": "?", "producer-kinds": "rename+sl_rm"},
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "removed-path-that-reappears-is-r-6",
        "Sapling lists a removed path `R` whether or not its file is back (`status.rs:123`); `?` is illegal, also when the status races the rename, where no direct comparison applies (sl rm r0, rename r1 onto r0, sl status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act("a", "n", "rename", requires=[("m", "finished")], source=R1, destination=R0)
    run.act("b", "s", "status", requires=[("m", "finished")], channel="sl")
    run.call("m", "ok", path=R0)
    run.inv("n", "s")
    run.done("n", "ok", source=R1, destination=R0)
    run.done("s", "ok", **sl_status({R0: "?", R1: "!"}, diff=[R0]))
    run.finish()
    for path in OTHER_RELEVANT_PATHS:
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    run.expect(
        "status.status-diff-mismatch",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"observed": "?", "producer-kinds": "rename+sl_rm"},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "bytes-status",
        "recorded-live-outputs-are-legal-live-sl-mv",
        "Each checked-in live status/diff result, after its mutation, accepts (a recorded live status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "x0", "sl_mv", destination=MOVED, source=R0)
    run.act("a", "x1", "status", requires=[("x0", "finished")], channel="sl")
    run.call("x0", "ok", destination=MOVED, source=R0)
    run.call("x1", "ok", **sl_status({MOVED: "A", R0: "R"}, diff=[MOVED]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, MOVED, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "x1",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "recorded-live-outputs-are-legal-live-sl-rm",
        "Each checked-in live status/diff result, after its mutation, accepts (a recorded live status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "x0", "sl_rm", path=R0)
    run.act("a", "x1", "status", requires=[("x0", "finished")], channel="sl")
    run.call("x0", "ok", path=R0)
    run.call("x1", "ok", **sl_status({R0: "R"}, diff=[R0]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "x1",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "recorded-live-outputs-are-legal-live-unlink",
        "Each checked-in live status/diff result, after its mutation, accepts (a recorded live status).",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "x0", "unlink", path=R0)
    run.act("a", "x1", "status", requires=[("x0", "finished")], channel="sl")
    run.call("x0", "ok", path=R0)
    run.call("x1", "ok", **sl_status({R0: "!"}, diff=[]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "x1",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "status-against-another-revision-lists-manifest-differences",
        "`sl status --rev 0` at a clean tip: `A`/`R`/`M` for every path the two "
        "manifests disagree on, diff empty.",
        basis=["edenfs/properties/status.py: status.sl-model-stale-status"],
    )
    run.act("a", "s", "status", channel="sl", revision=0)
    run.call(
        "s",
        "ok",
        **sl_status(
            {
                "tree/a0/000/00000.dat": "R",
                "tree/a0/000/00002.dat": "R",
                "tree/a1/000/00001.dat": "R",
                "tree/a0/r1/00000.dat": "A",
                "tree/a0/r3/00002.dat": "A",
                "tree/a1/r2/00001.dat": "A",
                R0: "M",
                R1: "M",
                R2: "M",
            },
            diff=[],
        ),
    )
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.sl-model-stale-status",
            "s",
            SATISFIED,
            "no sl model stale status",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "missing-path-is-not-in-diff",
        "Missing path is not in diff.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "u", "unlink", path=R0)
    run.act("a", "s", "status", requires=[("u", "finished")], channel="sl")
    run.call("u", "ok", path=R0)
    run.call("s", "ok", **sl_status({R0: "!"}, diff=[]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "sl-mv-lists-only-the-destination-in-diff",
        "`sl mv` lists only the destination in diff.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "m", "sl_mv", source=R0, destination=MOVED)
    run.act("a", "s", "status", requires=[("m", "finished")], channel="sl")
    run.call("m", "ok", source=R0, destination=MOVED)
    run.call("s", "ok", **sl_status({MOVED: "A", R0: "R"}, diff=[MOVED]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, MOVED, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "sl-mv-onto-a-tracked-path-is-modified",
        "Sapling keeps a tracked destination tracked (a copy): `M`, not `A`.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.model-live-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "u", "unlink", path=R1)
    run.act("a", "m", "sl_mv", requires=[("u", "finished")], source=R0, destination=R1)
    run.act("a", "s", "status", requires=[("m", "finished")], channel="sl")
    run.call("u", "ok", path=R1)
    run.call("m", "ok", source=R0, destination=R1)
    run.call("s", "ok", **sl_status({R0: "R", R1: "M"}, diff=[R0, R1]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "concurrent-writer-lets-status-and-diff-disagree",
        "A concurrent writer lets status and diff disagree.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "s", "status", channel="sl")
    run.act("b", "m", "rename", source=R0, destination=R1)
    run.inv("s", "m")
    run.done("m", "ok", source=R0, destination=R1)
    run.done("s", "ok", **sl_status({}, diff=[R1]))
    run.finish()
    for path in (*OTHER_RELEVANT_PATHS, R0, R1, R2):
        run.expect(
            "status.status-diff-mismatch",
            "s",
            SATISFIED,
            "no status diff mismatch",
            path=path,
        )
    found.append(run)

    run = Run(
        "bytes-status",
        "serial-status-still-compares-with-its-diff",
        "A serial status still compares with its diff.",
        basis=[
            "edenfs/properties/status.py: status.sl-fuse-stale-status, status.sl-model-stale-status, status.status-diff-mismatch"
        ],
    )
    run.act("a", "s", "status", channel="sl")
    run.act("b", "m", "rename", source=R0, destination=R1)
    run.call("m", "ok", source=R0, destination=R1)
    run.call("s", "ok", **sl_status({R0: "!"}, diff=[R1]))
    run.finish()
    run.expect(
        "status.status-diff-mismatch",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R1,
        identity={"observed": "absent", "producer-kinds": "rename"},
        attribution="undetermined",
    )
    found.append(run)
    return found


def status_then_diff_runs() -> list[Run]:
    """`sl status` and its `sl diff` overlap a write of r0; diff runs second."""
    runs = []
    for listed in (False, True):
        run = Run(
            "bytes-status",
            "status-then-diff-"
            + ("lists-then-omits" if listed else "omits-then-lists"),
            "A write of r0 overlaps sl status; status "
            + (
                "lists r0 modified, then its later sl diff omits it."
                if listed
                else "omits r0, then its later sl diff lists it."
            ),
            basis=["the executor runs `sl diff` only after `sl status` exits"],
        )
        run.act("a", "w0", "write", path=R0, value_id="w0")
        run.act("b", "s", "status", channel="sl")
        run.inv("s")
        run.inv("w0")
        run.done("w0", "ok", **WRITE_OK)
        entries, diff = ({R0: "M"}, []) if listed else ({}, [R0])
        run.done("s", "ok", **sl_status(entries, diff))
        run.finish()
        if listed:
            status_rule(
                run,
                STATUS_DIFF,
                "s",
                R0,
                VIOLATED,
                "the write landed before status read r0, so diff, run after it, sees it",
                observed="M",
                attribution=UNDETERMINED,
                producers="write",
            )
        else:
            diff_agrees(run, "s", R0, "the write landed between status and diff")
        runs.append(run)
    return runs


def runs() -> list[Run]:
    return [
        *status_then_diff_runs(),
        force_checkout_write_cycle(),
        force_checkout_write_read_and_dirty_status(),
        force_checkout_write_superseded(),
        force_checkout_write_lost(),
        _write_after_observed_reset(kept=False),
        _write_after_observed_reset(kept=True),
        dirty_bytes_clean_status(),
        dirty_bytes_dirty_status(),
        status_loss(),
        diff_loss(),
        mode_only_status(),
        *known_status_shapes(),
        status_reports_clean_path_modified(),
        *type_changed_status(),
        *tracked_status_runs(),
        *collector_runs(),
    ]
