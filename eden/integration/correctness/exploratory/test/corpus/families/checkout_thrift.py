# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `checkout`, Thrift side: NORMAL and FORCE checkouts, conflict
counts, parent order, overlapping checkouts, and client timeouts."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    CHURN,
    committed,
    errno_fail,
    R0,
    R1,
    read_ok,
    Run,
    SATISFIED,
    sl_status,
    thrift_checkout,
    thrift_rejected,
    thrift_status,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    CHECKOUT_GUARD,
    code,
    ENOENT,
    inputs_ok,
    omitted,
    stale_read,
    status_rule,
    THRIFT_MODEL,
    THRIFT_NORMAL_TABLE,
)


def _conflicts(run: "Run", anchor: str, outcome: str, reason: str) -> None:
    mode = run.actions[anchor]["mode"]
    identity = {"mode": mode} if outcome == VIOLATED else {}
    run.expect("checkout.conflict-count", anchor, outcome, reason, identity=identity)


def _normal(
    name: str,
    description: str,
    *,
    dirty: str | None,
    conflicts: int,
    observe: str,
    basis: str = THRIFT_NORMAL_TABLE,
) -> Run:
    """Dirty r0 (write), churn (write), or nothing; Thrift NORMAL to rev2; observe."""
    run = Run("checkout", name, description, basis=[basis])
    if dirty == "r0":
        run.act("a", "w", "write", path=R0, value_id="w")
    elif dirty == "churn":
        run.act("a", "w", "write", path=CHURN, value_id="w")
    elif dirty == "unlink":
        run.act("a", "u", "unlink", path=R0)
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    if observe.startswith("status"):
        run.act("a", "s", "status", channel="sl")
    else:
        run.act("a", "r", "read", path=R0, channel="fuse")
    if dirty in ("r0", "churn"):
        run.call("w", "ok", **WRITE_OK)
    elif dirty == "unlink":
        run.call("u", "ok", path=R0)
    run.call("c", "ok", **thrift_checkout(2, conflicts=conflicts))
    if observe == "status-dirty":
        run.call("s", "ok", **sl_status({CHURN: "M"}, [CHURN]))
    elif observe == "status-clean":
        run.call("s", "ok", **sl_status({}))
    elif observe == "enoent":
        run.call("r", "fail", **errno_fail(ENOENT))
    else:
        data = {
            "rev2": committed(R0, 2),
            "rev3": committed(R0, TIP),
            "write": value("a", "w"),
        }[observe]
        run.call("r", "ok", **read_ok(data))
    run.finish()
    if observe in ("rev2", "rev3", "write"):
        inputs_ok(run, "r")
    if observe.startswith("status"):
        inputs_ok(run, "s")
    return run


def _force(name: str, description: str, *, path: str, observe: str) -> Run:
    run = Run(
        "checkout",
        name,
        description,
        basis=[THRIFT_NORMAL_TABLE + " (FORCE resets S∪T)"],
    )
    run.act("a", "w", "write", path=path, value_id="w")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="force")
    if observe.startswith("status"):
        run.act("a", "s", "status", channel="sl")
    else:
        run.act("a", "r", "read", path=R0, channel="fuse")
    run.call("w", "ok", **WRITE_OK)
    run.call("c", "ok", **thrift_checkout(2, conflicts=1))
    if observe == "status-clean":
        run.call("s", "ok", **sl_status({}))
    elif observe == "status-dirty":
        run.call("s", "ok", **sl_status({CHURN: "M"}, [CHURN]))
    else:
        data = committed(R0, 2) if observe == "rev2" else value("a", "w")
        run.call("r", "ok", **read_ok(data))
    run.finish()
    inputs_ok(run, "w", "s" if observe.startswith("status") else "r")
    return run


def _force_runs() -> list[Run]:
    runs = []
    run = _force(
        "force-resets-dirty-register",
        "r0 is written; FORCE to rev2 resets it.",
        path=R0,
        observe="rev2",
    )
    stale_read(run, "r", R0, SATISFIED, "FORCE produces T(r0) over local dirt")
    runs.append(run)
    run = _force(
        "force-keeps-dirty-register",
        "r0 is written; after FORCE to rev2 it still reads the write's bytes.",
        path=R0,
        observe="write",
    )
    stale_read(run, "r", R0, VIOLATED, "FORCE superseded the earlier write")
    runs.append(run)
    run = _force(
        "force-resets-dirty-unchanged",
        "A churn file rev2 does not change is "
        "written; FORCE to rev2 resets it and sl status is clean.",
        path=CHURN,
        observe="status-clean",
    )
    omitted(run, "s", CHURN, SATISFIED, "FORCE resets every path either revision holds")
    runs.append(run)
    run = _force(
        "force-keeps-dirty-unchanged",
        "A churn file is written; after FORCE to "
        "rev2 sl status still reports it modified.",
        path=CHURN,
        observe="status-dirty",
    )
    code(
        run,
        "s",
        CHURN,
        "M",
        VIOLATED,
        "FORCE reset the churn file before the status",
        producers="checkout+write",
    )
    runs.append(run)
    return runs


