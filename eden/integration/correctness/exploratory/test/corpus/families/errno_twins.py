# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `errno-twins`: state errnos legal in one state and illegal in
its twin."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    MOVED,
    NEW_DIR,
    R0,
    R1,
    R2,
    read_ok,
    Run,
    SATISFIED,
    thrift_checkout,
    thrift_status,
    TIP,
    VIOLATED,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    CHECKOUT_GUARD,
    code,
    EISDIR,
    ENOENT,
    inputs_ok,
    process_fail,
    SL_RM_FORCE,
    stale_read,
    THRIFT_NORMAL_TABLE,
)


EEXIST = 17


def errno_runs() -> list[Run]:
    runs = []
    run = Run(
        "errno-twins",
        "enoent-after-unlink",
        "r0 is unlinked; a read fails ENOENT.",
        basis=["a failure's errno is judged against the path's state"],
    )
    run.act("a", "u", "unlink", path=R0)
    run.act("a", "r", "read", path=R0, channel="fuse")
    run.call("u", "ok", path=R0)
    run.call("r", "fail", **errno_fail(ENOENT))
    run.finish()
    run.expect("namespace.precondition", "u", SATISFIED, "r0 is a file at u", path=R0)
    runs.append(run)

    run = Run(
        "errno-twins",
        "enoent-with-file-present",
        "A read of r0 fails ENOENT while r0 exists.",
        basis=["a failure's errno is judged against the path's state"],
    )
    run.act("a", "r", "read", path=R0, channel="fuse")
    run.call("r", "fail", **errno_fail(ENOENT))
    run.finish()
    run.bad_input("r", "ENOENT", "r0 holds rev3's bytes at every point")
    runs.append(run)

    for existing in (True, False):
        name = "eexist-mkdir-existing" if existing else "eexist-mkdir-absent"
        run = Run(
            "errno-twins",
            name,
            (
                "mkdir d succeeds, then a second mkdir d fails EEXIST."
                if existing
                else "mkdir d fails EEXIST although d never existed."
            ),
            basis=["a failure's errno is judged against the path's state"],
        )
        if existing:
            run.act("a", "m1", "mkdir", path=NEW_DIR)
        run.act("a", "m2", "mkdir", path=NEW_DIR)
        if existing:
            run.call("m1", "ok", path=NEW_DIR)
        run.call("m2", "fail", **errno_fail(EEXIST))
        run.finish()
        if existing:
            run.expect(
                "namespace.precondition",
                "m1",
                SATISFIED,
                "d is absent at m1",
                path=NEW_DIR,
            )
        else:
            run.bad_input("m2", "EEXIST", "d is absent at every point")
        runs.append(run)

    for gone in (True, False):
        name = "enoent-rename-source-gone" if gone else "enoent-rename-source-present"
        run = Run(
            "errno-twins",
            name,
            (
                "r0 is renamed away; a second rename of r0 fails ENOENT."
                if gone
                else "A rename of r0 fails ENOENT while r0 exists."
            ),
            basis=["a failure's errno is judged against the path's state"],
        )
        if gone:
            run.act("a", "m1", "rename", source=R0, destination=MOVED)
        run.act("a", "m2", "rename", source=R0, destination="tree/registers/other.reg")
        if gone:
            run.call("m1", "ok", source=R0, destination=MOVED)
            inputs_ok(run, "m1")
        run.call("m2", "fail", **errno_fail(ENOENT))
        run.finish()
        if gone:
            run.expect(
                "core.input-legality",
                "m2",
                SATISFIED,
                "m1 removed r0",
                subject=run.input_subject("m2"),
            )
            run.expect(
                "namespace.precondition",
                "m1",
                SATISFIED,
                "r0 is present at m1",
                path=R0,
            )
        else:
            run.bad_input("m2", "ENOENT", "r0 holds rev3's bytes at every point")
        runs.append(run)
    return runs


WINDOWS_ERRNO = "EdenFS refuses a status only while a checkout runs"


def _guard_refusal(name: str, description: str, *, overlap: bool) -> Run:
    """A Thrift status refused CHECKOUT_IN_PROGRESS, inside a checkout or alone."""
    run = Run("errno-twins", name, description, basis=[CHECKOUT_GUARD, WINDOWS_ERRNO])
    run.act("b", "s", "status", channel="thrift")
    refused = {
        "reason": "thrift: cannot compute status while a checkout is currently in "
        "progress",
        "source": "thrift",
        "error_type": "CHECKOUT_IN_PROGRESS",
    }
    if overlap:
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.inv("c")
        run.call("s", "fail", **refused)
        run.done("c", "ok", **thrift_checkout(2, conflicts=0))
    else:
        run.call("s", "fail", **refused)
    run.finish()
    return run


