# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `attributes`: getattr's mode, size, type, and mtime against
the path's cells."""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    NEW_DIR,
    R0,
    R1,
    Run,
    SATISFIED,
    thrift_checkout,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.checkout_thrift import (
    racing,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    attrs,
    FUSE_ATTRS,
    getattr_fields,
    getattr_ok,
    UNDETERMINED,
)
from eden.integration.correctness.exploratory.test.corpus.families.existence import (
    EXISTENCE,
)


def _chmod_stat(
    name: str, description: str, *, mode: int, thrift_type: str = "file"
) -> Run:
    run = Run(
        "attributes",
        name,
        description,
        basis=[
            "edenfs/properties/attributes.py; `FuseDispatcherImpl::setattr` "
            "(eden/fs/inodes/FuseDispatcherImpl.cpp)"
        ],
    )
    run.act("a", "m", "chmod", path=R0, mode=0o755)
    run.act("a", "g", "getattr", path=R0)
    run.call("m", "ok", path=R0, mode=0o755)
    run.call("g", "ok", **getattr_ok(mode, thrift_type))
    run.finish()
    return run


def attribute_runs() -> list[Run]:
    run = _chmod_stat(
        "chmod-mode-visible",
        "r0 is made executable; a later getattr reports mode 0755 and one file type.",
        mode=0o755,
    )
    run.expect(
        "attributes.type-disagreement", "g", SATISFIED, "both report a file", path=R0
    )
    run2 = _chmod_stat(
        "chmod-mode-reverted",
        "r0 is made executable; a later getattr reports mode 0644.",
        mode=0o644,
    )
    run2.expect(
        "attributes.mode-revert",
        "g",
        VIOLATED,
        "the acked chmod set 0755 before g started",
        path=R0,
    )
    run3 = _chmod_stat(
        "fuse-thrift-type-disagree",
        "r0 is made executable; getattr reports a file over FUSE and a directory via "
        "Thrift.",
        mode=0o755,
        thrift_type="directory",
    )
    run3.expect(
        "attributes.type-disagreement",
        "g",
        VIOLATED,
        "both resolve the path against the same mount state (FUSE inode lookup; "
        "Thrift `co_getVirtualInode`)",
        path=R0,
        identity={"fuse-type": "file", "thrift-type": "directory"},
    )
    return [run, run2, run3]


ATTRIBUTES = "edenfs/properties/attributes.py"


def _chmod_after_observed_reset(kept: bool) -> Run:
    """Two equal chmods around a FORCE reset, so a content key would collide."""
    run = Run(
        "attributes",
        f"force-checkout-chmod-after-observed-reset-{'kept' if kept else 'lost'}",
        "chmod m0 makes r0 mode-dirty; chmod `a` (0755) overlaps a FORCE checkout to "
        "rev2. getattr g1 reports 0644, so the reset already replaced r0. chmod `b` "
        "(0755 again) starts after g1, so it reaches the new inode and g2 after "
        "everything must report 0755."
        + ("" if kept else " Here g2 reports 0644: b is lost."),
        basis=[
            "a FORCE checkout overlapping a chmod",
            "an op invoked after an observation that saw the reset follows the reset "
            "(`contracts.checkout._supersede_groups`, keyed per checkout and op rank)",
        ],
    )
    run.act("m0", "m0", "chmod", path=R0, mode=0o755)
    run.act("m1", "a", "chmod", path=R0, mode=0o755, requires=[("m0", "finished")])
    run.act(
        "c",
        "c",
        "checkout",
        revision=TIP,
        via="thrift",
        mode="force",
        requires=[("m0", "finished")],
    )
    run.act("o", "g1", "getattr", path=R0, requires=[("a", "finished")])
    run.act("m2", "b", "chmod", path=R0, mode=0o755, requires=[("g1", "finished")])
    run.act(
        "o",
        "g2",
        "getattr",
        path=R0,
        requires=[("a", "finished"), ("b", "finished"), ("c", "finished")],
    )
    run.call("m0", "ok", path=R0, mode=0o755)
    run.inv("a", "c")
    run.done("a", "ok", path=R0, mode=0o755)
    run.call("g1", "ok", **getattr_ok(0o644))
    run.call("b", "ok", path=R0, mode=0o755)
    run.done("c", "ok", **thrift_checkout(TIP, conflicts=1))
    run.call("g2", "ok", **getattr_ok(0o755 if kept else 0o644))
    run.finish()
    if kept:
        for anchor in ("g1", "g2"):
            run.expect(
                "attributes.mode-revert", anchor, SATISFIED, "no mode revert", path=R0
            )
    else:
        run.expect(
            "attributes.mode-revert",
            "g2",
            VIOLATED,
            "b follows the reset g1 observed",
            path=R0,
        )
        run.expect(
            "attributes.mode-revert",
            "g1",
            VIOLATED,
            "found-proof attribution: g1 is in the proof with g2 (its kept twin "
            "shows g1 alone is legal)",
            path=R0,
        )
    return run