SUPERSEDE_PER_OP = (
    "a checkout racing a write or chmod may supersede each op independently "
    "(`contracts.checkout._supersede_groups`)"
)


FUSE_FILE = {"mtime": 1.0, "mtime_history": 1.0, "size": 64, "fuse_type": "file"}


def racing(
    name: str,
    description: str,
    *,
    mode: str,
    revision: int,
    ops: tuple[str, ...],
    conflicts: int,
    data: bytes,
    executable: bool,
) -> Run:
    """A Thrift checkout races `ops` on r0; then a read and a getattr of r0."""
    run = Run("checkout", name, description, basis=[SUPERSEDE_PER_OP])
    run.act("a", "c", "checkout", revision=revision, via="thrift", mode=mode)
    for op in ops:
        if op == "write":
            run.act("b", "w", "write", path=R0, value_id="w")
        else:
            run.act("m", "m", "chmod", path=R0, mode=0o755)
    ids = ["w" if op == "write" else "m" for op in ops]
    after = [("c", "finished"), *((op, "finished") for op in ids)]
    run.act("o", "r", "read", path=R0, channel="fuse", requires=after)
    run.act("o", "g", "getattr", path=R0, requires=after)
    run.inv("c", *ids)
    for op in ids:
        if op == "w":
            run.done("w", "ok", **WRITE_OK)
        else:
            run.done("m", "ok", path=R0, mode=0o755)
    run.done("c", "ok", **thrift_checkout(revision, conflicts=conflicts))
    run.call("r", "ok", **read_ok(data))
    mode_bits = 0o755 if executable else 0o644
    run.call("g", "ok", **FUSE_FILE, mode=mode_bits, thrift_type="file")
    run.finish()
    return run


def _racing_runs() -> list[Run]:
    runs = []
    run = racing(
        "force-loses-write-on-kept-path",
        "A write to r0 overlaps FORCE to the current revision; FORCE reports no "
        "conflict and r0 reads its committed bytes: the write's open resolved the "
        "inode FORCE replaced, so its bytes landed in the unlinked inode (FORCE "
        "replaces every loaded path, changed or kept).",
        mode="force",
        revision=TIP,
        ops=("write",),
        conflicts=0,
        data=committed(R0, TIP),
        executable=False,
    )
    stale_read(run, "r", R0, SATISFIED, "c supersedes w after its visit of r0")
    runs.append(run)
    run = racing(
        "force-counts-write-yet-keeps-it",
        "A write to r0 overlaps FORCE to the current revision; FORCE reports one "
        "conflict yet r0 reads the write's bytes.",
        mode="force",
        revision=TIP,
        ops=("write",),
        conflicts=1,
        data=value("b", "w"),
        executable=False,
    )
    _conflicts(run, "c", VIOLATED, "a counted write landed before FORCE reset r0")
    stale_read(run, "r", R0, VIOLATED, "a counted write landed before FORCE reset r0")
    runs.append(run)
    return runs