def _out_of_date(name: str, description: str, *, moved: bool) -> Run:
    """A Thrift status of rev3 refused OUT_OF_DATE_PARENT, after a checkout or not."""
    run = Run(
        "errno-twins", name, description, basis=[THRIFT_NORMAL_TABLE, WINDOWS_ERRNO]
    )
    if moved:
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.act("a", "s", "status", channel="thrift", revision=TIP)
    if moved:
        run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    run.call(
        "s",
        "fail",
        reason="thrift: requested parent is out of date",
        source="thrift",
        error_type="OUT_OF_DATE_PARENT",
    )
    run.finish()
    return run


def _sl_refusal(name: str, description: str, *, overlap: bool) -> Run:
    """An `sl status` exits 255 with EdenFS's status refusal, inside a checkout or alone.

    The stderr is the one a live mount prints: `EdenMount::co_diff` refusing while
    a checkout runs, surfaced once Sapling's poll gives up.
    """
    run = Run("errno-twins", name, description, basis=[CHECKOUT_GUARD, WINDOWS_ERRNO])
    run.act("b", "s", "status", channel="sl")
    stderr = (
        "abort: EdenError: cannot compute status while a checkout is currently "
        "in progress\n"
    )
    facts = {
        "reason": "status: exit 255",
        "returncode": 255,
        "stdout": "",
        "stderr": stderr,
    }
    if overlap:
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.inv("c")
        run.call("s", "fail", **facts)
        run.done("c", "ok", **thrift_checkout(2, conflicts=0))
    else:
        run.call("s", "fail", **facts)
    run.finish()
    return run


def refused_status_runs() -> list[Run]:
    runs = []
    legal = _guard_refusal(
        "thrift-status-refused-during-checkout",
        "A Thrift status runs inside a Thrift checkout and fails CHECKOUT_IN_PROGRESS.",
        overlap=True,
    )
    legal.expect("result-shape.channel-fields", "s", SATISFIED, "one status variant")
    runs.append(legal)
    illegal = _guard_refusal(
        "thrift-status-refused-without-checkout",
        "A Thrift status fails CHECKOUT_IN_PROGRESS with no checkout in the run.",
        overlap=False,
    )
    illegal.bad_input("s", "CHECKOUT_IN_PROGRESS", "no checkout can hold the guard")
    runs.append(illegal)
    legal = _out_of_date(
        "thrift-status-out-of-date-after-checkout",
        "After a Thrift checkout to rev2, a Thrift status of rev3 fails "
        "OUT_OF_DATE_PARENT.",
        moved=True,
    )
    legal.expect("result-shape.channel-fields", "s", SATISFIED, "one status variant")
    runs.append(legal)
    illegal = _out_of_date(
        "thrift-status-out-of-date-at-parent",
        "With no checkout, a Thrift status of rev3 (EdenFS's parent) fails "
        "OUT_OF_DATE_PARENT.",
        moved=False,
    )
    illegal.bad_input("s", "OUT_OF_DATE_PARENT", "rev3 is EdenFS's parent throughout")
    runs.append(illegal)
    legal = _sl_refusal(
        "sl-status-refused-during-checkout",
        "An sl status inside a Thrift checkout exits 255 with the status refusal.",
        overlap=True,
    )
    legal.expect("result-shape.channel-fields", "s", SATISFIED, "one status variant")
    runs.append(legal)
    illegal = _sl_refusal(
        "sl-status-refused-without-checkout",
        "An sl status exits 255 with the status refusal and no checkout in the run.",
        overlap=False,
    )
    illegal.bad_input("s", "none", "no checkout can hold the guard")
    runs.append(illegal)
    return runs


def _directory_descriptor(name: str, description: str, *, path: str) -> Run:
    run = Run(
        "errno-twins",
        name,
        description,
        basis=[
            "FUSE OPENDIR: open(O_RDONLY) of a directory succeeds; read(2) on it "
            "fails EISDIR"
        ],
        type_changes=[{"path": R2, "at_revision": TIP}],
    )
    run.act("a", "o", "open", path=path)
    run.act("a", "f", "read_fd", fd="o", requires=[("o", "finished")])
    run.act("a", "z", "close", fd="o", requires=[("f", "finished")])
    run.act("a", "g", "read", path=R0, channel="fuse", requires=[("z", "finished")])
    run.call("o", "ok", fd=5)
    run.call("f", "fail", reason="read", errno=EISDIR)
    run.call("z", "ok", fd=5)
    run.call("g", "ok", **read_ok(committed(R0, TIP)))
    run.finish()
    stale_read(run, "g", R0, SATISFIED, "nothing wrote r0")
    return run


