# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `rename`: renames overlapping writes and reads."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    MOVED,
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
    ENOENT,
    stale_read,
)


RENAME_WRITE_OVERLAP = (
    "a write overlapping a rename lands at the source or the destination"
)


def _rename(
    name: str,
    description: str,
    *,
    overlap: bool,
    write_ok: bool,
    dest_bytes: str,
    destination: str = MOVED,
) -> Run:
    run = Run("rename", name, description, basis=[RENAME_WRITE_OVERLAP])
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("b", "m", "rename", source=R0, destination=destination)
    run.act(
        "o",
        "r",
        "read",
        path=destination,
        channel="fuse",
        requires=[("w", "finished"), ("m", "finished")],
    )
    run.act("o", "g", "getattr", path=R0)
    if overlap:
        run.inv("w", "m")
        run.done("m", "ok", source=R0, destination=destination)
        run.done(
            "w",
            "ok" if write_ok else "fail",
            **(WRITE_OK if write_ok else errno_fail(ENOENT)),
        )
    elif write_ok:
        run.call("w", "ok", **WRITE_OK)
        run.call("m", "ok", source=R0, destination=destination)
    else:
        run.call("m", "ok", source=R0, destination=destination)
        run.call("w", "fail", **errno_fail(ENOENT))
    data = {
        "write": value("a", "w"),
        "source": committed(R0, TIP),
        "destination": committed(destination, TIP),
    }[dest_bytes]
    run.call("r", "ok", **read_ok(data))
    run.call("g", "fail", reason="getattr", errno=ENOENT)
    run.finish()
    return run


def write_then_rename() -> Run:
    run = _rename(
        "write-then-rename",
        "Write r0, then rename r0 to moved.reg; moved.reg "
        "holds the write's bytes and r0 is gone.",
        overlap=False,
        write_ok=True,
        dest_bytes="write",
    )
    stale_read(run, "r", MOVED, SATISFIED, "the rename transferred w's bytes")
    return run


def rename_then_write() -> Run:
    run = _rename(
        "rename-then-write-enoent",
        "Rename r0 away, then a write to r0 fails "
        "ENOENT; moved.reg holds r0's committed bytes.",
        overlap=False,
        write_ok=False,
        dest_bytes="source",
    )
    stale_read(run, "r", MOVED, SATISFIED, "the rename transferred rev3's r0 bytes")
    return run


def overlap_write_rename_new_bytes() -> Run:
    run = _rename(
        "overlap-write-rename-new-bytes",
        "An acked write to r0 overlaps the "
        "rename r0 -> moved.reg; moved.reg holds the write's bytes.",
        overlap=True,
        write_ok=True,
        dest_bytes="write",
    )
    run.expect(
        "namespace.precondition", "w", SATISFIED, "r0 is a file before m", path=R0
    )
    return run


def overlap_write_rename_old_bytes() -> Run:
    run = _rename(
        "overlap-write-rename-old-bytes",
        "An acked write to r0 overlaps the "
        "rename, yet moved.reg holds r0's committed bytes: the rename must "
        "precede the write, which then had no file to write.",
        overlap=True,
        write_ok=True,
        dest_bytes="source",
    )
    stale_read(run, "r", MOVED, VIOLATED, "rev3's bytes at moved.reg need Before(m, w)")
    run.expect(
        "namespace.precondition",
        "w",
        VIOLATED,
        "Before(m, w) leaves r0 absent, but an acked write needs a file",
        path=R0,
        identity={"input-kind": "write"},
    )
    run.expect(
        "existence.missing-after-create",
        "g",
        VIOLATED,
        "with the stale read the write lands on r0 after the rename, so g's "
        "absence completes a minimal core with that read",
        path=R0,
    )
    return run


def overlap_rename_then_write_enoent() -> Run:
    run = _rename(
        "overlap-rename-write-enoent",
        "A write to r0 overlapping the rename "
        "fails ENOENT; moved.reg holds r0's committed bytes.",
        overlap=True,
        write_ok=False,
        dest_bytes="source",
    )
    stale_read(run, "r", MOVED, SATISFIED, "the rename transferred rev3's r0 bytes")
    return run


def _onto_register(name: str, observed: int, description: str) -> Run:
    run = Run("rename", name, description, basis=[RENAME_WRITE_OVERLAP])
    run.act("a", "m", "rename", source=R0, destination=R1)
    run.act("a", "r", "read", path=R1, channel="fuse")
    run.call("m", "ok", source=R0, destination=R1)
    run.call("r", "ok", **read_ok(committed(R0 if observed == 0 else R1, TIP)))
    run.finish()
    return run


def rename_replaces_destination() -> Run:
    run = _onto_register(
        "rename-replaces-destination",
        0,
        "Rename r0 onto r1; r1 then holds r0's committed bytes.",
    )
    stale_read(run, "r", R1, SATISFIED, "the rename installed r0's binding at r1")
    return run


def rename_keeps_prior_destination() -> Run:
    run = _onto_register(
        "rename-keeps-prior-destination",
        1,
        "Rename r0 onto r1, yet r1 still returns its own prior bytes.",
    )
    stale_read(
        run, "r", R1, VIOLATED, "the rename atomically removed r1's prior binding"
    )
    return run


def rename_source_still_readable() -> Run:
    run = Run(
        "rename",
        "rename-source-still-readable",
        "After an acked rename of r0, a read of r0 still returns its bytes.",
        basis=[RENAME_WRITE_OVERLAP],
    )
    run.act("a", "m", "rename", source=R0, destination=MOVED)
    run.act("a", "r", "read", path=R0, channel="fuse")
    run.call("m", "ok", source=R0, destination=MOVED)
    run.call("r", "ok", **read_ok(committed(R0, TIP)))
    run.finish()
    stale_read(run, "r", R0, VIOLATED, "the rename removed r0 before the read")
    return run


def runs() -> list[Run]:
    return [
        write_then_rename(),
        rename_then_write(),
        overlap_write_rename_new_bytes(),
        overlap_write_rename_old_bytes(),
        overlap_rename_then_write_enoent(),
        rename_replaces_destination(),
        rename_keeps_prior_destination(),
        rename_source_still_readable(),
    ]
