# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `checkout`, Sapling side: sl goto's two phases, aborts, and
the paths they leave."""

from __future__ import annotations

from eden.integration.correctness.exploratory.edenfs.actions import WALK_DEADLOCK_TASK
from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    MOVED,
    R0,
    R1,
    R2,
    read_ok,
    Run,
    SATISFIED,
    sl_abort,
    sl_checkout,
    sl_status,
    thrift_checkout,
    thrift_status,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    ENOENT,
    inputs_ok,
    MODE_DIRT,
    SL_ABORT_OR_SUCCESS,
    sl_abort_rule,
    SL_GOTO_CONTRACT,
    SL_RM_FORCE,
    stale_read,
    status_rule,
)


def _sl(
    name: str,
    description: str,
    *,
    dirty: str,
    ok: bool,
    observe: str | None,
    basis: str = SL_ABORT_OR_SUCCESS,
) -> Run:
    run = Run("checkout", name, description, basis=[basis])
    if dirty == "write":
        run.act("a", "w", "write", path=R0, value_id="w")
    elif dirty == "unlink":
        run.act("a", "u", "unlink", path=R0)
    else:
        run.act("a", "m", "chmod", path=R0, mode=0o755)
    run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
    if observe is not None:
        run.act("a", "r", "read", path=R0, channel="fuse")
    if dirty == "write":
        run.call("w", "ok", **WRITE_OK)
        inputs_ok(run, "w")
    elif dirty == "unlink":
        run.call("u", "ok", path=R0)
    else:
        run.call("m", "ok", path=R0, mode=0o755)
    if ok:
        run.call("c", "ok", **sl_checkout(2))
    else:
        run.call("c", "fail", **sl_abort([R0]))
    if observe == "enoent":
        run.call("r", "fail", **errno_fail(ENOENT))
    elif observe is not None:
        data = {"rev2": committed(R0, 2), "write": value("a", "w")}[observe]
        run.call("r", "ok", **read_ok(data))
    run.finish()
    return run


def _sl_runs() -> list[Run]:
    runs = []
    run = _sl(
        "sl-goto-dirty-changed-aborts",
        "r0 is written; sl goto to rev2 aborts listing r0, and r0 keeps the write.",
        dirty="write",
        ok=False,
        observe="write",
    )
    stale_read(run, "r", R0, SATISFIED, "an abort has no effect")
    runs.append(run)

    run = _sl(
        "sl-goto-abort-with-effect",
        "r0 is written; sl goto to rev2 aborts, yet r0 then reads rev2's bytes.",
        dirty="write",
        ok=False,
        observe="rev2",
    )
    stale_read(run, "r", R0, VIOLATED, "an aborted sl goto changes nothing")
    runs.append(run)

    run = _sl(
        "sl-goto-dirty-changed-succeeds",
        "r0 is written; sl goto to rev2 "
        "succeeds although the dirty changed r0 must block it.",
        dirty="write",
        ok=True,
        observe=None,
    )
    sl_abort_rule(
        run, "c", VIOLATED, "success while a blocking pre-state held", "succeeded"
    )
    runs.append(run)

    run = _sl(
        "sl-goto-removed-modified-aborts",
        "r0 is unlinked; sl goto to rev2 aborts listing r0, and r0 stays absent.",
        dirty="unlink",
        ok=False,
        observe="enoent",
    )
    sl_abort_rule(
        run,
        "c",
        SATISFIED,
        "an unlinked path the destination changes blocks",
        "aborted",
    )
    runs.append(run)

    run = _sl(
        "sl-goto-removed-modified-succeeds",
        "r0 is unlinked; sl goto to rev2 "
        "succeeds although the unlinked changed r0 must block it.",
        dirty="unlink",
        ok=True,
        observe=None,
    )
    sl_abort_rule(
        run, "c", VIOLATED, "success while a blocking pre-state held", "succeeded"
    )
    runs.append(run)

    run = _sl(
        "sl-goto-mode-only-dirt-aborts",
        "r0's exec bit is set; sl goto to rev2 aborts listing r0.",
        dirty="chmod",
        ok=False,
        observe=None,
        basis=MODE_DIRT,
    )
    sl_abort_rule(
        run, "c", SATISFIED, "mode-only dirt on a changed path blocks", "aborted"
    )
    runs.append(run)

    run = _sl(
        "sl-goto-mode-only-dirt-succeeds",
        "r0's exec bit is set; sl goto to rev2 succeeds.",
        dirty="chmod",
        ok=True,
        observe=None,
        basis=MODE_DIRT,
    )
    sl_abort_rule(
        run, "c", VIOLATED, "mode-only dirt on a changed path blocks", "succeeded"
    )
    runs.append(run)
    return runs


def _missing_removed(name: str, description: str, *, ok: bool) -> Run:
    """r2 is a directory at rev3 and a file at rev2; renaming it away leaves its child absent."""
    run = Run(
        "checkout",
        name,
        description,
        basis=[SL_GOTO_CONTRACT],
        type_changes=[{"path": R2, "at_revision": TIP}],
    )
    child = run.layout().type_change_child(R2).path
    run.act("a", "m", "rename", source=R2, destination=MOVED)
    run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
    run.call("m", "ok", source=R2, destination=MOVED)
    if ok:
        run.call("c", "ok", **sl_checkout(2))
    else:
        run.call("c", "fail", **sl_abort([child]))
    run.finish()
    inputs_ok(run, "m")
    return run


def _tracked_removal(name: str, description: str, *, move: bool, ok: bool) -> Run:
    run = Run(
        "checkout",
        name,
        description,
        basis=["sl rm or sl mv of a path the destination changes blocks sl goto"],
    )
    if move:
        run.act("a", "m", "sl_mv", source=R0, destination=MOVED)
    else:
        run.act("a", "m", "sl_rm", path=R0)
    run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
    if move:
        run.call("m", "ok", source=R0, destination=MOVED)
    else:
        run.call("m", "ok", path=R0)
    if ok:
        run.call("c", "ok", **sl_checkout(2))
    else:
        run.call("c", "fail", **sl_abort([R0]))
    run.finish()
    return run


def abort_runs() -> list[Run]:
    runs = []
    for ok in (False, True):
        run = _missing_removed(
            (
                "sl-goto-missing-removed-succeeds"
                if ok
                else "sl-goto-missing-removed-aborts"
            ),
            "r2 (a directory at rev3, a file at rev2) is renamed away, so its child is "
            "absent while rev2 removes it; sl goto to rev2 "
            + ("succeeds." if ok else "aborts listing the child."),
            ok=ok,
        )
        sl_abort_rule(
            run,
            "c",
            VIOLATED if ok else SATISFIED,
            "a plainly unlinked path the destination removes (MISSING_REMOVED) blocks",
            "succeeded",
        )
        runs.append(run)
    for move in (False, True):
        for ok in (False, True):
            verb = "sl-mv" if move else "sl-rm"
            run = _tracked_removal(
                f"sl-goto-{verb}-{'succeeds' if ok else 'aborts'}",
                f"r0 is removed through `{verb.replace('-', ' ')}`; sl goto to rev2 "
                + ("succeeds." if ok else "aborts listing r0."),
                move=move,
                ok=ok,
            )
            sl_abort_rule(
                run,
                "c",
                VIOLATED if ok else SATISFIED,
                "a changed path Sapling records removed blocks sl goto",
                "succeeded",
            )
            runs.append(run)
    return runs


SL_GOTO_PHASES = (
    "sl goto runs a DRY_RUN checkout, then a NORMAL one; operations between the "
    "two are kept (`edenfs.rs:366-426`)"
)


def _gap(name: str, description: str, *, op: str, observe: str) -> Run:
    """An operation lands inside `sl goto` to rev2 (between its two phases)."""
    run = Run("checkout", name, description, basis=[SL_GOTO_PHASES])
    run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
    if op == "chmod":
        run.act("b", "x", "chmod", path=R0, mode=0o755)
        done: dict[str, object] = {"path": R0, "mode": 0o755}
    else:
        run.act("b", "x", "rename", source=R1, destination=R0)
        done = {"source": R1, "destination": R0}
        run.metadata["task"] = WALK_DEADLOCK_TASK
    run.act(
        "o",
        "g",
        "read",
        path=R0,
        channel="fuse",
        requires=[("c", "finished"), ("x", "finished")],
    )
    if op == "rename":
        run.act("o", "h", "read", path=R1, channel="fuse", requires=[("g", "finished")])
    run.inv("c")
    run.inv("x")
    run.done("x", "ok", **done)
    run.done("c", "ok", **sl_checkout(2))
    data = {
        "rev3": committed(R0, TIP),
        "r1": committed(R1, TIP),
        "rev2": committed(R0, 2),
    }[observe]
    run.call("g", "ok", **read_ok(data))
    if op == "rename":
        run.call("h", "fail", **errno_fail(ENOENT))
    run.finish()
    return run


def _set_parents_refused() -> Run:
    """B5-1: Sapling's `set_parents` is refused after a completed walk."""
    run = Run(
        "checkout",
        "sl-goto-set-parents-refused-after-walk",
        "sl goto to rev2 applies its walk (a read inside sees rev2's bytes), then "
        "`set_parents` is refused because a Thrift checkout to rev1 began; a "
        "status after both sees rev1.",
        basis=[SL_GOTO_PHASES],
    )
    run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
    run.act("b", "r", "read", path=R0, channel="fuse")
    run.act(
        "b",
        "t",
        "checkout",
        revision=1,
        via="thrift",
        mode="normal",
        requires=[("r", "finished")],
    )
    run.act(
        "a", "s", "status", channel="thrift", revision=1, requires=[("c", "finished")]
    )
    run.inv("c")
    run.call("r", "ok", **read_ok(committed(R0, 2)))
    run.inv("t")
    run.done(
        "c",
        "fail",
        reason="goto: abort",
        returncode=255,
        stdout="",
        stderr="abort: EdenError: cannot reset parent while a checkout is "
        "currently in progress\n",
    )
    run.done("t", "ok", **thrift_checkout(1, 0))
    run.call("s", "ok", **thrift_status({}, revision=1))
    run.finish()
    stale_read(
        run, "r", R0, SATISFIED, "the walk applied r0 before set_parents was refused"
    )
    return run


