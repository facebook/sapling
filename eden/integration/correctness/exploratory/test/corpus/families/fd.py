# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `fd`: reads through descriptors held across writes, renames,
unlinks, and checkouts."""

from __future__ import annotations

import errno

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    NOT_EVALUATED,
    R0,
    R1,
    R2,
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
    getattr_fields,
    inputs_ok,
    UNDETERMINED,
)


ESTALE = 116


DESCRIPTOR = "descriptor:o"


def _held(name: str, description: str, *, mutate: str, result: str) -> Run:  # noqa: C901
    run = Run(
        "fd",
        name,
        description,
        basis=[
            "edenfs/properties/fd.py: a held descriptor reads the inode its open "
            "resolved",
            "a descriptor opened before a checkout keeps the old inode",
        ],
    )
    run.act("a", "o", "open", path=R0)
    if mutate == "write":
        run.act("a", "w", "write", path=R0, value_id="w")
    else:
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    if mutate == "write":
        run.call("w", "ok", **WRITE_OK)
    else:
        run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    if result == "estale":
        run.call("d", "fail", **errno_fail(ESTALE, "pread"))
    else:
        data = {
            "write": value("a", "w"),
            "committed": committed(R0, TIP),
            "torn": value("a", "w")[:32] + committed(R0, TIP)[32:],
            "fabricated": value("ghost", "x")[:40],
        }[result]
        run.call("d", "ok", **read_ok(data))
    run.call("e", "ok", fd=3)
    run.finish()
    return run


def fd_runs() -> list[Run]:
    run = _held(
        "held-read-across-checkout",
        "r0 is opened, a Thrift checkout to rev2 replaces the path, and the held "
        "descriptor still reads rev3's bytes.",
        mutate="checkout",
        result="committed",
    )
    for rule, reason in (
        ("fd.torn-read", "one whole committed value"),
        ("fd.fabricated-value", "the opened inode's committed value"),
    ):
        run.expect(rule, "d", SATISFIED, reason, subject=DESCRIPTOR)
    run2 = _held(
        "held-read-misses-write",
        "r0 is opened, an acked write through the path changes the inode, and the "
        "held descriptor reads rev3's bytes.",
        mutate="write",
        result="committed",
    )
    run2.expect(
        "fd.stale-read",
        "d",
        VIOLATED,
        "w wrote the opened inode before d started",
        subject=DESCRIPTOR,
    )
    run3 = _held(
        "held-read-fails-estale",
        "r0 is opened, a Thrift checkout to rev2 replaces the path, and the held "
        "descriptor's read fails ESTALE.",
        mutate="checkout",
        result="estale",
    )
    run3.expect(
        "fd.read-failed",
        "d",
        VIOLATED,
        "a descriptor held across a checkout never fails ESTALE",
        subject=DESCRIPTOR,
        identity={"errno": "ESTALE"},
        attribution=UNDETERMINED,
    )
    run3.bad_input("d", "ESTALE", "read_fd declares no ESTALE and no nemesis caused it")
    run4 = _held(
        "held-read-torn",
        "r0 is opened and written; the held descriptor returns the write's first 32 "
        "bytes followed by rev3's last 32.",
        mutate="write",
        result="torn",
    )
    run4.expect(
        "fd.torn-read",
        "d",
        VIOLATED,
        "the bytes splice two values of the opened inode",
        subject=DESCRIPTOR,
        attribution=UNDETERMINED,
    )
    run5 = _held(
        "held-read-fabricated",
        "r0 is opened and written; the held descriptor returns 40 bytes no action "
        "wrote.",
        mutate="write",
        result="fabricated",
    )
    run5.expect(
        "fd.fabricated-value",
        "d",
        VIOLATED,
        "no candidate producer of the opened inode yields these bytes",
        subject=DESCRIPTOR,
        attribution=UNDETERMINED,
    )
    return [run, run2, run3, run4, run5]


FD_BASIS = (
    "an open stays the descriptor identity across path "
    "replacement; read_fd sees the old file description, getattr the new type"
)


RENAME_BASIS = (
    "a rename moves the directory entry, inode number included, to its new "
    "name (`TreeInode::doRename`, eden/fs/inodes/TreeInode.cpp), so it "
    "transfers the object identity"
)


def _descriptor_stale(run: Run, outcome: str, reason: str) -> None:
    run.expect("fd.stale-read", "d", outcome, reason, subject=DESCRIPTOR)


def _follow(name: str, description: str, *, replace: bool, sees_write: bool) -> Run:
    """r0 is opened; a rename moves its object (or replaces it); a write follows."""
    run = Run("fd", name, description, basis=[FD_BASIS, RENAME_BASIS])
    run.act("a", "o", "open", path=R0)
    source, destination = (R1, R0) if replace else (R0, R1)
    run.act("a", "m", "rename", source=source, destination=destination)
    run.act("a", "w", "write", path=destination, value_id="w")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.call("m", "ok", source=source, destination=destination)
    run.call("w", "ok", **WRITE_OK)
    data = value("a", "w") if sees_write else committed(R0, TIP)
    run.call("d", "ok", **read_ok(data))
    run.call("e", "ok", fd=3)
    run.finish()
    return run


def follow_runs() -> list[Run]:
    moved = _follow(
        "held-object-follows-rename",
        "r0 is opened and renamed onto r1; a write through r1 reaches the held "
        "object, and read_fd returns it.",
        replace=False,
        sees_write=True,
    )
    _descriptor_stale(
        moved, SATISFIED, "the rename moved the object to r1, which w wrote"
    )
    missed = _follow(
        "held-object-misses-write-after-rename",
        "r0 is opened and renamed onto r1; a write through r1 reaches the held "
        "object, yet read_fd returns rev3's bytes.",
        replace=False,
        sees_write=False,
    )
    _descriptor_stale(missed, VIOLATED, "w wrote the moved object before d started")
    kept = _follow(
        "held-object-survives-replacement",
        "r0 is opened and r1 is renamed onto it; a write through r0 reaches r1's "
        "object, and read_fd still returns the opened object's rev3 bytes.",
        replace=True,
        sees_write=False,
    )
    _descriptor_stale(
        kept, SATISFIED, "the rename unbound the opened object from r0 before w"
    )
    leaked = _follow(
        "held-object-sees-replacement-write",
        "r0 is opened and r1 is renamed onto it; read_fd returns the bytes w wrote "
        "through r0 into r1's object.",
        replace=True,
        sees_write=True,
    )
    _descriptor_stale(
        leaked, VIOLATED, "w wrote the object the rename put at r0, not the open's"
    )
    return [moved, missed, kept, leaked]


def _type_change(name: str, description: str, *, data: bytes) -> Run:
    """r2 is a file at rev3 and a directory at rev2; it is opened, then checked out."""
    run = Run(
        "fd",
        name,
        description,
        basis=[FD_BASIS],
        type_changes=[{"path": R2, "at_revision": 1}, {"path": R2, "at_revision": 3}],
    )
    run.act("a", "o", "open", path=R2)
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "g", "getattr", path=R2)
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call(
        "g",
        "ok",
        **getattr_fields(size=0, fuse_type="directory", thrift_type="directory"),
    )
    run.call("d", "ok", **read_ok(data))
    run.call("e", "ok", fd=3)
    run.finish()
    return run


def type_change_runs() -> list[Run]:
    legal = _type_change(
        "held-object-across-type-change",
        "r2 is opened as a file; a checkout makes it a directory; getattr sees the "
        "directory and read_fd still returns the file's rev3 bytes.",
        data=committed(R2, TIP),
    )
    legal.expect(
        "attributes.type-disagreement",
        "g",
        SATISFIED,
        "both channels see the new directory",
        path=R2,
    )
    legal.expect(
        "checkout.conflict-count",
        "c",
        NOT_EVALUATED,
        "conflict typing for a path the checkout retypes is not modeled",
        identity={"mode": "normal"},
        not_evaluated_reason="checkout.conflict-count-unmodeled",
    )
    return [legal]


def superseded_runs() -> list[Run]:
    """A NORMAL checkout overlaps a write through the held path."""
    run = Run(
        "fd",
        "held-object-superseded-write",
        "r0 is opened; a write through r0 overlaps a Thrift NORMAL checkout that "
        "changes it. Whether the write lands in the replaced inode the descriptor "
        "holds is not modeled.",
        basis=[FD_BASIS],
    )
    run.act("a", "o", "open", path=R0)
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("b", "w", "write", path=R0, value_id="w", requires=[("o", "finished")])
    run.act("a", "d", "read_fd", fd="o", requires=[("w", "finished")])
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.inv("c", "w")
    run.done("w", "ok", **WRITE_OK)
    run.done("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call("d", "ok", **read_ok(committed(R0, TIP)))
    run.call("e", "ok", fd=3)
    run.finish()
    inputs_ok(run, "o", "w", "d")
    run.expect(
        "fd.stale-read",
        "d",
        NOT_EVALUATED,
        "the write may land in the replaced inode",
        subject=DESCRIPTOR,
        not_evaluated_reason="fd.unmodeled-object",
    )
    return [run]


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "fd",
        "unlink-unbinds-but-keeps-the-object",
        "Unlink unbinds but keeps the object.",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "o", "open", path=R0)
    run.act("a", "u", "unlink", path=R0)
    run.act("a", "d", "read_fd", fd="o")
    run.call("o", "ok", fd=3)
    run.call("u", "ok", path=R0)
    run.call("d", "ok", **read_ok(committed(R0, 3)))
    run.finish()
    for rule, reason in (
        ("fd.fabricated-value", "no fabricated value"),
        ("fd.read-failed", "no read failed"),
        ("fd.stale-read", "no stale read"),
        ("fd.torn-read", "no torn read"),
    ):
        run.expect(rule, "d", SATISFIED, reason, subject="descriptor:o")
    found.append(run)

    run = Run(
        "fd",
        "write-before-open-is-the-object-content",
        "Write before open is the object content.",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "o", "open", path=R0)
    run.act("a", "d", "read_fd", fd="o")
    run.call("w", "ok", **WRITE_OK)
    run.call("o", "ok", fd=3)
    run.call("d", "ok", **read_ok(committed(R0, 3)))
    run.finish()
    run.expect(
        "fd.stale-read",
        "d",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        subject="descriptor:o",
        identity={},
    )
    found.append(run)

    run = Run(
        "fd",
        "read-fd-through-a-failed-open-is-not-judged",
        "A read fd through a failed open is not judged.",
        basis=["core input legality: the contract observation of a failed input"],
    )
    run.act("a", "o", "open", path=R0)
    run.act("a", "d", "read_fd", fd="o")
    run.call("o", "fail", **errno_fail(errno.ENOENT))
    run.call("d", "fail", **errno_fail(errno.EBADF))
    run.finish()
    run.expect(
        "core.input-legality",
        "o",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"errno": "ENOENT", "input-kind": "open"},
    )
    found.append(run)

    run = Run(
        "fd",
        "normal-to-the-current-revision-keeps-the-object-1",
        "Normal to the current revision keeps the object (a held descriptor across a checkout).",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "o", "open", path=R0)
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="normal")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.call("c", "ok", **thrift_checkout(3, conflicts=0))
    run.call("w", "ok", **WRITE_OK)
    run.call("d", "ok", **read_ok(value("a", "w")))
    run.call("e", "ok", fd=3)
    run.finish()
    for rule, reason in (
        ("fd.fabricated-value", "no fabricated value"),
        ("fd.read-failed", "no read failed"),
        ("fd.stale-read", "no stale read"),
        ("fd.torn-read", "no torn read"),
    ):
        run.expect(rule, "d", SATISFIED, reason, subject="descriptor:o")
    found.append(run)

    run = Run(
        "fd",
        "normal-to-the-current-revision-keeps-the-object-2",
        "Normal to the current revision keeps the object (a held descriptor across a checkout).",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "o", "open", path=R0)
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="normal")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.call("c", "ok", **thrift_checkout(3, conflicts=0))
    run.call("w", "ok", **WRITE_OK)
    run.call("d", "ok", **read_ok(committed(R0, 3)))
    run.call("e", "ok", fd=3)
    run.finish()
    run.expect(
        "fd.stale-read",
        "d",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        subject="descriptor:o",
        identity={},
    )
    found.append(run)

    run = Run(
        "fd",
        "force-changing-the-path-detaches-the-object-1",
        "Force changing the path detaches the object (a held descriptor across a checkout).",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "o", "open", path=R0)
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="force")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call("w", "ok", **WRITE_OK)
    run.call("d", "ok", **read_ok(value("a", "w")))
    run.call("e", "ok", fd=3)
    run.finish()
    run.expect(
        "fd.stale-read",
        "d",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        subject="descriptor:o",
        identity={},
    )
    found.append(run)

    run = Run(
        "fd",
        "force-changing-the-path-detaches-the-object-2",
        "Force changing the path detaches the object (a held descriptor across a checkout).",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "o", "open", path=R0)
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="force")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call("w", "ok", **WRITE_OK)
    run.call("d", "ok", **read_ok(committed(R0, 3)))
    run.call("e", "ok", fd=3)
    run.finish()
    for rule, reason in (
        ("fd.fabricated-value", "no fabricated value"),
        ("fd.read-failed", "no read failed"),
        ("fd.stale-read", "no stale read"),
        ("fd.torn-read", "no torn read"),
    ):
        run.expect(rule, "d", SATISFIED, reason, subject="descriptor:o")
    found.append(run)

    run = Run(
        "fd",
        "force-resetting-dirt-detaches-the-object-1",
        "Force resetting dirt detaches the object (a held descriptor across a checkout).",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "v", "write", path=R0, value_id="v")
    run.act("a", "o", "open", path=R0)
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="force")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("v", "ok", **WRITE_OK)
    run.call("o", "ok", fd=3)
    run.call("c", "ok", **thrift_checkout(3, conflicts=1))
    run.call("w", "ok", **WRITE_OK)
    run.call("d", "ok", **read_ok(value("a", "w")))
    run.call("e", "ok", fd=3)
    run.finish()
    run.expect(
        "fd.stale-read",
        "d",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        subject="descriptor:o",
        identity={},
    )
    found.append(run)

    run = Run(
        "fd",
        "force-resetting-dirt-detaches-the-object-2",
        "Force resetting dirt detaches the object (a held descriptor across a checkout).",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "v", "write", path=R0, value_id="v")
    run.act("a", "o", "open", path=R0)
    run.act("a", "c", "checkout", revision=3, via="thrift", mode="force")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("v", "ok", **WRITE_OK)
    run.call("o", "ok", fd=3)
    run.call("c", "ok", **thrift_checkout(3, conflicts=1))
    run.call("w", "ok", **WRITE_OK)
    run.call("d", "ok", **read_ok(value("a", "v")))
    run.call("e", "ok", fd=3)
    run.finish()
    for rule, reason in (
        ("fd.fabricated-value", "no fabricated value"),
        ("fd.read-failed", "no read failed"),
        ("fd.stale-read", "no stale read"),
        ("fd.torn-read", "no torn read"),
    ):
        run.expect(rule, "d", SATISFIED, reason, subject="descriptor:o")
    found.append(run)

    run = Run(
        "fd",
        "checkout-that-keeps-the-path-opens-no-gap",
        "A checkout that keeps the path opens no gap.",
        basis=[
            "edenfs/properties/fd.py: fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
    )
    run.act("a", "o", "open", path=R0)
    run.act(
        "b",
        "c",
        "checkout",
        requires=[("o", "finished")],
        revision=3,
        via="thrift",
        mode="normal",
    )
    run.act("c", "w", "write", requires=[("o", "finished")], path=R0, value_id="w")
    run.act(
        "a", "d", "read_fd", requires=[("c", "finished"), ("w", "finished")], fd="o"
    )
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.inv("c", "w")
    run.done("w", "ok", **WRITE_OK)
    run.done("c", "ok", **thrift_checkout(3, conflicts=0))
    run.call("d", "ok", **read_ok(committed(R0, 3)))
    run.call("e", "ok", fd=3)
    run.finish()
    run.expect(
        "fd.stale-read",
        "d",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        subject="descriptor:o",
        identity={},
    )
    found.append(run)

    run = Run(
        "fd",
        "unordered-write-through-the-moved-object-is-refused",
        "An unordered write through the moved object is refused.",
        basis=[
            "edenfs/properties/fd.py: differential.value-disagreement, fd.fabricated-value, fd.read-failed, fd.stale-read, fd.torn-read"
        ],
        reference=True,
    )
    run.act("a", "o", "open", path=R0)
    run.act("a", "m", "rename", source=R0, destination=R1)
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.act(
        "b",
        "w",
        "write",
        requires=[("m", "finished"), ("e", "finished")],
        path=R1,
        value_id="w",
    )
    run.inv("o")
    run.tick(2)
    run.done(
        "o",
        "ok",
        reference_result={
            "schema_version": 1,
            "outcome": "ok",
            "started": 1.2,
            "completed": 1.3,
            "facts": {"fd": 3},
        },
        fd=3,
    )
    run.inv("m")
    run.tick(2)
    run.done(
        "m",
        "ok",
        reference_result={
            "schema_version": 1,
            "outcome": "ok",
            "started": 1.6,
            "completed": 1.7,
            "facts": {"source": R0, "destination": R1},
        },
        source=R0,
        destination=R1,
    )
    run.inv("d")
    run.tick(2)
    run.done(
        "d",
        "ok",
        reference_result={
            "schema_version": 1,
            "outcome": "ok",
            "started": 2.0,
            "completed": 2.1,
            "facts": {
                "bytes_prefix": "5245473120736565643d3030303430343034207772697465723d636f6d6d69743320616374696f6e3d616462626231346461666336313032622e2e2e2e2e2e2e",
                "length": 64,
                "sha256": "d74873eaa0dab90ec0cd324e50e0ef57d8583117df1f9eddea2f6a5cc608b116",
            },
        },
        bytes_prefix="5245473120736565643d3030303430343034207772697465723d636f6d6d69743320616374696f6e3d616462626231346461666336313032622e2e2e2e2e2e2e",
        length=64,
        sha256="d74873eaa0dab90ec0cd324e50e0ef57d8583117df1f9eddea2f6a5cc608b116",
    )
    run.inv("e")
    run.tick(2)
    run.done(
        "e",
        "ok",
        reference_result={
            "schema_version": 1,
            "outcome": "ok",
            "started": 2.4,
            "completed": 2.5,
            "facts": {"fd": 3},
        },
        fd=3,
    )
    run.inv("w")
    run.tick(2)
    run.done(
        "w",
        "ok",
        reference_result={
            "schema_version": 1,
            "outcome": "ok",
            "started": 2.8,
            "completed": 2.9,
            "facts": {"bytes_written": 64},
        },
        bytes_written=64,
    )
    run.finish()
    run.expect(
        "differential.value-disagreement",
        "d",
        SATISFIED,
        "no value disagreement",
        path=R0,
    )
    found.append(run)
    return found


def runs() -> list[Run]:
    return [
        *fd_runs(),
        *follow_runs(),
        *type_change_runs(),
        *superseded_runs(),
        *collector_runs(),
    ]