def eisdir_runs() -> list[Run]:
    run = _directory_descriptor(
        "eisdir-read-fd-directory",
        "r2 is a directory at the start; a descriptor opened on it fails read EISDIR.",
        path=R2,
    )
    twin = _directory_descriptor(
        "eisdir-read-fd-file",
        "A descriptor opened on the file r0 fails read EISDIR.",
        path=R0,
    )
    twin.bad_input("f", "EISDIR", "the descriptor's object is a file")
    return [run, twin]


SL_MV = (
    "`sl mv` moves a tracked present source onto an absent destination and exits "
    "255 when the source has no tracked file"
)


THRIFT_READDIR_ERRORS = (
    "a Thrift readdir failure's (errorType, errno) pair names the path's state"
)


ENOTDIR = 20


EINVAL = 22


def _rename_into(name: str, description: str, *, made: bool) -> Run:
    run = Run(
        "errno-twins",
        name,
        description,
        basis=["POSIX rename(2): ENOENT for a missing directory in newpath"],
    )
    run.act("a", "m", "mkdir", path=NEW_DIR)
    run.act(
        "a",
        "r",
        "rename",
        source=R0,
        destination=NEW_DIR + "/x",
        requires=[("m", "finished")],
    )
    run.act("a", "g", "read", path=R0, channel="fuse", requires=[("r", "finished")])
    run.inv("m")
    if made:
        run.done("m", "ok", path=NEW_DIR)
    else:
        run.done("m", "info", reason="operation deadline")
    run.call("r", "fail", **errno_fail(ENOENT, "rename"))
    run.call("g", "ok", **read_ok(committed(R0, TIP)))
    run.finish()
    stale_read(run, "g", R0, SATISFIED, "the failed rename moved nothing")
    return run


def _readdir_thrift(
    name: str, description: str, *, made: bool, error: tuple[str, int]
) -> Run:
    run = Run("errno-twins", name, description, basis=[THRIFT_READDIR_ERRORS])
    run.act("a", "m", "mkdir", path=NEW_DIR)
    run.act(
        "a",
        "d",
        "readdir",
        path=NEW_DIR,
        channel="thrift",
        requires=[("m", "finished")],
    )
    run.act("a", "g", "read", path=R0, channel="fuse", requires=[("d", "finished")])
    run.inv("m")
    if made:
        run.done("m", "ok", path=NEW_DIR)
    else:
        run.done("m", "info", reason="operation deadline")
    error_type, number = error
    run.call(
        "d",
        "fail",
        reason="thrift: x",
        source="thrift",
        error_type=error_type,
        errno=number,
    )
    run.call("g", "ok", **read_ok(committed(R0, TIP)))
    run.finish()
    stale_read(run, "g", R0, SATISFIED, "nothing wrote r0")
    return run


def probe_errno_runs() -> list[Run]:
    runs = []
    run = _rename_into(
        "enoent-rename-destination-directory-absent",
        "A mkdir of the destination directory never resolves; renaming r0 into "
        "it fails ENOENT and r0 keeps its bytes.",
        made=False,
    )
    runs.append(run)

    run = _rename_into(
        "enoent-rename-destination-directory-present",
        "The destination directory exists and r0 is present, yet renaming r0 "
        "into it fails ENOENT.",
        made=True,
    )
    run.bad_input(
        "r", "ENOENT", "the source and the destination's directory both exist"
    )
    runs.append(run)

    run = _readdir_thrift(
        "thrift-readdir-posix-enoent-unmade-directory",
        "A mkdir never resolves; a Thrift readdir of it fails POSIX_ERROR/ENOENT.",
        made=False,
        error=("POSIX_ERROR", ENOENT),
    )
    runs.append(run)

    run = _readdir_thrift(
        "thrift-readdir-posix-enoent-made-directory",
        "A mkdir succeeds; a Thrift readdir of it then fails POSIX_ERROR/ENOENT.",
        made=True,
        error=("POSIX_ERROR", ENOENT),
    )
    run.bad_input("d", "ENOENT", "the directory exists")
    runs.append(run)

    run = _readdir_thrift(
        "thrift-readdir-argument-error-directory",
        "A mkdir succeeds; a Thrift readdir of it then fails ARGUMENT_ERROR/EINVAL "
        "(only a regular file does).",
        made=True,
        error=("ARGUMENT_ERROR", EINVAL),
    )
    run.bad_input("d", "EINVAL", "the path is a directory, not a regular file")
    runs.append(run)

    for made in (False, True):
        run = _readdir_under(
            "thrift-readdir-posix-enotdir-"
            + ("directory-ancestor" if made else "file-ancestor"),
            (
                "Both directories are made; a Thrift readdir of the inner one fails "
                "POSIX_ERROR/ENOTDIR."
                if made
                else "The directory mkdirs never resolve and r0 is renamed onto the "
                "outer one; a Thrift readdir of the inner one fails "
                "POSIX_ERROR/ENOTDIR."
            ),
            made=made,
        )
        if made:
            run.bad_input("d", "ENOTDIR", "every directory above the path exists")
        else:
            run.legal_input("d", "the renamed r0 is a file above the path")
        runs.append(run)
    return runs