def two_phase_runs() -> list[Run]:
    runs = [_set_parents_refused()]
    run = _gap(
        "sl-goto-chmod-in-gap-kept",
        "A chmod of r0 lands inside sl goto to rev2; r0 keeps rev3's bytes.",
        op="chmod",
        observe="rev3",
    )
    stale_read(
        run,
        "g",
        R0,
        SATISFIED,
        "the chmod landed before r0's NORMAL apply point, which kept it",
    )
    runs.append(run)

    run = _gap(
        "sl-goto-rename-in-gap-kept",
        "A rename of r1 onto r0 lands inside sl goto to rev2; r0 holds r1's "
        "rev3 bytes and r1 stays absent.",
        op="rename",
        observe="r1",
    )
    stale_read(
        run,
        "g",
        R0,
        SATISFIED,
        "NORMAL kept the renamed r0 and the DRY_RUN plan was empty",
    )
    runs.append(run)

    run = Run(
        "checkout",
        "sl-goto-abort-lists-clean-path",
        "sl goto to rev2 aborts listing r0, which is chmodded only after the "
        "abort returned.",
        basis=[SL_GOTO_PHASES],
    )
    run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
    run.act("a", "x", "chmod", path=R0, mode=0o755)
    run.call("c", "fail", **sl_abort([R0]))
    run.call("x", "ok", path=R0, mode=0o755)
    run.finish()
    inputs_ok(run, "x")
    sl_abort_rule(
        run,
        "c",
        VIOLATED,
        "r0 was clean at its DRY_RUN visit, so it could not block",
        "aborted",
        blockers=("not-blocking",),
    )
    runs.append(run)

    run = Run(
        "checkout",
        "sl-goto-abort-lists-clean-path-beside-blocker",
        "r0 is written before sl goto to rev2, which aborts listing r0 and r1; r1 "
        "is clean throughout.",
        basis=[SL_GOTO_PHASES],
    )
    run.act("b", "w", "write", path=R0, value_id="w")
    run.act(
        "a",
        "c",
        "checkout",
        revision=2,
        via="goto",
        mode="normal",
        requires=[("w", "finished")],
    )
    run.call("w", "ok", bytes_written=64)
    run.call("c", "fail", **sl_abort([R0, R1]))
    run.finish()
    inputs_ok(run, "w")
    sl_abort_rule(
        run,
        "c",
        VIOLATED,
        "r1 was clean at its DRY_RUN visit, so it could not block",
        "aborted",
        blockers=("not-blocking",),
    )
    runs.append(run)

    run = Run(
        "checkout",
        "sl-goto-dirt-before-dryrun-succeeds",
        "A write to r0 finishes before sl goto to rev2 is invoked; the update "
        "succeeds.",
        basis=[SL_GOTO_PHASES],
    )
    run.act("b", "w", "write", path=R0, value_id="w")
    run.act(
        "a",
        "c",
        "checkout",
        revision=2,
        via="goto",
        mode="normal",
        requires=[("w", "finished")],
    )
    run.call("w", "ok", bytes_written=64)
    run.call("c", "ok", **sl_checkout(2))
    run.finish()
    sl_abort_rule(
        run,
        "c",
        VIOLATED,
        "r0 was dirty at its DRY_RUN visit, which blocks",
        "succeeded",
    )
    runs.append(run)
    return runs


