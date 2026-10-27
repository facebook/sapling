# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `existence`: presence and absence after creates, removes,
renames, and mkdirs."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    MOVED,
    NEW_DIR,
    NOT_EVALUATED,
    R0,
    R2,
    read_ok,
    Run,
    SATISFIED,
    thrift_checkout,
    TIP,
    VIOLATED,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    EISDIR,
    ENOENT,
    getattr_ok,
)


def existence_runs() -> list[Run]:
    basis = ["edenfs/properties/existence.py"]
    run = Run(
        "existence",
        "renamed-destination-present",
        "r0 is renamed to moved.reg; moved.reg is then present and r0 absent.",
        basis=basis,
    )
    run.act("a", "m", "rename", source=R0, destination=MOVED)
    run.act("a", "g", "getattr", path=MOVED)
    run.act("a", "h", "getattr", path=R0)
    run.call("m", "ok", source=R0, destination=MOVED)
    run.call("g", "ok", **getattr_ok(0o644))
    run.call("h", "fail", **errno_fail(ENOENT, "stat"))
    run.finish()
    run.expect(
        "existence.present-after-remove",
        "g",
        SATISFIED,
        "the rename created moved.reg",
        path=MOVED,
    )
    return [run]


def existence_absence_runs() -> list[Run]:
    basis = ["edenfs/properties/existence.py"]
    run = Run(
        "existence",
        "unlinked-path-absent",
        "r0 is unlinked; a later getattr fails ENOENT.",
        basis=basis,
    )
    run.act("a", "u", "unlink", path=R0)
    run.act("a", "g", "getattr", path=R0)
    run.call("u", "ok", path=R0)
    run.call("g", "fail", **errno_fail(ENOENT, "stat"))
    run.finish()
    run.expect(
        "existence.missing-after-create",
        "g",
        SATISFIED,
        "nothing re-created r0 after the unlink",
        path=R0,
    )
    run2 = Run(
        "existence",
        "created-directory-missing",
        "mkdir d succeeds; a later getattr of d fails ENOENT.",
        basis=basis,
    )
    run2.act("a", "m", "mkdir", path=NEW_DIR)
    run2.act("a", "g", "getattr", path=NEW_DIR)
    run2.call("m", "ok", path=NEW_DIR)
    run2.call("g", "fail", **errno_fail(ENOENT, "stat"))
    run2.finish()
    run2.expect(
        "existence.missing-after-create",
        "g",
        VIOLATED,
        "the acked mkdir created d before g started",
        path=NEW_DIR,
    )
    runs = [run, run2]
    for ok in (False, True):
        run = Run(
            "existence",
            "directory-read-succeeds" if ok else "directory-read-eisdir",
            "mkdir d succeeds; a later FUSE read of d "
            + ("returns r0's committed bytes." if ok else "fails EISDIR."),
            basis=basis,
        )
        run.act("a", "m", "mkdir", path=NEW_DIR)
        run.act("a", "r", "read", path=NEW_DIR, channel="fuse")
        run.call("m", "ok", path=NEW_DIR)
        if ok:
            run.call("r", "ok", **read_ok(committed(R0, TIP)))
        else:
            run.call("r", "fail", **errno_fail(EISDIR))
        run.finish()
        if ok:
            run.expect(
                "existence.directory-read",
                "r",
                VIOLATED,
                "d is a directory at every point of r",
                path=NEW_DIR,
                identity={"observed": "ok"},
            )
            run.expect(
                "bytes.stale-read",
                "r",
                VIOLATED,
                "d never holds file bytes; r0's committed value is a candidate of "
                "the run, so the engine judges it (never torn)",
                path=NEW_DIR,
                identity={"channel": "fuse"},
            )
        else:
            run.expect(
                "existence.directory-read",
                "r",
                SATISFIED,
                "the read of a directory failed EISDIR",
                path=NEW_DIR,
            )
        runs.append(run)
    return runs


EXISTENCE = "edenfs/properties/existence.py"


def retype_runs() -> list[Run]:
    """A checkout replacing a directory with a file unbinds the name before binding the file."""
    runs = []
    for during in (True, False):
        run = Run(
            "existence",
            f"retyped-file-missing-{'during' if during else 'after'}-checkout",
            "r2 is a directory at rev3 and a file at rev2. A getattr of r2 fails "
            "ENOENT "
            + (
                "while a Thrift NORMAL checkout to rev2 runs."
                if during
                else "after a Thrift NORMAL checkout to rev2 completed."
            ),
            basis=[
                EXISTENCE,
                "TreeInode::finalizeDirectoryRemoval removes the directory and "
                "inserts the file under two contents locks",
            ],
            type_changes=[{"path": R2, "at_revision": TIP}],
        )
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        if during:
            run.act("b", "g", "getattr", path=R2)
            run.inv("c")
            run.call("g", "fail", **errno_fail(ENOENT, "stat"))
            run.done("c", "ok", **thrift_checkout(2, conflicts=0))
        else:
            run.act("a", "g", "getattr", path=R2)
            run.call("c", "ok", **thrift_checkout(2, conflicts=0))
            run.call("g", "fail", **errno_fail(ENOENT, "stat"))
        run.finish()
        run.expect(
            "checkout.conflict-count",
            "c",
            NOT_EVALUATED,
            "conflict typing for a path the checkout retypes is not modeled",
            identity={"mode": "normal"},
            not_evaluated_reason="checkout.conflict-count-unmodeled",
        )
        if during:
            run.expect(
                "existence.missing-after-create",
                "g",
                SATISFIED,
                "the checkout may have removed the directory and not yet bound the file",
                path=R2,
            )
        else:
            run.expect(
                "existence.missing-after-create",
                "g",
                VIOLATED,
                "the acked checkout bound the file before g started",
                path=R2,
            )
        runs.append(run)
    return runs


def runs() -> list[Run]:
    return [
        *existence_runs(),
        *existence_absence_runs(),
        *retype_runs(),
    ]