def _readdir_under(name: str, description: str, *, made: bool) -> Run:
    """A Thrift readdir of `NEW_DIR/sub` fails ENOTDIR (a component is a file)."""
    inner = NEW_DIR + "/sub"
    run = Run("errno-twins", name, description, basis=[THRIFT_READDIR_ERRORS])
    run.act("a", "m", "mkdir", path=NEW_DIR)
    run.act("a", "n", "mkdir", path=inner, requires=[("m", "finished")])
    if not made:
        run.act(
            "a",
            "v",
            "rename",
            source=R0,
            destination=NEW_DIR,
            requires=[("n", "finished")],
        )
    run.act(
        "a",
        "d",
        "readdir",
        path=inner,
        channel="thrift",
        requires=[("n", "finished")],
    )
    # r0's bytes, wherever r0 is now.
    holder = NEW_DIR if not made else R0
    run.act("a", "g", "read", path=holder, channel="fuse", requires=[("d", "finished")])
    if made:
        run.call("m", "ok", path=NEW_DIR)
        run.call("n", "ok", path=inner)
    else:
        run.call("m", "info", reason="operation deadline")
        run.call("n", "info", reason="operation deadline")
        run.call("v", "ok", source=R0, destination=NEW_DIR)
    run.call(
        "d",
        "fail",
        reason="thrift: x",
        source="thrift",
        error_type="POSIX_ERROR",
        errno=ENOTDIR,
    )
    run.call("g", "ok", **read_ok(committed(R0, TIP)))
    run.finish()
    stale_read(run, "g", holder, SATISFIED, "r0's bytes moved at most by the rename")
    return run


def _enotdir_under(kind: str, *, made: bool) -> Run:
    """A `kind` of a path under `NEW_DIR` fails ENOTDIR: the path walk found a file."""
    inner = NEW_DIR + "/sub"
    run = Run(
        "errno-twins",
        f"{kind}-enotdir-" + ("directory-ancestor" if made else "file-ancestor"),
        (
            f"The directory is made; a {kind} of a path under it fails ENOTDIR."
            if made
            else f"The directory mkdir never resolves and r0 is renamed onto it; a "
            f"{kind} of a path under it fails ENOTDIR."
        ),
        basis=["POSIX path resolution: ENOTDIR for a non-directory component"],
    )
    run.act("a", "m", "mkdir", path=NEW_DIR)
    if kind == "read":
        # The read names a path the model knows: the inner directory's.
        run.act("a", "n", "mkdir", path=inner, requires=[("m", "finished")])
    if not made:
        run.act(
            "a",
            "v",
            "rename",
            source=R0,
            destination=NEW_DIR,
            requires=[("m", "finished")],
        )
    payload = {"channel": "fuse"} if kind == "read" else {}
    run.act("a", "d", kind, path=inner, requires=[("m", "finished")], **payload)
    # r0's bytes, wherever r0 is now.
    holder = R0 if made else NEW_DIR
    run.act("a", "g", "read", path=holder, channel="fuse", requires=[("d", "finished")])
    made_outcome = (
        {"outcome": "ok"}
        if made
        else {"outcome": "info", "reason": "operation deadline"}
    )
    run.call("m", **made_outcome, **({"path": NEW_DIR} if made else {}))
    if kind == "read":
        run.call("n", **made_outcome, **({"path": inner} if made else {}))
    if not made:
        run.call("v", "ok", source=R0, destination=NEW_DIR)
    run.call("d", "fail", **errno_fail(ENOTDIR, kind))
    run.call("g", "ok", **read_ok(committed(R0, TIP)))
    run.finish()
    stale_read(run, "g", holder, SATISFIED, "r0's bytes moved at most by the rename")
    if made:
        run.bad_input("d", "ENOTDIR", "every directory above the path exists")
    else:
        run.legal_input("d", "the renamed r0 is a file above the path")
    return run