def removed_through_sapling_runs() -> list[Run]:
    """`sl rm` of the directory r2 (a file at rev2) removes its child, which rev2 removes.

    EdenFS ignores Sapling's tracking, so the child reaches Sapling as
    MISSING_REMOVED, like a plain unlink, and blocks.
    """
    runs = []
    for aborted in (False, True):
        run = Run(
            "checkout",
            "sl-goto-over-sl-rm-removed-" + ("aborts" if aborted else "succeeds"),
            "sl rm r2 (a directory at rev3) removes its child; sl goto to rev2, "
            "which removes the child, "
            + ("aborts listing only the child." if aborted else "succeeds."),
            basis=[SL_RM_FORCE, SL_GOTO_CONTRACT],
            type_changes=[{"path": R2, "at_revision": TIP}],
        )
        child = run.layout().type_change_child(R2).path
        run.act("a", "h", "sl_rm", path=R2)
        run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
        run.call("h", "ok", path=R2)
        if aborted:
            run.call("c", "fail", **sl_abort([child]))
        else:
            run.call("c", "ok", **sl_checkout(2))
        run.finish()
        if aborted:
            sl_abort_rule(
                run,
                "c",
                SATISFIED,
                "the only listed path is MISSING_REMOVED, which blocks",
                "aborted",
            )
        else:
            sl_abort_rule(
                run,
                "c",
                VIOLATED,
                "a file removed through Sapling that rev2 removes blocks",
                "succeeded",
            )
        runs.append(run)
    return runs