def _size(name: str, description: str, *, size: int, write: bool) -> Run:
    run = Run("attributes", name, description, basis=[ATTRIBUTES])
    if write:
        run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "g", "getattr", path=R0)
    if write:
        run.call("w", "ok", **WRITE_OK)
    run.call("g", "ok", **getattr_fields(size=size))
    run.finish()
    return run


def _mtimes(name: str, description: str, *, first: int, second: int) -> Run:
    run = Run("attributes", name, description, basis=[ATTRIBUTES])
    run.act("a", "g", "getattr", path=R0)
    run.act("a", "h", "getattr", path=R0)
    run.call("g", "ok", **getattr_fields(mtime_ns=first))
    run.call("h", "ok", **getattr_fields(mtime_ns=second))
    run.finish()
    return run


def size_and_mtime_runs() -> list[Run]:
    written = _size(
        "size-after-write",
        "w writes r0's 64 bytes; a later getattr reports size 64.",
        size=64,
        write=True,
    )
    written.expect("attributes.size-mismatch", "g", SATISFIED, "w's 64 bytes", path=R0)
    short = _size(
        "size-short",
        "Nothing touches r0; a getattr reports size 32 for its 64 committed bytes.",
        size=32,
        write=False,
    )
    short.expect(
        "attributes.size-mismatch",
        "g",
        VIOLATED,
        "r0 holds its 64 committed bytes throughout",
        path=R0,
    )
    rising = _mtimes(
        "mtime-nondecreasing",
        "Two getattrs of an untouched r0 report mtime_ns 100 and then 200.",
        first=100,
        second=200,
    )
    rising.expect(
        "attributes.mtime-decreased", "h", SATISFIED, "200 is not below 100", path=R0
    )
    falling = _mtimes(
        "mtime-decreased",
        "Two getattrs of an untouched r0 report mtime_ns 200 and then 100.",
        first=200,
        second=100,
    )
    falling.expect(
        "attributes.mtime-decreased",
        "h",
        VIOLATED,
        "nothing wrote r0 between the two getattrs",
        path=R0,
        attribution="undetermined",
    )
    return [written, short, rising, falling]


def _untouched(name: str, description: str, **fields: Any) -> Run:
    run = Run("attributes", name, description, basis=[ATTRIBUTES])
    run.act("a", "g", "getattr", path=R0)
    run.call("g", "ok", **{**getattr_fields(), **fields})
    run.finish()
    return run