def _overlap(
    name: str,
    description: str,
    *,
    overlapping: bool,
    observed: int,
    basis: tuple[str, ...] = (),
) -> Run:
    run = Run(
        "checkout",
        name,
        description,
        basis=[CHECKOUT_GUARD, *basis],
    )
    if overlapping:
        run.act(
            "nemesis",
            "f",
            "fault_install",
            fault_id="F",
            site="checkout-begin",
            target_action="c1",
            behavior="block",
        )
        run.act(
            "nemesis", "h", "fault_await", fault_id="F", requires=[("c1", "started")]
        )
        run.act(
            "nemesis", "x", "fault_remove", fault_id="F", requires=[("c2", "finished")]
        )
        run.act(
            "a",
            "c1",
            "checkout",
            revision=2,
            via="thrift",
            mode="normal",
            requires=[("f", "finished")],
            deadline="run",
        )
        run.act(
            "b",
            "c2",
            "checkout",
            revision=1,
            via="thrift",
            mode="normal",
            requires=[("h", "finished")],
        )
        run.act(
            "o",
            "r",
            "read",
            path=R0,
            channel="fuse",
            requires=[("c1", "finished"), ("c2", "finished")],
        )
        run.call("f", "ok", fault_id="F")
        run.inv("c1")
        run.call("h", "ok", fault_id="F", hit=True)
        run.call("c2", "fail", **thrift_rejected())
        run.call("x", "ok", fault_id="F", removed=1)
        run.done("c1", "ok", **thrift_checkout(2, conflicts=0))
    else:
        run.act("a", "c1", "checkout", revision=2, via="thrift", mode="normal")
        run.act("a", "c2", "checkout", revision=1, via="thrift", mode="normal")
        run.act("a", "r", "read", path=R0, channel="fuse")
        run.call("c1", "ok", **thrift_checkout(2, conflicts=0))
        run.call("c2", "fail", **thrift_rejected())
    run.call("r", "ok", **read_ok(committed(R0, observed)))
    if overlapping:
        run.fault_removed("F")
    run.finish()
    inputs_ok(run, "r")
    return run


def _overlap_runs() -> list[Run]:
    run = _overlap(
        "overlapping-checkouts-one-rejected",
        "c2 overlaps the parked c1 and is "
        "rejected CHECKOUT_IN_PROGRESS; c1 applies rev2.",
        overlapping=True,
        observed=2,
    )
    stale_read(run, "r", R0, SATISFIED, "c1 produced rev2's bytes")
    run2 = _overlap(
        "checkout-rejected-without-overlap",
        "c2 is rejected CHECKOUT_IN_PROGRESS although c1 finished before c2 started.",
        overlapping=False,
        observed=2,
        basis=(
            "INFERRED: illegal only if EdenFS resets its guard before c1's Thrift reply. "
            "EdenFS resets it in CheckoutInProgressGuard::finish or the destructor "
            "(EdenMount.cpp:1648-1716); a back-to-back checkout succeeds live "
            "(backend_recheckout_same), but the reset-versus-reply order is unknown.",
        ),
    )
    run2.expect(
        "checkout.in-progress-rejection",
        "c2",
        VIOLATED,
        "no other checkout's window overlaps c2",
        identity={"via": "thrift"},
    )
    stale_read(run2, "r", R0, SATISFIED, "c1 produced rev2's bytes")
    run3 = _overlap(
        "rejected-checkout-had-effects",
        "c2 overlaps c1 and is rejected, yet r0 later reads c2's rev1 bytes.",
        overlapping=True,
        observed=1,
        basis=(
            "a committed (path, revision) value is a solver observation, never "
            "fabricated",
        ),
    )
    stale_read(
        run3,
        "r",
        R0,
        VIOLATED,
        "rev1's r0 bytes are a committed value, so not fabricated; only the rejected "
        "c2 could produce them",
    )
    run3.expect(
        "checkout.in-progress-rejection",
        "c2",
        VIOLATED,
        "the rejected c2 has no effects, yet r's bytes need c2's r0 effect: one core "
        "with r's observation",
        identity={"via": "thrift"},
    )
    return [run, run2, run3]


INTERRUPTED = {
    "reason": "std::runtime_error: interrupted",
    "source": "thrift",
    "error_type": "GENERIC_ERROR",
}


def _parent_order(
    name: str, description: str, *, revision: int, failure: dict[str, str]
) -> Run:
    run = Run(
        "checkout",
        name,
        description,
        basis=[
            "a checkout moves the EdenFS parent before any path",
            "getScmStatusV2(R) fails OUT_OF_DATE_PARENT unless R is the EdenFS parent",
        ],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "s", "status", channel="thrift", revision=revision)
    run.call("c", "fail", **failure)
    entries = {} if revision == 2 else {R0: "M"}
    run.call("s", "ok", **thrift_status(entries, revision))
    run.finish()
    return run