GOTO_CLEAN = (
    "`sl goto --clean` runs a Thrift FORCE checkout, then forgets every path's "
    "tracking and moves Sapling's parent (`edenfs_checkout` with "
    "`revert_conflicts`: `edenfs_force_checkout`, `clear_edenfs_dirstate`, "
    "`set_parents`)"
)


def _goto_clean(name: str, description: str, *, dirt: str) -> Run:
    run = Run("checkout", name, description, basis=[GOTO_CLEAN])
    if dirt == "write":
        run.act("a", "w", "write", path=R0, value_id="w")
    else:
        run.act("a", "h", "sl_rm", path=R1)
    run.act("a", "c", "checkout", revision=2, via="goto", mode="force")
    if dirt == "write":
        run.act("a", "r", "read", path=R0, channel="fuse")
        run.call("w", "ok", **WRITE_OK)
    else:
        run.act("a", "s", "status", channel="sl")
        run.call("h", "ok", path=R1)
    run.call("c", "ok", **sl_checkout(2))
    return run


def goto_clean_runs() -> list[Run]:
    runs = []
    for kept in (False, True):
        run = _goto_clean(
            (
                "goto-clean-keeps-dirty-register"
                if kept
                else "goto-clean-resets-dirty-register"
            ),
            "r0 is written; `sl goto --clean` to rev2 succeeds; r0 then reads "
            + ("the written bytes." if kept else "rev2's bytes."),
            dirt="write",
        )
        run.call("r", "ok", **read_ok(value("a", "w") if kept else committed(R0, 2)))
        run.finish()
        stale_read(
            run,
            "r",
            R0,
            VIOLATED if kept else SATISFIED,
            "FORCE resets r0 to rev2 over local dirt, with no abort",
        )
        runs.append(run)
    for kept in (False, True):
        run = _goto_clean(
            "goto-clean-keeps-sl-rm" if kept else "goto-clean-forgets-sl-rm",
            "r1 is removed through Sapling; `sl goto --clean` to rev2 succeeds; "
            + ("sl status still lists r1 removed." if kept else "sl status is clean."),
            dirt="sl_rm",
        )
        status = sl_status({R1: "R"} if kept else {})
        run.call("s", "ok", **{**status, "parent": sl_checkout(2)["node"]})
        run.finish()
        status_rule(
            run,
            "status.sl-model-stale-status" if kept else "status.sl-fuse-stale-status",
            "s",
            R1,
            VIOLATED if kept else SATISFIED,
            "`clear_edenfs_dirstate` forgets r1's removal and FORCE restores it",
            observed="R" if kept else "",
            producers="checkout+sl_rm" if kept else "",
        )
        runs.append(run)
    return runs