def enotdir_runs() -> list[Run]:
    return [
        _enotdir_under(kind, made=made)
        for kind in ("mkdir", "read")
        for made in (False, True)
    ]


def dirstate_runs() -> list[Run]:
    runs = []
    run = Run(
        "errno-twins",
        "sl-rm-exit-0-already-removed",
        "sl rm r0 succeeds; sl rm r0 again exits 0 (an already-removed file is "
        "still in the dirstate).",
        basis=[SL_RM_FORCE],
    )
    run.act("a", "h", "sl_rm", path=R0)
    run.act("a", "x", "sl_rm", path=R0)
    run.act("a", "s", "status", channel="thrift")
    run.call("h", "ok", path=R0)
    run.call("x", "ok", path=R0)
    run.call("s", "ok", **thrift_status({R0: "R"}))
    run.finish()
    code(run, "s", R0, "R", SATISFIED, "both sl rm calls removed r0")
    runs.append(run)

    run = Run(
        "errno-twins",
        "sl-rm-exit-1-tracked",
        "sl rm of the tracked r0 exits 1, which only an untracked path does.",
        basis=[SL_RM_FORCE],
    )
    run.act("a", "x", "sl_rm", path=R0)
    run.call("x", "fail", **process_fail(1))
    run.finish()
    run.bad_input("x", "none", "r0 is tracked, so `sl rm --force` exits 0")
    runs.append(run)

    for exists in (True, False):
        run = Run(
            "errno-twins",
            "sl-mv-existing-destination-" + ("kept" if exists else "overwritten"),
            "r1 is renamed to the destination; sl mv r0 onto it exits 0 and "
            + (
                "both keep their bytes."
                if exists
                else "the destination reads r0's bytes."
            ),
            basis=[SL_MV],
        )
        run.act("a", "m", "rename", source=R1, destination=MOVED)
        run.act("a", "v", "sl_mv", source=R0, destination=MOVED)
        run.act("a", "g", "read", path=MOVED, channel="fuse")
        run.call("m", "ok", source=R1, destination=MOVED)
        run.call("v", "ok", source=R0, destination=MOVED)
        data = committed(R1, TIP) if exists else committed(R0, TIP)
        run.call("g", "ok", **read_ok(data))
        run.finish()
        stale_read(
            run,
            "g",
            MOVED,
            SATISFIED if exists else VIOLATED,
            "sl mv does not overwrite an existing destination",
        )
        runs.append(run)

    for removed in (True, False):
        run = Run(
            "errno-twins",
            "sl-mv-exit-255-" + ("removed-source" if removed else "tracked-source"),
            ("sl rm r0, then " if removed else "")
            + "sl mv r0 exits 255 `no files to copy`.",
            basis=[SL_MV],
        )
        if removed:
            run.act("a", "h", "sl_rm", path=R0)
        run.act("a", "v", "sl_mv", source=R0, destination=MOVED)
        if removed:
            run.act("a", "s", "status", channel="thrift")
            run.call("h", "ok", path=R0)
        run.call("v", "fail", **process_fail(255, "abort: no files to copy\n"))
        if removed:
            run.call("s", "ok", **thrift_status({R0: "R"}))
            code(run, "s", R0, "R", SATISFIED, "the refused sl mv created nothing")
        run.finish()
        if removed:
            run.legal_input("v", "a source marked removed has no tracked file")
        else:
            run.bad_input("v", "none", "r0 is a tracked file, so sl mv moves it")
        runs.append(run)
    return runs


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "errno-twins",
        "second-checkout-rejection-is-no-status-refusal",
        "The second checkout rejection is no status refusal.",
        basis=["core input legality: the contract observation of a failed input"],
    )
    run.act("b", "s", "status", channel="sl")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.inv("c", "s")
    run.done(
        "s",
        "fail",
        reason="status: exit 255",
        returncode=255,
        stdout="",
        stderr="abort: EdenError: another checkout operation is still in progress\n",
    )
    run.done("c", "ok", **thrift_checkout(2, conflicts=0))
    run.finish()
    run.expect(
        "core.input-legality",
        "s",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=".",
        identity={"errno": "none", "input-kind": "status"},
    )
    found.append(run)
    return found


def runs() -> list[Run]:
    return [
        *errno_runs(),
        *refused_status_runs(),
        *eisdir_runs(),
        *probe_errno_runs(),
        *enotdir_runs(),
        *dirstate_runs(),
        *collector_runs(),
    ]