def parent_runs() -> list[Run]:
    run = _parent_order(
        "parent-before-paths-applied",
        "A Thrift NORMAL to rev2 times out at the client; a later Thrift status for "
        "rev2 is clean: the daemon finished the checkout, parent first. (Had EdenFS "
        "reported the failure, the mount would be interrupted and the status "
        "refused: `CheckoutInProgressGuard::applyResetState`.)",
        revision=2,
        failure={"reason": "TTransportException: TIMED_OUT", "source": "thrift"},
    )
    omitted(run, "s", R0, SATISFIED, "c's r0 group followed its parent move")
    run2 = _parent_order(
        "parent-before-paths-violated",
        "A Thrift NORMAL to rev2 fails mid-walk; a later Thrift status for rev3 "
        "succeeds (so the parent is still rev3) yet lists r0 modified, which only c's "
        "r0 effect explains.",
        revision=3,
        failure=INTERRUPTED,
    )
    run2.expect(
        "checkout.parent-before-paths",
        "c",
        VIOLATED,
        "c's r0 effect requires the parent to be rev2 already; s sees rev3 after it",
        identity={"via": "thrift"},
    )
    status_rule(
        run2,
        THRIFT_MODEL,
        "s",
        R0,
        VIOLATED,
        "only c's r0 effect makes r0 differ from rev3, and it needs the parent moved",
        observed="M",
        producers="checkout",
    )
    return [run, run2]


def _client_timeout(name: str, description: str, *, first: int, second: int) -> Run:
    run = Run(
        "checkout",
        name,
        description,
        basis=[
            "a client Thrift timeout does not cancel the checkout; it completes later"
        ],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "r1", "read", path=R0, channel="fuse")
    run.act("a", "r2", "read", path=R0, channel="fuse")
    run.call("c", "fail", reason="TTransportException: TIMED_OUT", source="thrift")
    run.call("r1", "ok", **read_ok(committed(R0, first)))
    run.call("r2", "ok", **read_ok(committed(R0, second)))
    run.finish()
    return run


def stale_sapling_status() -> Run:
    run = Run(
        "checkout",
        "sl-status-rev-after-thrift-checkout",
        "A Thrift checkout moves EdenFS to rev2 and leaves Sapling's `.` at rev3; "
        "`sl status --rev 3` is then clean although the tree holds rev2.",
        basis=["a Thrift checkout leaves Sapling's `.` unchanged"],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "s", "status", channel="sl", revision=TIP)
    run.act("a", "r", "read", path=R0, channel="fuse")
    run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call("s", "ok", **sl_status({}, []))
    run.call("r", "ok", **read_ok(committed(R0, 2)))
    run.finish()
    stale_read(run, "r", R0, SATISFIED, "c produced rev2's bytes")
    return run


FORCE_UNCHANGED_DIRT = "FORCE reports a dirty path the destination leaves unchanged"


def _force_unchanged(name: str, description: str, *, conflicts: int) -> Run:
    """The churn file (unchanged by rev2) is written; FORCE to rev2; sl status."""
    run = Run(
        "checkout",
        name,
        description,
        basis=[THRIFT_NORMAL_TABLE + " (FORCE contrast)", FORCE_UNCHANGED_DIRT],
    )
    run.act("a", "w", "write", path=CHURN, value_id="w")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="force")
    run.act("a", "s", "status", channel="sl")
    run.call("w", "ok", **WRITE_OK)
    run.call("c", "ok", **thrift_checkout(2, conflicts=conflicts))
    run.call("s", "ok", **sl_status({}))
    run.finish()
    return run


def force_count_runs() -> list[Run]:
    counted = _force_unchanged(
        "force-counts-dirty-unchanged",
        "A churn file rev2 leaves unchanged is written; FORCE to rev2 reports one "
        "conflict.",
        conflicts=1,
    )
    counted.expect(
        "checkout.conflict-count",
        "c",
        SATISFIED,
        "FORCE types the dirty unchanged churn file MODIFIED_MODIFIED",
    )
    uncounted = _force_unchanged(
        "force-omits-dirty-unchanged",
        "A churn file rev2 leaves unchanged is written; FORCE to rev2 reports no "
        "conflict.",
        conflicts=0,
    )
    uncounted.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "the acked write left the churn file dirty at FORCE's visit",
        identity={"mode": "force"},
    )
    return [counted, uncounted]