SET_PARENTS = (
    "`sl goto` ends with `set_parents`, whose `resetParentCommits` moves EdenFS's "
    "parent to the update's target without touching files "
    "(`EdenMount::resetParent`)"
)


def set_parents_race_runs() -> list[Run]:
    """A Thrift checkout runs between an `sl goto`'s NORMAL walk and its `set_parents`."""
    runs = []
    for observe in ("out-of-date", "noop-checkout"):
        run = Run(
            "checkout",
            f"sl-goto-set-parents-after-thrift-checkout-{observe}",
            "sl goto to rev1 and a Thrift NORMAL to rev2 overlap; r0 then reads "
            "rev2's bytes while EdenFS's parent is rev1: "
            + (
                "a Thrift status for rev2 fails OUT_OF_DATE_PARENT."
                if observe == "out-of-date"
                else "a Thrift NORMAL to rev1 is a no-op and r0 keeps rev2's bytes."
            ),
            basis=[SET_PARENTS],
            registers=[R0],
        )
        run.act("a", "c2", "checkout", revision=1, via="goto", mode="normal")
        run.act("b", "c3", "checkout", revision=2, via="thrift", mode="normal")
        after = [("c2", "finished"), ("c3", "finished")]
        run.act("o", "r", "read", path=R0, channel="fuse", requires=after)
        last = [*after, ("r", "finished")]
        if observe == "out-of-date":
            run.act("o", "s", "status", channel="thrift", revision=2, requires=last)
        else:
            run.act(
                "o",
                "c4",
                "checkout",
                revision=1,
                via="thrift",
                mode="normal",
                requires=last,
            )
            run.act(
                "o",
                "r4",
                "read",
                path=R0,
                channel="fuse",
                requires=[("c4", "finished")],
            )
        run.inv("c2")
        run.inv("c3")
        run.done("c3", "ok", **thrift_checkout(2, conflicts=0))
        run.done("c2", "ok", **sl_checkout(1))
        run.call("r", "ok", **read_ok(committed(R0, 2)))
        if observe == "out-of-date":
            run.call(
                "s",
                "fail",
                reason="error computing status: requested parent commit is out-of-date",
                source="thrift",
                error_type="OUT_OF_DATE_PARENT",
            )
        else:
            run.call("c4", "ok", **thrift_checkout(1, conflicts=0))
            run.call("r4", "ok", **read_ok(committed(R0, 2)))
        run.finish()
        stale_read(run, "r", R0, SATISFIED, "c3 applied r0 after c2's walk")
        runs.append(run)
    return runs


def runs() -> list[Run]:
    return [
        *set_parents_race_runs(),
        *goto_clean_runs(),
        *_sl_runs(),
        *abort_runs(),
        *two_phase_runs(),
        *removed_through_sapling_runs(),
    ]