def untouched_runs() -> list[Run]:
    """An untouched r0 is a non-executable file at the start and at every revision."""
    legal = _untouched(
        "untouched-file",
        "Nothing touches r0; a getattr reports a non-executable file.",
    )
    legal.expect(
        "attributes.mode-revert", "g", SATISFIED, "r0 is not executable", path=R0
    )
    directory = _untouched(
        "untouched-reported-directory",
        "Nothing touches r0; a getattr reports a directory on both channels.",
        fuse_type="directory",
        thrift_type="directory",
    )
    directory.expect(
        "attributes.type-mismatch",
        "g",
        VIOLATED,
        "r0 is a file at the start and at every revision",
        path=R0,
        identity={"fuse-type": "directory"},
    )
    executable = _untouched(
        "untouched-reported-executable",
        "Nothing touches r0; a getattr reports mode 0755.",
        mode=0o755,
    )
    executable.expect(
        "attributes.mode-revert",
        "g",
        VIOLATED,
        "r0 is not executable at the start or at any revision",
        path=R0,
    )
    return [legal, directory, executable]


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "attributes",
        "racing-size-leaves-absence-to-existence",
        "A racing size leaves absence to existence.",
        basis=[
            "edenfs/properties/attributes.py: attributes.mode-revert, attributes.size-mismatch, attributes.type-mismatch"
        ],
    )
    run.act("a", "u", "unlink", path=R0)
    run.act("b", "g", "getattr", path=R0)
    run.inv("u", "g")
    run.done(
        "g",
        "ok",
        mtime=1.0,
        mtime_history=1.0,
        size=64,
        mode=420,
        fuse_type="file",
        thrift_type="file",
    )
    run.done("u", "ok", path=R0)
    run.finish()
    run.expect("attributes.type-mismatch", "g", SATISFIED, "no type mismatch", path=R0)
    found.append(run)

    run = Run(
        "attributes",
        "racing-wrong-size-is-violated-by-the-engine",
        "A racing wrong size is violated by the engine.",
        basis=[
            "edenfs/properties/attributes.py: attributes.mode-revert, attributes.size-mismatch, attributes.type-mismatch"
        ],
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("b", "g", "getattr", path=R0)
    run.inv("w", "g")
    run.done(
        "g",
        "ok",
        mtime=1.0,
        mtime_history=1.0,
        size=32,
        mode=420,
        fuse_type="file",
        thrift_type="file",
    )
    run.done("w", "ok", **WRITE_OK)
    run.finish()
    run.expect(
        "attributes.size-mismatch",
        "g",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={},
    )
    found.append(run)

    run = Run(
        "attributes",
        "equal-mtimes-are-legal",
        "Equal mtimes are legal.",
        basis=[
            "edenfs/properties/attributes.py: attributes.mode-revert, attributes.mtime-decreased, attributes.size-mismatch, attributes.type-disagreement, attributes.type-mismatch"
        ],
    )
    run.act("a", "g", "getattr", path=R0)
    run.act("a", "h", "getattr", path=R0)
    run.call(
        "g",
        "ok",
        mtime=1.0,
        mtime_history=1.0,
        size=64,
        mode=420,
        fuse_type="file",
        thrift_type="file",
        mtime_ns=100,
    )
    run.call(
        "h",
        "ok",
        mtime=1.0,
        mtime_history=1.0,
        size=64,
        mode=420,
        fuse_type="file",
        thrift_type="file",
        mtime_ns=100,
    )
    run.finish()
    for rule, anchor, reason in (
        ("attributes.mode-revert", "g", "no mode revert"),
        ("attributes.mode-revert", "h", "no mode revert"),
        ("attributes.mtime-decreased", "h", "no mtime decreased"),
        ("attributes.size-mismatch", "g", "no size mismatch"),
        ("attributes.size-mismatch", "h", "no size mismatch"),
        ("attributes.type-disagreement", "g", "no type disagreement"),
        ("attributes.type-disagreement", "h", "no type disagreement"),
        ("attributes.type-mismatch", "g", "no type mismatch"),
        ("attributes.type-mismatch", "h", "no type mismatch"),
    ):
        run.expect(rule, anchor, SATISFIED, reason, path=R0)
    found.append(run)
    return found


def detached_stat_runs() -> list[Run]:
    """A getattr racing a move or unlink of its path stats the inode its walk found."""
    basis = [
        "a FUSE getattr answers from one inode's stat (`FuseDispatcherImpl::getattr`), "
        "so its size and mode belong to an inode the path bound in the window"
    ]
    found = []
    for name, description, mover, size, mode, outcomes in (
        (
            "getattr-during-unlink-wrong-size",
            "A getattr of r0 overlaps an unlink of r0 and reports a size no value "
            "of r0 has.",
            "unlink",
            12345,
            0o644,
            {"attributes.size-mismatch": VIOLATED, "attributes.mode-revert": SATISFIED},
        ),
        (
            "getattr-during-rename-wrong-size",
            "A getattr of r1 overlaps a rename of r1 onto r0 and reports a size no "
            "value of r1 or r0 has: the moved inode keeps r1's bytes.",
            "rename",
            12345,
            0o644,
            {"attributes.size-mismatch": VIOLATED, "attributes.mode-revert": SATISFIED},
        ),
        (
            "getattr-during-rename-then-chmod-exec",
            "A getattr of r1 overlaps a rename of r1 onto r0 and a chmod 0755 of r0; "
            "it reports 0755: the walk found r1's inode, which the rename carried to "
            "r0 and the chmod made executable.",
            "rename+chmod",
            64,
            0o755,
            {
                "attributes.size-mismatch": SATISFIED,
                "attributes.mode-revert": SATISFIED,
            },
        ),
    ):
        run = Run("attributes", name, description, basis=basis)
        stat_path = R0 if mover == "unlink" else R1
        if mover == "unlink":
            run.act("a", "u", "unlink", path=R0)
        else:
            run.act("a", "u", "rename", source=R1, destination=R0)
            if mover == "rename+chmod":
                run.act("a", "x", "chmod", path=R0, mode=0o755)
        run.act("b", "g", "getattr", path=stat_path)
        run.inv("g")
        if mover == "unlink":
            run.call("u", "ok", path=R0)
        else:
            run.call("u", "ok", source=R1, destination=R0)
            if mover == "rename+chmod":
                run.call("x", "ok", path=R0, mode=0o755)
        run.done("g", "ok", **{**getattr_fields(size=size), "mode": mode})
        run.finish()
        for rule, outcome in outcomes.items():
            run.expect(rule, "g", outcome, description, path=stat_path)
        found.append(run)
    return found


def unlinked_path_present_runs() -> list[Run]:
    """An unlinked path a later getattr reports as a file: existence and attributes both reject."""
    basis = [EXISTENCE]
    run2 = Run(
        "existence",
        "unlinked-path-present",
        "r0 is unlinked; a later getattr reports it a 64-byte file.",
        basis=basis,
    )
    run2.act("a", "u", "unlink", path=R0)
    run2.act("a", "g", "getattr", path=R0)
    run2.call("u", "ok", path=R0)
    run2.call("g", "ok", **getattr_ok(0o644))
    run2.finish()
    run2.expect(
        "existence.present-after-remove",
        "g",
        VIOLATED,
        "the acked unlink removed r0 before g started",
        path=R0,
        identity={"observation-kind": "getattr"},
    )
    for rule in ("attributes.mode-revert", "attributes.size-mismatch"):
        run2.expect(
            rule,
            "g",
            VIOLATED,
            "no inode can leave r0 (no move from it, open, or checkout), so the stat "
            "must match a file bound at r0 in g's window; none is",
            path=R0,
        )
    return [run2]


def mkdir_type_channel_runs() -> list[Run]:
    """A mkdir'd directory whose getattr channels agree, or report a file via Thrift."""
    basis = [EXISTENCE]
    runs = []
    for agree in (True, False):
        run = Run(
            "existence",
            "mkdir-type-agrees" if agree else "mkdir-type-disagrees",
            "mkdir d succeeds; a later getattr reports a directory over FUSE and "
            + ("over Thrift." if agree else "a file via Thrift."),
            basis=basis,
        )
        run.act("a", "m", "mkdir", path=NEW_DIR)
        run.act("a", "g", "getattr", path=NEW_DIR)
        run.call("m", "ok", path=NEW_DIR)
        thrift_type = "directory" if agree else "file"
        run.call(
            "g",
            "ok",
            **{
                **FUSE_ATTRS,
                "size": 0,
                "mode": 0o755,
                "fuse_type": "directory",
                "thrift_type": thrift_type,
            },
        )
        run.finish()
        if agree:
            run.expect(
                "existence.mkdir-type",
                "g",
                SATISFIED,
                "both channels report the directory",
                path=NEW_DIR,
            )
        else:
            identity = {"fuse-type": "directory", "thrift-type": "file"}
            run.expect(
                "existence.mkdir-type",
                "g",
                VIOLATED,
                "the acked mkdir made d a directory",
                path=NEW_DIR,
                identity=identity,
            )
            run.expect(
                "attributes.type-disagreement",
                "g",
                VIOLATED,
                "both resolve the path against the same mount state (FUSE inode lookup; "
                "Thrift `co_getVirtualInode`)",
                path=NEW_DIR,
                identity=identity,
            )
        runs.append(run)
    return runs


def _rename_observers(name: str, description: str, *, legal: bool) -> Run:
    """r1 is unlinked; r0 is renamed onto it with getattrs of both inside the rename."""
    run = Run("existence", name, description, basis=[EXISTENCE])
    run.act("a", "u", "unlink", path=R1)
    run.act("a", "m", "rename", source=R0, destination=R1)
    observers = {"g": R0, "h": R1}
    first, second = ("g", "h") if legal else ("h", "g")
    for action in (first, second):
        run.act("b", action, "getattr", path=observers[action])
    run.call("u", "ok", path=R1)
    run.inv("m")
    run.call(first, "ok", **attrs())
    run.call(second, "ok", **attrs())
    run.done("m", "ok", source=R0, destination=R1)
    run.finish()
    return run


def rename_atomicity_runs() -> list[Run]:
    legal = _rename_observers(
        "rename-observers-ordered",
        "r1 is unlinked; during the rename of r0 onto r1, r0 is seen present and "
        "then r1 is seen present: one swap between the two explains both.",
        legal=True,
    )
    legal.expect(
        "existence.rename-atomicity",
        "m",
        SATISFIED,
        "the swap point lies between the two getattrs",
    )
    illegal = _rename_observers(
        "rename-observers-both-present",
        "r1 is unlinked; during the rename of r0 onto r1, r1 is seen present and "
        "then r0 is seen present: no single swap explains both.",
        legal=False,
    )
    illegal.expect(
        "existence.rename-atomicity",
        "m",
        VIOLATED,
        "r1 present fixes the swap before h ended; r0 present after it",
        identity={"state": "both-present"},
    )
    for rule in ("attributes.mode-revert", "attributes.size-mismatch"):
        illegal.expect(
            rule,
            "h",
            VIOLATED,
            "r1 is no move source, so h's stat must match a file bound at r1 in its "
            "window; in the proof with the atomicity violation none is",
            path=R1,
        )
    return [legal, illegal]


def mkdir_type_runs() -> list[Run]:
    run = Run(
        "existence",
        "mkdir-type-both-file",
        "mkdir d succeeds; a later getattr reports a file over FUSE and via Thrift.",
        basis=[EXISTENCE, "FUSE and Thrift are one serving group"],
    )
    run.act("a", "m", "mkdir", path=NEW_DIR)
    run.act("a", "g", "getattr", path=NEW_DIR)
    run.call("m", "ok", path=NEW_DIR)
    run.call("g", "ok", **attrs(thrift_type="file"))
    run.finish()
    run.expect(
        "existence.mkdir-type",
        "g",
        VIOLATED,
        "the acked mkdir made d a directory; FUSE and Thrift agree on a file: two "
        "observers of one serving group (the inode layer), so `undetermined`",
        path=NEW_DIR,
        attribution=UNDETERMINED,
        identity={"fuse-type": "file", "thrift-type": "file"},
    )
    for rule in ("attributes.mode-revert", "attributes.size-mismatch"):
        run.expect(
            rule,
            "g",
            VIOLATED,
            "no inode can leave d, so the stat must match a file bound at d in g's "
            "window; the acked mkdir bound a directory",
            path=NEW_DIR,
        )
    return [run]


def chmod_racing_checkout_runs() -> list[Run]:
    """A chmod racing a NORMAL checkout: superseded on a changed path, kept on a kept one."""
    runs = []
    run = racing(
        "normal-loses-chmod",
        "chmod 0755 of r0 overlaps a NORMAL checkout to rev2 with no conflict; r0 "
        "ends at rev2 with mode 0644: the chmod's lookup resolved the replaced inode.",
        mode="normal",
        revision=2,
        ops=("chmod",),
        conflicts=0,
        data=committed(R0, 2),
        executable=False,
    )
    run.expect("attributes.mode-revert", "g", SATISFIED, "c supersedes m", path=R0)
    runs.append(run)
    run = racing(
        "normal-loses-chmod-on-kept-path",
        "chmod 0755 of r0 overlaps a NORMAL checkout to the current revision; r0 "
        "reports mode 0644, but NORMAL never touches a path it keeps.",
        mode="normal",
        revision=TIP,
        ops=("chmod",),
        conflicts=0,
        data=committed(R0, TIP),
        executable=False,
    )
    run.expect(
        "attributes.mode-revert",
        "g",
        VIOLATED,
        "NORMAL keeps r0, so the acked chmod stays",
        path=R0,
    )
    runs.append(run)
    run = racing(
        "normal-keeps-write-loses-chmod",
        "A write and a chmod 0755 of r0 overlap a NORMAL checkout to rev2; r0 reads "
        "the write's bytes with mode 0644: each op resolves on its own.",
        mode="normal",
        revision=2,
        ops=("write", "chmod"),
        conflicts=0,
        data=value("b", "w"),
        executable=False,
    )
    run.expect(
        "attributes.mode-revert", "g", SATISFIED, "c supersedes m alone", path=R0
    )
    runs.append(run)
    return runs


def runs() -> list[Run]:
    return [
        *attribute_runs(),
        _chmod_after_observed_reset(kept=False),
        _chmod_after_observed_reset(kept=True),
        *size_and_mtime_runs(),
        *untouched_runs(),
        *collector_runs(),
        *detached_stat_runs(),
        *unlinked_path_present_runs(),
        *mkdir_type_channel_runs(),
        *rename_atomicity_runs(),
        *mkdir_type_runs(),
        *chmod_racing_checkout_runs(),
    ]