def normal_runs() -> list[Run]:
    runs: list[Run] = []

    run = _normal(
        "normal-clean-changed-applied",
        "Thrift NORMAL to rev2 over a clean r0; the read returns rev2's bytes.",
        dirty=None,
        conflicts=0,
        observe="rev2",
    )
    _conflicts(run, "c", SATISFIED, "no path was dirty")
    runs.append(run)

    run = _normal(
        "normal-clean-changed-not-applied",
        "Thrift NORMAL to rev2 over a clean r0 acks, yet r0 still reads rev3's bytes.",
        dirty=None,
        conflicts=0,
        observe="rev3",
    )
    stale_read(
        run, "r", R0, VIOLATED, "the acked checkout produced rev2's r0 before the read"
    )
    runs.append(run)

    run = _normal(
        "normal-dirty-changed-kept",
        "r0 is written, then Thrift NORMAL to rev2 "
        "reports one conflict; r0 keeps the write (MODIFIED_MODIFIED).",
        dirty="r0",
        conflicts=1,
        observe="write",
    )
    _conflicts(run, "c", SATISFIED, "r0 was the one dirty changed path")
    runs.append(run)

    run = _normal(
        "normal-dirty-changed-overwritten",
        "r0 is written, then Thrift NORMAL to "
        "rev2 reports one conflict, yet r0 reads rev2's bytes.",
        dirty="r0",
        conflicts=1,
        observe="rev2",
    )
    stale_read(
        run, "r", R0, VIOLATED, "NORMAL never overwrites a path dirty other than to T"
    )
    runs.append(run)

    run = _normal(
        "normal-dirty-changed-no-conflict",
        "r0 is written and keeps the write "
        "across Thrift NORMAL to rev2, which reports zero conflicts.",
        dirty="r0",
        conflicts=0,
        observe="write",
    )
    _conflicts(run, "c", VIOLATED, "the kept dirty r0 is a MODIFIED_MODIFIED conflict")
    runs.append(run)

    run = _normal(
        "normal-dirty-unchanged-kept",
        "A churn file rev2 does not change is "
        "written; after Thrift NORMAL sl status still reports it modified.",
        dirty="churn",
        conflicts=0,
        observe="status-dirty",
    )
    _conflicts(run, "c", SATISFIED, "an unchanged path is never a conflict")
    runs.append(run)

    run = _normal(
        "normal-dirty-unchanged-reset",
        "A churn file rev2 does not change is "
        "written; after Thrift NORMAL sl status omits it.",
        dirty="churn",
        conflicts=0,
        observe="status-clean",
    )
    omitted(
        run,
        "s",
        CHURN,
        VIOLATED,
        "NORMAL changed a path T leaves unchanged",
        producers="checkout+write",
    )
    runs.append(run)

    run = _normal(
        "normal-removed-modified-stays-absent",
        "r0 is unlinked; Thrift NORMAL "
        "to rev2 reports REMOVED_MODIFIED and r0 stays absent.",
        dirty="unlink",
        conflicts=1,
        observe="enoent",
    )
    _conflicts(
        run, "c", SATISFIED, "the absent changed r0 is a REMOVED_MODIFIED conflict"
    )
    runs.append(run)

    run = _normal(
        "normal-removed-modified-recreated",
        "r0 is unlinked, yet after Thrift NORMAL to rev2 it reads rev2's bytes.",
        dirty="unlink",
        conflicts=1,
        observe="rev2",
    )
    stale_read(
        run, "r", R0, VIOLATED, "Thrift NORMAL never re-creates a locally absent path"
    )
    runs.append(run)
    return runs


