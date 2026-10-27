# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""A legal-history simulator for the EdenFS namespace and checkout surface.

A test fixture that shares no code with the contracts, the collectors, or
the engine (only the corpus authoring helpers that encode results). It
does share their modelling assumptions, written down twice rather than
checked independently: the checkout guard's refusals, the per-mode
conflict tables, which local changes block `sl goto`, and when Sapling
writes and deletes the updatestate file. Its scope is registers that are
committed files at every revision, so a conflict is a modified-modified
one; added, removed, type-changed, and untracked paths are a non-goal
here (the corpus covers them). Each action applies at points inside its
window.

Checkouts run a server interval [b, e] inside the client window. At b the
guard refuses a checkout while another one runs ("another checkout ... in
progress"), and while an interrupted checkout to another target stands
(the interrupted advice); a checkout to the interrupted one's target
resumes it from its source. Thrift NORMAL/FORCE moves the EdenFS
parent at b and visits each register at its own point with the per-mode
conflict tables; the conflict count is the number of conflicting
visits. `sl goto` runs as Sapling runs it
(`fbcode/eden/scm/lib/checkout/src/edenfs.rs:366-426`):
per-register DRY_RUN visits (blocked: dirty other than to the
destination, unlinked, or removed through Sapling), an abort
listing the blockers, else `.hg/updatestate` written, the parent moved,
per-register NORMAL visits with the Thrift NORMAL table. Every `sl` attempt EdenFS's checkout guard does not
refuse deletes `.hg/updatestate` first;
`sl go --continue` without it exits "not in an interrupted goto state".
An interrupted checkout (the run's first, in the interrupted shapes)
stops at a point inside its walk: a prefix of its visits applied, the
mount interrupted until a same-target checkout completes; a Thrift
status refuses while a checkout runs or the mount stays interrupted.
`sl` commands never overlap (Sapling's lock): their windows are
serialized. Namespace operations (write/rename/unlink/mkdir/chmod)
return state-dependent errnos, `sl rm`/`sl mv` Sapling's exit
statuses (`sl rm` of a path already marked removed changes nothing,
`cmdutil.remove`); observers (read/getattr/readdir/Thrift status) report
the simulated state. Phase B observers run after phase A.

`simulate(seed)` returns a `Run`, and `judge(seed)` returns None when the
spec validator refuses it (the `--continue` destination rule), or the task
a refused spec waits on (`SpecError.waiting_on`: a checkout walk
overlapping a remove or transfer, T293953113);
`mutate="count"` adds one to the conflict count
of every acked Thrift checkout that resumes nothing, a planted
contradiction. A resume may also report the paths the interrupted walk
already added or removed, which registers never are, so a
count planted there may be legal. `judge(seed)` checks it with
`EDENFS_TARGET`.
"""

from __future__ import annotations

import errno as E
import json
import random
from typing import Any

from eden.integration.correctness.exploratory.core.checker import check, Verdict
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.runspec import from_json, SpecError
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    MOVED,
    NEW_DIR,
    R0,
    R1,
    R2,
    read_ok,
    Run,
    sl_abort,
    sl_checkout,
    thrift_checkout,
    thrift_rejected,
    thrift_status,
    TIP,
    value,
    WRITE_OK,
)

REGS = (R0, R1, R2)
DIR = "DIR"
IN_PROGRESS = "another checkout operation is still in progress"
INTERRUPTED = (
    "a previous checkout was interrupted - please run `sl go {rev}` to resume it."
)
STATUS_IN_PROGRESS = "cannot compute status while a checkout is currently in progress"
SL_KINDS = frozenset({"sl_rm", "sl_mv"})
# Run shapes: plain Thrift, plain sl, and a first checkout interrupted mid-walk.
SHAPES = (*("thrift",) * 7, "sl", "thrift-int", "sl-int")


def attrs(kind: str, exe: bool = False) -> dict[str, Any]:
    return {
        "mtime": 1.0,
        "mtime_history": 1.0,
        "size": 64,
        "mode": 0o755 if exe else 0o644,
        "fuse_type": kind,
        "thrift_type": kind,
    }


class St:
    def __init__(self):
        self.f = {
            p: (committed(p, TIP), False) for p in REGS
        }  # path -> (bytes|DIR, exec)
        self.track = dict.fromkeys((*REGS, MOVED), "clean")
        self.eden = TIP
        self.sapling = TIP
        self.active = []  # applied checkouts in progress (ids)
        self.interrupted = None  # (source, target) of the interrupted checkout
        self.updatestate = None  # the destination `.hg/updatestate` names


def efail(code: int) -> tuple[str, dict[str, Any]]:
    return ("fail", {"reason": "syscall failed", "errno": code})


def _sl_fail(stderr: str) -> tuple[str, dict[str, Any]]:
    return (
        "fail",
        {
            "reason": f"update: {stderr.strip()}",
            "returncode": 255,
            "stdout": "",
            "stderr": stderr,
        },
    )


def thrift_codes(st: St) -> dict[str, str]:
    out: dict[str, str] = {}
    for p in (*REGS, MOVED):
        base = committed(p, st.eden) if p in REGS else None
        cur = st.f.get(p)
        if cur is None:
            if base is not None:
                out[p] = "R"
        elif base is None:
            out[p] = "A"
        elif cur[0] != base or cur[1]:
            out[p] = "M"
    return out


def _actions(  # noqa: C901
    rng: random.Random, shape: str
) -> list[tuple[str, str, dict[str, Any]]]:
    """Return the phase A actions (id, kind, payload); an interrupted shape starts with `c0`."""
    acts: list[tuple[str, str, dict[str, Any]]] = []
    interrupted = shape.endswith("-int")
    if interrupted:
        via = "thrift" if shape == "thrift-int" else "goto"
        acts.append(
            (
                "c0",
                "checkout",
                {"revision": rng.choice((1, 2)), "via": via, "mode": "normal"},
            )
        )
    ncheck = 0
    for i in range(1, rng.randrange(3, 9) + 1):
        r = rng.random()
        if r < 0.22 and ncheck < (2 if shape == "sl" else 3):
            ncheck += 1
            rev = rng.choice((1, 2, TIP, TIP))
            if shape == "sl-int" and rng.random() < 0.5:
                target = acts[0][2]["revision"]
                payload = {"revision": target, "via": "goto", "mode": "continue"}
            elif shape in ("sl", "sl-int") or (
                shape == "thrift-int" and rng.random() < 0.5
            ):
                payload = {"revision": rev, "via": "goto", "mode": "normal"}
            else:
                mode = rng.choice(("normal", "force"))
                payload = {"revision": rev, "via": "thrift", "mode": mode}
            acts.append((f"c{i}", "checkout", payload))
        elif r < 0.36:
            acts.append(
                (f"w{i}", "write", {"path": rng.choice((R0, R1)), "value_id": f"v{i}"})
            )
        elif r < 0.46:
            acts.append(
                (f"r{i}", "read", {"path": rng.choice(REGS), "channel": "fuse"})
            )
        elif r < 0.54:
            acts.append((f"g{i}", "getattr", {"path": rng.choice(REGS)}))
        elif r < 0.60:
            acts.append((f"u{i}", "unlink", {"path": rng.choice((R0, R1))}))
        elif r < 0.66:
            s, d = rng.choice(((R0, MOVED), (R1, R0), (R2, R1)))
            acts.append((f"m{i}", "rename", {"source": s, "destination": d}))
        elif r < 0.71:
            acts.append(
                (
                    f"x{i}",
                    "chmod",
                    {"path": rng.choice((R0, R1)), "mode": rng.choice((0o755, 0o644))},
                )
            )
        elif r < 0.75:
            acts.append((f"d{i}", "mkdir", {"path": NEW_DIR}))
        elif r < 0.79:
            acts.append((f"h{i}", "sl_rm", {"path": rng.choice((R0, R1))}))
        elif r < 0.83:
            acts.append(
                (
                    f"v{i}",
                    "sl_mv",
                    {"source": rng.choice((R0, R1)), "destination": MOVED},
                )
            )
        elif r < 0.91:
            acts.append((f"t{i}", "status", {"channel": "thrift", "revision": TIP}))
        else:
            acts.append(
                (
                    f"l{i}",
                    "readdir",
                    {
                        "path": "tree/registers",
                        "channel": rng.choice(("fuse", "thrift")),
                    },
                )
            )
    if any(k == "mkdir" for _, k, _ in acts):
        acts = [a for a in acts if a[1] != "status"]
    return acts


def _windows(
    rng: random.Random,
    acts: list[tuple[str, str, dict[str, Any]]],
    bacts: list[tuple[str, str, dict[str, Any]]],
    interrupted: bool,
) -> dict[str, tuple[float, float]]:
    """Return each action's client window.

    An interrupted shape's `c0` completes before any other action starts.
    `sl` commands are shifted so no two overlap (Sapling's lock).
    """
    win: dict[str, tuple[float, float]] = {}
    for aid, _kind, _ in acts:
        lo = (
            rng.uniform(0.5, 1.0)
            if interrupted and aid == "c0"
            else rng.uniform(1.0, 10.0)
        )
        win[aid] = (lo, lo + rng.uniform(0.1, 4.0))
    if interrupted:
        first = win["c0"][1]
        for aid in win:
            if aid != "c0":
                win[aid] = (win[aid][0] + first, win[aid][1] + first)
    sl = sorted(
        (
            aid
            for aid, kind, payload in acts
            if kind in SL_KINDS or (kind == "checkout" and payload["via"] != "thrift")
        ),
        key=lambda aid: win[aid][0],
    )
    for prev, nxt in zip(sl, sl[1:]):
        if win[nxt][0] <= win[prev][1]:
            shift = win[prev][1] + 0.05 - win[nxt][0]
            win[nxt] = (win[nxt][0] + shift, win[nxt][1] + shift)
    t0 = max(hi for _, hi in win.values()) + 1.0
    for aid, _kind, _ in bacts:
        lo = rng.uniform(t0, t0 + 3)
        win[aid] = (lo, lo + rng.uniform(0.1, 1.0))
    return win


def _points(
    prng: random.Random,
    kinds: dict[str, tuple[str, dict[str, Any]]],
    win: dict[str, tuple[float, float]],
    interrupted: bool,
) -> list[tuple[float, float, str, str, str | None]]:
    """Return the server points (time, order, action, part, path), sorted."""
    pts: list[tuple[float, float, str, str, str | None]] = []
    for aid, (kind, payload) in kinds.items():
        lo, hi = win[aid]
        if kind != "checkout":
            pts.append((prng.uniform(lo, hi), 0, aid, "op", None))
            continue
        b = prng.uniform(lo, hi)
        e = prng.uniform(b, hi)
        pts.append((b, 0, aid, "begin", None))
        walk = b
        if payload["via"] != "thrift":
            decide = prng.uniform(b, e)
            walk = prng.uniform(decide, e)
            for p in REGS:
                pts.append((prng.uniform(b, decide), 1, aid, "dry", p))
            pts.append((decide, 2, aid, "decide", None))
            pts.append((walk, 3, aid, "normal", None))
        for p in REGS:
            pts.append((prng.uniform(walk, e), 4, aid, "path", p))
        if interrupted and aid == "c0":
            pts.append((prng.uniform(walk, e), 4.5, aid, "interrupt", None))
        pts.append((e, 5, aid, "end", None))
    pts.sort(key=lambda x: (x[0], x[1]))
    return pts


def _checkout_point(  # noqa: C901
    st: St, c: dict[str, Any], aid: str, pl: dict[str, Any], part: str, p: str | None
) -> None:
    """Apply one checkout point to the simulated state."""
    T = pl["revision"]
    sl = pl["via"] != "thrift"
    if c["dead"] or c["rejected"]:
        return
    if part == "begin":
        if pl["mode"] == "continue" and st.updatestate is None:
            c["rejected"] = "not-interrupted"
            return
        if st.active:
            c["rejected"] = "in-progress"
            return
        if st.interrupted is not None and st.interrupted[1] != T:
            c["rejected"] = "interrupted"
            c["interrupted"] = st.interrupted[1]
            return
        if sl:
            st.updatestate = None
        c["src"] = st.eden if st.interrupted is None else st.interrupted[0]
        c["resume"] = st.interrupted is not None
        st.active.append(aid)
        if not sl:
            st.eden = T
        return
    S = c["src"]
    if part == "dry":
        assert p is not None
        sb, tb = committed(p, S), committed(p, T)
        if sb == tb:
            return
        cur = st.f.get(p)
        if st.track.get(p) == "removed" or cur not in ((sb, False), (tb, False)):
            c["blocked"].append(p)
        return
    if part == "decide":
        if c["blocked"]:
            c["abort"] = sorted(c["blocked"])
            st.active.remove(aid)
            c["dead"] = True
        else:
            st.updatestate = T
        return
    if part == "normal":
        st.eden = T
        return
    if part == "interrupt":
        st.interrupted = (S, T)
        st.active.remove(aid)
        c["dead"] = True
        c["failed"] = True
        return
    if part == "path":
        assert p is not None
        sb, tb = committed(p, S), committed(p, T)
        cur = st.f.get(p)
        tgt = (tb, False)
        if pl["mode"] == "force":
            if cur not in (tgt, (sb, False)):
                c["conflicts"] += 1
            st.f[p] = tgt
            return
        if sb == tb:
            return
        if cur in ((sb, False), tgt):
            st.f[p] = tgt
            if sl:
                st.track[p] = "clean"
        else:
            c["conflicts"] += 1  # MODIFIED_MODIFIED or REMOVED_MODIFIED
        return
    # end
    if sl:
        st.sapling = T
        st.updatestate = None
    if c["resume"]:
        st.interrupted = None
    st.active.remove(aid)


def _operation(  # noqa: C901
    st: St, aid: str, kind: str, pl: dict[str, Any]
) -> tuple[str, dict[str, Any]]:
    """Apply one namespace or observer action at its point; return its result."""
    if kind == "write":
        p = pl["path"]
        cur = st.f.get(p)
        if cur is None:
            return efail(E.ENOENT)
        if cur[0] == DIR:
            return efail(E.EISDIR)
        st.f[p] = (value(aid, pl["value_id"]), cur[1])
        return ("ok", WRITE_OK)
    if kind == "read":
        cur = st.f.get(pl["path"])
        if cur is None:
            return efail(E.ENOENT)
        if cur[0] == DIR:
            return efail(E.EISDIR)
        return ("ok", read_ok(cur[0]))
    if kind == "getattr":
        cur = st.f.get(pl["path"])
        if cur is None:
            return efail(E.ENOENT)
        kind_name = "directory" if cur[0] == DIR else "file"
        return ("ok", attrs(kind_name, bool(cur[1]) and cur[0] != DIR))
    if kind == "unlink":
        cur = st.f.get(pl["path"])
        if cur is None:
            return efail(E.ENOENT)
        if cur[0] == DIR:
            return efail(E.EISDIR)
        del st.f[pl["path"]]
        return ("ok", {"path": pl["path"]})
    if kind == "rename":
        s, d = pl["source"], pl["destination"]
        cs, cd = st.f.get(s), st.f.get(d)
        if cs is None:
            return efail(E.ENOENT)
        if cd is not None and cd[0] == DIR and cs[0] != DIR:
            return efail(E.EISDIR)
        st.f[d] = st.f.pop(s)
        return ("ok", {"source": s, "destination": d})
    if kind == "chmod":
        p = pl["path"]
        cur = st.f.get(p)
        if cur is None:
            return efail(E.ENOENT)
        st.f[p] = (cur[0], pl["mode"] == 0o755 and cur[0] != DIR)
        return ("ok", {"path": p, "mode": pl["mode"]})
    if kind == "mkdir":
        p = pl["path"]
        if p in st.f:
            return efail(E.EEXIST)
        st.f[p] = (DIR, False)
        return ("ok", {"path": p})
    if kind == "sl_rm":
        # `sl rm --force`: a register is in every revision's
        # manifest, so it is tracked unless an earlier sl rm/mv marked it
        # removed. A file already marked removed is untouched even when the
        # path was re-created on disk (`cmdutil.remove` acts only on
        # modified, deleted, clean, and added files), and exits 0 too.
        p = pl["path"]
        if st.track.get(p) != "removed":
            st.f.pop(p, None)
            st.track[p] = "removed"
        return ("ok", {"path": p})
    if kind == "sl_mv":
        # `sl mv`: a source marked removed has no tracked file (exit 255);
        # a tracked source absent from disk is only marked removed; an
        # existing destination file leaves everything as it was.
        s, d = pl["source"], pl["destination"]
        if st.track.get(s) == "removed":
            return (
                "fail",
                {
                    "reason": "sl_mv: exit 255",
                    "returncode": 255,
                    "stdout": "",
                    "stderr": "abort: no files to copy\n",
                },
            )
        if s not in st.f:
            st.track[s] = "removed"
        elif d not in st.f:
            st.f[d] = st.f.pop(s)
            st.track[s] = "removed"
            st.track[d] = "added"
        return ("ok", {"source": s, "destination": d})
    if kind == "status":
        refused = None
        if st.active:
            refused = STATUS_IN_PROGRESS
        elif st.interrupted is not None:
            refused = INTERRUPTED.format(rev=st.interrupted[1])
        if refused is not None:
            return (
                "fail",
                {
                    "reason": refused,
                    "source": "thrift",
                    "error_type": "CHECKOUT_IN_PROGRESS",
                },
            )
        if st.eden != pl["revision"]:
            return (
                "fail",
                {
                    "reason": "out of date",
                    "source": "thrift",
                    "error_type": "OUT_OF_DATE_PARENT",
                },
            )
        return ("ok", thrift_status(thrift_codes(st), st.eden))
    assert kind == "readdir"
    names = sorted(p.rsplit("/", 1)[1] for p in st.f if p.startswith("tree/registers/"))
    return ("ok", {"channel": pl["channel"], "entries": names})


def _checkout_result(
    c: dict[str, Any], pl: dict[str, Any], mutate: str | None
) -> tuple[str, dict[str, Any]]:
    T, via = pl["revision"], pl["via"]
    if c["rejected"] == "in-progress":
        if via == "thrift":
            return ("fail", thrift_rejected())
        return _sl_fail(f"abort: EdenError: {IN_PROGRESS}\n")
    if c["rejected"] == "interrupted":
        advice = INTERRUPTED.format(rev=c["interrupted"])
        if via == "thrift":
            return (
                "fail",
                {
                    "reason": advice,
                    "source": "thrift",
                    "error_type": "CHECKOUT_IN_PROGRESS",
                },
            )
        return _sl_fail(f"abort: EdenError: {advice}\n")
    if c["rejected"] == "not-interrupted":
        return _sl_fail("abort: not in an interrupted goto state\n")
    if c["abort"]:
        return ("fail", sl_abort(c["abort"]))
    if c["failed"]:
        if via == "thrift":
            return (
                "fail",
                {
                    "reason": "thrift: fault",
                    "source": "thrift",
                    "error_type": "GENERIC_ERROR",
                },
            )
        return _sl_fail("abort: EdenError: std::runtime_error: fault\n")
    if via == "thrift":
        planted = mutate == "count" and not c["resume"]
        count = c["conflicts"] + (1 if planted else 0)
        return ("ok", thrift_checkout(T, conflicts=count))
    return ("ok", sl_checkout(T, via=via))


def simulate(
    seed: int, mutate: str | None = None, points_seed: int | None = None
) -> Run:
    """Simulate one legal history (see the module docstring).

    `points_seed` re-samples only the server points inside each recorded
    window (checkout begin, per-path visits, phases, interruption, end;
    every other action's point) from its own generator: same actions,
    same windows, possibly other results. None keeps the seed's own points.
    """
    rng = random.Random(seed)
    run = Run("gen", f"n8g{seed}", "n8 generated")
    shape = rng.choice(SHAPES)
    interrupted = shape.endswith("-int")
    acts = _actions(rng, shape)
    # phase B observers
    bacts: list[tuple[str, str, dict[str, Any]]] = []
    for j, p in enumerate((*REGS, MOVED, NEW_DIR)):
        known = p in REGS or any(
            (k in ("rename", "sl_mv") and p in pl.values())
            or (k == "mkdir" and pl["path"] == p)
            for _, k, pl in acts
        )
        if not known:
            continue
        if p != NEW_DIR:
            bacts.append((f"B{j}r", "read", {"path": p, "channel": "fuse"}))
        bacts.append((f"B{j}g", "getattr", {"path": p}))
    bacts.append(("Bl1", "readdir", {"path": "tree/registers", "channel": "fuse"}))
    bacts.append(("Bl2", "readdir", {"path": "tree/registers", "channel": "thrift"}))
    for aid, kind, payload in acts:
        run.act(aid, aid, kind, **payload)
    for aid, kind, payload in bacts:
        run.act(
            aid, aid, kind, requires=[(a, "finished") for a, _, _ in acts], **payload
        )
    win = _windows(rng, acts, bacts, interrupted)
    kinds: dict[str, tuple[str, dict[str, Any]]] = {
        a: (k, p) for a, k, p in acts + bacts
    }
    prng = rng if points_seed is None else random.Random(points_seed)
    st = St()
    res: dict[str, tuple[str, dict[str, Any]]] = {}
    co: dict[str, dict[str, Any]] = {}
    for _t, _o, aid, part, extra in _points(prng, kinds, win, interrupted):
        kind, pl = kinds[aid]
        if kind == "checkout":
            c = co.setdefault(
                aid,
                {
                    "rejected": None,
                    "conflicts": 0,
                    "blocked": [],
                    "abort": None,
                    "dead": False,
                    "failed": False,
                    "resume": False,
                },
            )
            _checkout_point(st, c, aid, pl, part, extra)
        else:
            res[aid] = _operation(st, aid, kind, pl)
    for aid, c in co.items():
        res[aid] = _checkout_result(c, kinds[aid][1], mutate)
    events = []
    for aid in kinds:
        lo, hi = win[aid]
        events.append(Event("invoke", lo, {"action": aid, "actor": aid}))
        outcome, facts = res[aid]
        events.append(Event(outcome, hi, {"action": aid, "actor": aid, **facts}))
    events.sort(key=lambda e: e.time)
    run.events = events
    run.clock = int(max(e.time for e in events) * 10) + 10
    run.finish()
    return run


WITNESS_POINTS = 10_000_000


def count_witness(seed: int, trials: int) -> int | None:
    """Return a points seed under which the planted count is itself legal, or None.

    The planted history (`mutate="count"`) is legal exactly when some
    choice of server points inside the same windows reproduces every
    recorded result, the planted conflict counts included. Tries points
    seeds `WITNESS_POINTS + t` for t < `trials`; the first whose events
    equal the planted history's is returned (as t).
    """
    planted = simulate(seed, "count").events
    for trial in range(trials):
        if simulate(seed, points_seed=WITNESS_POINTS + trial).events == planted:
            return trial
    return None


def judge(
    seed: int, mutate: str | None = None, files: int | None = None
) -> Verdict | str | None:
    """Judge one simulated history with `EDENFS_TARGET`.

    A refused spec gives the task it waits on, or None for an unmodeled
    shape. `files` overrides the repository's file count (sparse-scale runs).
    """
    try:
        run = simulate(seed, mutate)
        document = json.loads(run.spec_text())
        if files is not None:
            document["environment"]["target"]["repository"]["file_count"] = files
        spec = from_json(json.dumps(document), EDENFS_TARGET)
    except SpecError as error:
        return error.waiting_on
    return check(EDENFS_TARGET, spec, run.events, execution={"status": "completed"})
