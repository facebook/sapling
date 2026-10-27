# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `windows`: process, connection, fault, and timeout windows
around effects."""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    R0,
    read_ok,
    Run,
    SATISFIED,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    EIO,
    ENOENT,
    inputs_ok,
    stale_read,
)


ETIMEDOUT = 110


WINDOWS = (
    "an `info` or ETIMEDOUT result leaves the effect unresolved until the daemon "
    "stops (`contracts.checkout.effect_window`)"
)


def _info_write(name: str, description: str, *, read_first: bool) -> Run:
    run = Run("windows", name, description, basis=[WINDOWS])
    run.act(
        "a",
        "w",
        "write",
        path=R0,
        value_id="w",
        requires=[("r", "finished")] if read_first else (),
    )
    run.act(
        "o",
        "r",
        "read",
        path=R0,
        channel="fuse",
        requires=() if read_first else [("w", "started")],
    )
    if read_first:
        run.call("r", "ok", **read_ok(value("a", "w")))
        run.inv("w")
        run.wait(120)
        run.done("w", "info", reason="operation-timeout")
    else:
        run.inv("w", "r")
        run.done("r", "ok", **read_ok(value("a", "w")))
        run.wait(120)
        run.done("w", "info", reason="operation-timeout")
    run.finish()
    return run


def _timed_out(
    name: str,
    description: str,
    *,
    fault: bool,
    observed: str,
    basis: tuple[str, ...] = (),
) -> Run:
    run = Run(
        "windows", name, description, basis=[WINDOWS + " (FUSE ETIMEDOUT gap)", *basis]
    )
    if fault:
        run.act(
            "nemesis",
            "f",
            "fault_install",
            fault_id="F",
            site="overlay-write",
            target_action="w",
            behavior="block",
        )
        run.act(
            "nemesis", "h", "fault_await", fault_id="F", requires=[("w", "started")]
        )
        run.act(
            "nemesis", "x", "fault_remove", fault_id="F", requires=[("h", "finished")]
        )
        run.act(
            "a",
            "w",
            "write",
            path=R0,
            value_id="w",
            requires=[("f", "finished")],
            deadline="run",
        )
        run.act(
            "o",
            "r",
            "read",
            path=R0,
            channel="fuse",
            requires=[("x", "finished"), ("w", "finished")],
        )
        run.call("f", "ok", fault_id="F")
        run.inv("w")
        run.call("h", "ok", fault_id="F", hit=True)
    else:
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "r", "read", path=R0, channel="fuse")
        run.inv("w")
    run.wait(60)
    run.done("w", "fail", **errno_fail(ETIMEDOUT, "pwrite"))
    if fault:
        run.call("x", "ok", fault_id="F", removed=1)
    data = value("a", "w") if observed == "write" else committed(R0, TIP)
    run.call("r", "ok", **read_ok(data))
    if fault:
        run.fault_removed("F")
    run.finish()
    inputs_ok(run, "r")
    return run


def windows_runs() -> list[Run]:
    run = _info_write(
        "info-write-later-observed",
        "A write ends raw info "
        "(operation-timeout); a read overlapping it returns its bytes.",
        read_first=False,
    )
    stale_read(run, "r", R0, SATISFIED, "Happened(w) with its point inside r's window")
    run2 = _info_write(
        "info-write-observed-before-invoke",
        "A read returns a write's bytes "
        "and ends before that write, which later ends raw info, starts.",
        read_first=True,
    )
    stale_read(
        run2, "r", R0, VIOLATED, "w's resolution window opens after the read ends"
    )
    run3 = _timed_out(
        "etimedout-write-applied",
        "A write parked by a targeted fault fails "
        "ETIMEDOUT; a later read returns its bytes (the FUSE request may "
        "still apply after the timeout reply).",
        fault=True,
        observed="write",
        basis=(
            "INFERRED: FuseChannel.cpp:2466-2472 (`within()` does not cancel the "
            "request); late effects were observed live only after "
            "info{operation-timeout}, not after an ETIMEDOUT reply",
        ),
    )
    run3.legal_input("w", "ETIMEDOUT under a witnessed fault targeting w is excused")
    stale_read(
        run3, "r", R0, SATISFIED, "the ETIMEDOUT write is indeterminate and happened"
    )
    run4 = _timed_out(
        "etimedout-write-not-applied",
        "The same parked write fails ETIMEDOUT "
        "and a later read returns r0's committed bytes.",
        fault=True,
        observed="committed",
    )
    stale_read(
        run4,
        "r",
        R0,
        SATISFIED,
        "the ETIMEDOUT write is indeterminate and did not happen",
    )
    run5 = _timed_out(
        "etimedout-write-unexcused",
        "A write fails ETIMEDOUT with no fault targeting it.",
        fault=False,
        observed="committed",
    )
    run5.bad_input(
        "w", "ETIMEDOUT", "no nemesis targets w, so ETIMEDOUT is not excused"
    )
    return [run, run2, run3, run4, run5]


ENOTCONN = 107


ECONNABORTED = 103


KILLED = -9


def _killed_update(name: str, description: str, *, dirty: bool) -> Run:
    run = Run(
        "windows",
        name,
        description,
        basis=[
            "an sl process killed -9 is indeterminate: its daemon-side work may "
            "continue",
            WINDOWS,
        ],
    )
    if dirty:
        run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
    run.act(
        "nemesis",
        "k",
        "kill_process",
        target_action="c",
        after_ms=100,
        requires=[("c", "started")],
    )
    run.act(
        "o",
        "r",
        "read",
        path=R0,
        channel="fuse",
        requires=[("c", "finished"), ("k", "finished")],
    )
    if dirty:
        run.call("w", "ok", **WRITE_OK)
    run.inv("c")
    run.call("k", "ok", target_action="c", pgid=4242, group_empty=True)
    run.done(
        "c",
        "fail",
        reason="goto: killed by signal 9",
        returncode=KILLED,
        stdout="",
        stderr="",
    )
    run.call("r", "ok", **read_ok(committed(R0, 2)))
    run.finish()
    return run


def _killed_rm() -> Run:
    run = Run(
        "windows",
        "killed-sl-rm-applied",
        "sl rm of r0 is killed -9; a later read of r0 fails ENOENT: the removal may "
        "still have applied.",
        basis=["an sl process killed -9 is indeterminate", WINDOWS],
    )
    run.act("a", "m", "sl_rm", path=R0)
    run.act(
        "nemesis",
        "k",
        "kill_process",
        target_action="m",
        after_ms=100,
        requires=[("m", "started")],
    )
    run.act(
        "o",
        "r",
        "read",
        path=R0,
        channel="fuse",
        requires=[("m", "finished"), ("k", "finished")],
    )
    run.inv("m")
    run.call("k", "ok", target_action="m", pgid=4243, group_empty=True)
    run.done(
        "m",
        "fail",
        reason="sl_rm: killed by signal 9",
        returncode=KILLED,
        stdout="",
        stderr="",
    )
    run.call("r", "fail", **errno_fail(ENOENT))
    run.finish()
    run.expect("result-shape.channel-fields", "r", SATISFIED, "one FUSE variant")
    return run


def _lost_write(
    name: str,
    description: str,
    *,
    fault: bool,
    failure: dict[str, Any],
    observed: str,
    read_first: bool = False,
) -> Run:
    """A write fails with a connection or close error; it may still have applied."""
    run = Run(
        "windows",
        name,
        description,
        basis=[
            "a FUSE mutation failing ECONNABORTED, ENOTCONN, or with any close_errno "
            "is indeterminate",
            "a fault errno is legal only when a nemesis caused it",
        ],
    )
    if fault:
        run.act(
            "nemesis",
            "f",
            "fault_install",
            fault_id="F",
            site="overlay-write",
            target_action="w",
            behavior="error",
        )
    write_requires = [("f", "finished")] if fault else []
    if read_first:
        run.act("o", "r", "read", path=R0, channel="fuse")
        write_requires.append(("r", "finished"))
    run.act("a", "w", "write", path=R0, value_id="w", requires=write_requires)
    if fault:
        run.act(
            "nemesis", "x", "fault_remove", fault_id="F", requires=[("w", "finished")]
        )
    if not read_first:
        after = [("w", "finished")] + ([("x", "finished")] if fault else [])
        run.act("o", "r", "read", path=R0, channel="fuse", requires=after)
    data = value("a", "w") if observed == "write" else committed(R0, TIP)
    if fault:
        run.call("f", "ok", fault_id="F")
    if read_first:
        run.call("r", "ok", **read_ok(data))
    run.call("w", "fail", **failure)
    if fault:
        run.call("x", "ok", fault_id="F", removed=1)
    if not read_first:
        run.call("r", "ok", **read_ok(data))
    if fault:
        run.fault_removed("F")
    run.finish()
    if fault:
        run.legal_input("w", "a fault targeting w caused its failure")
    return run


def window_runs() -> list[Run]:
    run = _killed_update(
        "killed-sl-goto-applies",
        "sl goto to rev2 is killed -9; the clean r0 then reads rev2's bytes.",
        dirty=False,
    )
    stale_read(run, "r", R0, SATISFIED, "the daemon may have finished c's r0 effect")
    run2 = _killed_update(
        "killed-sl-goto-overwrites-dirty",
        "r0 is written; sl goto to rev2 is killed -9; r0 then reads rev2's bytes.",
        dirty=True,
    )
    stale_read(
        run2, "r", R0, VIOLATED, "no checkout path overwrites a dirty changed path"
    )
    run3 = _killed_rm()
    lost = errno_fail(ECONNABORTED, "pwrite")
    run4 = _lost_write(
        "econnaborted-write-applied",
        "A write under a targeted fault fails ECONNABORTED; a later read returns its "
        "bytes.",
        fault=True,
        failure=lost,
        observed="write",
    )
    stale_read(run4, "r", R0, SATISFIED, "the indeterminate write happened")
    run5 = _lost_write(
        "enotconn-write-unexcused",
        "A write fails ENOTCONN with no nemesis; a later read returns r0's committed "
        "bytes.",
        fault=False,
        failure=errno_fail(ENOTCONN, "pwrite"),
        observed="committed",
    )
    run5.bad_input(
        "w", "ENOTCONN", "ENOTCONN is a fault errno and no nemesis caused it"
    )
    close = {
        "reason": "close: [Errno 107] Transport endpoint is not connected",
        "errno": ENOTCONN,
        "close_errno": ENOTCONN,
        "bytes_written": 64,
    }
    run6 = _lost_write(
        "close-error-write-applied",
        "A write's close fails ENOTCONN under a targeted fault; a later read returns "
        "its bytes.",
        fault=True,
        failure=close,
        observed="write",
    )
    stale_read(
        run6, "r", R0, SATISFIED, "a failed close leaves the write indeterminate"
    )
    run7 = _lost_write(
        "close-error-write-observed-before-invoke",
        "A read returns a write's bytes and ends before that write, whose close later "
        "fails ENOTCONN, starts.",
        fault=True,
        failure=close,
        observed="write",
        read_first=True,
    )
    stale_read(
        run7, "r", R0, VIOLATED, "w's indeterminate window opens after the read ends"
    )
    return [run, run2, run3, run4, run5, run6, run7]


POST_EFFECT_SITE = (
    "an error injected at journal-record fails a write after its bytes landed; "
    "one at overlay-write fails it before (`edenfs.sites`)"
)


def _faulted_write(name: str, description: str, *, site: str) -> Run:
    run = Run("windows", name, description, basis=[POST_EFFECT_SITE])
    run.act(
        "nemesis",
        "f",
        "fault_install",
        fault_id="F",
        site=site,
        target_action="w",
        behavior="error",
    )
    run.act("a", "w", "write", path=R0, value_id="w", requires=[("f", "finished")])
    run.act("nemesis", "x", "fault_remove", fault_id="F", requires=[("w", "finished")])
    run.act(
        "o",
        "r",
        "read",
        path=R0,
        channel="fuse",
        requires=[("w", "finished"), ("x", "finished")],
    )
    run.call("f", "ok", fault_id="F")
    run.call("w", "fail", **errno_fail(EIO, "pwrite"))
    run.call("x", "ok", fault_id="F", removed=1)
    run.call("r", "ok", **read_ok(value("a", "w")))
    run.fault_removed("F")
    run.finish()
    return run


def site_runs() -> list[Run]:
    runs = []
    run = _faulted_write(
        "journal-record-error-landed",
        "An error injected at journal-record fails a write EIO; a later read "
        "returns the write's bytes.",
        site="journal-record",
    )
    stale_read(
        run,
        "r",
        R0,
        SATISFIED,
        "journal-record fires after the bytes are in the overlay",
    )
    runs.append(run)

    run = _faulted_write(
        "overlay-write-error-landed",
        "An error injected at overlay-write fails a write EIO; a later read "
        "returns the write's bytes.",
        site="overlay-write",
    )
    stale_read(run, "r", R0, VIOLATED, "overlay-write fires before any byte is written")
    runs.append(run)
    return runs


def runs() -> list[Run]:
    return [
        *windows_runs(),
        *window_runs(),
        *site_runs(),
    ]