def timeout_runs() -> list[Run]:
    run = _client_timeout(
        "client-timeout-completes-later",
        "A Thrift NORMAL to rev2 fails with a client transport timeout; r0 reads "
        "rev3, then rev2: the daemon finished the checkout later.",
        first=TIP,
        second=2,
    )
    stale_read(run, "r2", R0, SATISFIED, "c's r0 effect happened after r1")
    run2 = _client_timeout(
        "client-timeout-regresses",
        "The same timed-out checkout; r0 reads rev2, then rev3 again.",
        first=2,
        second=TIP,
    )
    for anchor in ("r1", "r2"):
        stale_read(
            run2,
            anchor,
            R0,
            VIOLATED,
            "nothing produces rev3's bytes after c's r0 effect, which r1 saw",
        )
    return [run, run2]


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "checkout",
        "listed-directory-does-not-widen-the-count-1",
        "A listing puts `tree/registers` in the contract set; it never conflicts, so a count one too high is still caught (Thrift NORMAL to rev2).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "l", "readdir", path="tree/registers", channel="fuse")
    run.call("w", "ok", **WRITE_OK)
    run.call("c", "ok", **thrift_checkout(2, conflicts=1))
    run.call("l", "ok", channel="fuse", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "listed-directory-does-not-widen-the-count-2",
        "A listing puts `tree/registers` in the contract set; it never conflicts, so a count one too high is still caught (Thrift NORMAL to rev2).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "l", "readdir", path="tree/registers", channel="fuse")
    run.call("w", "ok", **WRITE_OK)
    run.call("c", "ok", **thrift_checkout(2, conflicts=2))
    run.call("l", "ok", channel="fuse", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "listed-directory-does-not-widen-the-count-3",
        "A listing puts `tree/registers` in the contract set; it never conflicts, so a count one too high is still caught (Thrift NORMAL to rev2).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "l", "readdir", path="tree/registers", channel="fuse")
    run.call("w", "ok", **WRITE_OK)
    run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call("l", "ok", channel="fuse", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "racing-write-on-a-changed-path-is-ordered-by-the-engine-1",
        "R0, dirtied before the checkout, counts 1; a write racing the checkout on R1, which rev2 changes, adds 1 exactly when it lands before R1's visit (earlier R0 dirt, racing R1 write).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w0", "write", path=R0, value_id="w0")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w1", "write", path=R1, value_id="w1")
    run.call("w0", "ok", **WRITE_OK)
    run.inv("c", "w1")
    run.done("c", "ok", **thrift_checkout(2, conflicts=0))
    run.done("w1", "ok", **WRITE_OK)
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "racing-write-on-a-changed-path-is-ordered-by-the-engine-2",
        "R0, dirtied before the checkout, counts 1; a write racing the checkout on R1, which rev2 changes, adds 1 exactly when it lands before R1's visit (earlier R0 dirt, racing R1 write).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w0", "write", path=R0, value_id="w0")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w1", "write", path=R1, value_id="w1")
    run.call("w0", "ok", **WRITE_OK)
    run.inv("c", "w1")
    run.done("c", "ok", **thrift_checkout(2, conflicts=1))
    run.done("w1", "ok", **WRITE_OK)
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "racing-write-on-a-changed-path-is-ordered-by-the-engine-3",
        "R0, dirtied before the checkout, counts 1; a write racing the checkout on R1, which rev2 changes, adds 1 exactly when it lands before R1's visit (earlier R0 dirt, racing R1 write).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w0", "write", path=R0, value_id="w0")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w1", "write", path=R1, value_id="w1")
    run.call("w0", "ok", **WRITE_OK)
    run.inv("c", "w1")
    run.done("c", "ok", **thrift_checkout(2, conflicts=2))
    run.done("w1", "ok", **WRITE_OK)
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "racing-write-on-a-changed-path-is-ordered-by-the-engine-4",
        "R0, dirtied before the checkout, counts 1; a write racing the checkout on R1, which rev2 changes, adds 1 exactly when it lands before R1's visit (earlier R0 dirt, racing R1 write).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w0", "write", path=R0, value_id="w0")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w1", "write", path=R1, value_id="w1")
    run.call("w0", "ok", **WRITE_OK)
    run.inv("c", "w1")
    run.done("c", "ok", **thrift_checkout(2, conflicts=3))
    run.done("w1", "ok", **WRITE_OK)
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "conflict-and-an-overwritten-write-never-coexist-1",
        "The write lands before R0's visit (counted, kept) or after it (not counted; kept or superseded). A count of 1 with R0 at rev2 is a lost write (a write racing NORMAL, then a read).",
        basis=[
            "edenfs/properties/checkout.py: bytes.stale-read, checkout.conflict-count"
        ],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w", "write", path=R0, value_id="w")
    run.act(
        "o",
        "g",
        "read",
        requires=[("c", "finished"), ("w", "finished")],
        path=R0,
        channel="fuse",
    )
    run.inv("c", "w")
    run.done("w", "ok", **WRITE_OK)
    run.done("c", "ok", **thrift_checkout(2, conflicts=1))
    run.call("g", "ok", **read_ok(committed(R0, 2)))
    run.finish()
    run.expect(
        "bytes.stale-read",
        "g",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"channel": "fuse"},
    )
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "conflict-and-an-overwritten-write-never-coexist-2",
        "The write lands before R0's visit (counted, kept) or after it (not counted; kept or superseded). A count of 1 with R0 at rev2 is a lost write (a write racing NORMAL, then a read).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w", "write", path=R0, value_id="w")
    run.act(
        "o",
        "g",
        "read",
        requires=[("c", "finished"), ("w", "finished")],
        path=R0,
        channel="fuse",
    )
    run.inv("c", "w")
    run.done("w", "ok", **WRITE_OK)
    run.done("c", "ok", **thrift_checkout(2, conflicts=1))
    run.call("g", "ok", **read_ok(value("b", "w")))
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "conflict-and-an-overwritten-write-never-coexist-3",
        "The write lands before R0's visit (counted, kept) or after it (not counted; kept or superseded). A count of 1 with R0 at rev2 is a lost write (a write racing NORMAL, then a read).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w", "write", path=R0, value_id="w")
    run.act(
        "o",
        "g",
        "read",
        requires=[("c", "finished"), ("w", "finished")],
        path=R0,
        channel="fuse",
    )
    run.inv("c", "w")
    run.done("w", "ok", **WRITE_OK)
    run.done("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call("g", "ok", **read_ok(committed(R0, 2)))
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "conflict-and-an-overwritten-write-never-coexist-4",
        "The write lands before R0's visit (counted, kept) or after it (not counted; kept or superseded). A count of 1 with R0 at rev2 is a lost write (a write racing NORMAL, then a read).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w", "write", path=R0, value_id="w")
    run.act(
        "o",
        "g",
        "read",
        requires=[("c", "finished"), ("w", "finished")],
        path=R0,
        channel="fuse",
    )
    run.inv("c", "w")
    run.done("w", "ok", **WRITE_OK)
    run.done("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call("g", "ok", **read_ok(value("b", "w")))
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "write-on-a-path-the-destination-keeps-widens-nothing-1",
        "An overlapping write makes a path optional only where the destination changes it, so count 1 is caught (NORMAL to the current revision (seed-76)).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="normal")
    run.act("b", "w5", "write", path=R0, value_id="v5")
    run.act("c", "w2", "write", path=R1, value_id="v2")
    run.inv("c", "w5")
    run.done("c", "ok", **thrift_checkout(3, conflicts=1))
    run.done("w5", "ok", **WRITE_OK)
    run.call("w2", "ok", **WRITE_OK)
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "checkout/conflict-count: c reported 1 conflicts where no path can be in a conflict state",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "write-on-a-path-the-destination-keeps-widens-nothing-2",
        "An overlapping write makes a path optional only where the destination changes it, so count 1 is caught (NORMAL to the current revision (seed-76)).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="normal")
    run.act("b", "w5", "write", path=R0, value_id="v5")
    run.act("c", "w2", "write", path=R1, value_id="v2")
    run.inv("c", "w5")
    run.done("c", "ok", **thrift_checkout(3, conflicts=0))
    run.done("w5", "ok", **WRITE_OK)
    run.call("w2", "ok", **WRITE_OK)
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "write-on-a-path-the-destination-keeps-widens-nothing-3",
        "An overlapping write makes a path optional only where the destination changes it, so count 1 is caught (NORMAL to the current revision (seed-81)).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w4", "write", path=R1, value_id="v4")
    run.act("b", "c", "checkout", revision=3, via="thrift", mode="normal")
    run.act("c", "w0", "write", path=R1, value_id="v0")
    run.act("d", "w5", "write", path=R0, value_id="v5")
    run.inv("w4", "c")
    run.done("w4", "ok", **WRITE_OK)
    run.inv("w0")
    run.done("c", "ok", **thrift_checkout(3, conflicts=1))
    run.done("w0", "ok", **WRITE_OK)
    run.call("w5", "ok", **WRITE_OK)
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "checkout/conflict-count: c reported 1 conflicts where no path can be in a conflict state",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "write-on-a-path-the-destination-keeps-widens-nothing-4",
        "An overlapping write makes a path optional only where the destination changes it, so count 1 is caught (NORMAL to the current revision (seed-81)).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "w4", "write", path=R1, value_id="v4")
    run.act("b", "c", "checkout", revision=3, via="thrift", mode="normal")
    run.act("c", "w0", "write", path=R1, value_id="v0")
    run.act("d", "w5", "write", path=R0, value_id="v5")
    run.inv("w4", "c")
    run.done("w4", "ok", **WRITE_OK)
    run.inv("w0")
    run.done("c", "ok", **thrift_checkout(3, conflicts=0))
    run.done("w0", "ok", **WRITE_OK)
    run.call("w5", "ok", **WRITE_OK)
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "write-on-a-path-the-destination-keeps-widens-nothing-5",
        "An overlapping write makes a path optional only where the destination changes it, so count 1 is caught (NORMAL to the current revision (seed-271)).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="normal")
    run.act("b", "w0", "write", path=R0, value_id="v0")
    run.inv("c", "w0")
    run.done("c", "ok", **thrift_checkout(3, conflicts=1))
    run.done("w0", "ok", **WRITE_OK)
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "checkout/conflict-count: c reported 1 conflicts where no path can be in a conflict state",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "write-on-a-path-the-destination-keeps-widens-nothing-6",
        "An overlapping write makes a path optional only where the destination changes it, so count 1 is caught (NORMAL to the current revision (seed-271)).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="normal")
    run.act("b", "w0", "write", path=R0, value_id="v0")
    run.inv("c", "w0")
    run.done("c", "ok", **thrift_checkout(3, conflicts=0))
    run.done("w0", "ok", **WRITE_OK)
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "write-on-a-path-the-destination-keeps-widens-nothing-7",
        "An overlapping write makes a path optional only where the destination changes it, so count 1 is caught (NORMAL to the current revision (seed-25-c1)).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c0", "checkout", revision=3, via="thrift", mode="force")
    run.act("b", "c1", "checkout", revision=3, via="thrift", mode="normal")
    run.act("c", "w2", "write", path=R1, value_id="v2")
    run.inv("c0", "w2")
    run.done("c0", "ok", **thrift_checkout(3, conflicts=1))
    run.inv("c1")
    run.done("w2", "ok", **WRITE_OK)
    run.done("c1", "ok", **thrift_checkout(3, conflicts=1))
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c1",
        VIOLATED,
        "checkout/conflict-count: c1 reported 1 conflicts where no path can be in a conflict state",
        identity={"mode": "normal"},
    )
    found.append(run)

    run = Run(
        "checkout",
        "write-on-a-path-the-destination-keeps-widens-nothing-8",
        "An overlapping write makes a path optional only where the destination changes it, so count 1 is caught (NORMAL to the current revision (seed-25-c1)).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c0", "checkout", revision=3, via="thrift", mode="force")
    run.act("b", "c1", "checkout", revision=3, via="thrift", mode="normal")
    run.act("c", "w2", "write", path=R1, value_id="v2")
    run.inv("c0", "w2")
    run.done("c0", "ok", **thrift_checkout(3, conflicts=1))
    run.inv("c1")
    run.done("w2", "ok", **WRITE_OK)
    run.done("c1", "ok", **thrift_checkout(3, conflicts=0))
    run.finish()
    run.expect("checkout.conflict-count", "c1", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "force-counts-an-overlapping-write-on-a-kept-path-optionally-1",
        "FORCE reports a kept path once it differs, so a write racing a FORCE checkout to the current revision may count (FORCE to the current revision, racing write).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="force")
    run.act("b", "w", "write", path=R0, value_id="w")
    run.inv("c", "w")
    run.done("c", "ok", **thrift_checkout(3, conflicts=0))
    run.done("w", "ok", **WRITE_OK)
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "force-counts-an-overlapping-write-on-a-kept-path-optionally-2",
        "FORCE reports a kept path once it differs, so a write racing a FORCE checkout to the current revision may count (FORCE to the current revision, racing write).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="force")
    run.act("b", "w", "write", path=R0, value_id="w")
    run.inv("c", "w")
    run.done("c", "ok", **thrift_checkout(3, conflicts=1))
    run.done("w", "ok", **WRITE_OK)
    run.finish()
    run.expect("checkout.conflict-count", "c", SATISFIED, "no conflict count")
    found.append(run)

    run = Run(
        "checkout",
        "force-counts-an-overlapping-write-on-a-kept-path-optionally-3",
        "FORCE reports a kept path once it differs, so a write racing a FORCE checkout to the current revision may count (FORCE to the current revision, racing write).",
        basis=["edenfs/properties/checkout.py: checkout.conflict-count"],
    )
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="force")
    run.act("b", "w", "write", path=R0, value_id="w")
    run.inv("c", "w")
    run.done("c", "ok", **thrift_checkout(3, conflicts=2))
    run.done("w", "ok", **WRITE_OK)
    run.finish()
    run.expect(
        "checkout.conflict-count",
        "c",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        identity={"mode": "force"},
    )
    found.append(run)
    return found


def runs() -> list[Run]:
    return [
        *normal_runs(),
        *_force_runs(),
        *_racing_runs(),
        *_overlap_runs(),
        *parent_runs(),
        *timeout_runs(),
        stale_sapling_status(),
        *force_count_runs(),
        *collector_runs(),
    ]
