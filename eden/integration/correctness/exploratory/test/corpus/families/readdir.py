# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `readdir`: directory streams and listings across checkouts and
mkdirs."""

from __future__ import annotations

import errno
from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    MOVED,
    NOT_EVALUATED,
    R0,
    R1,
    R2,
    read_ok,
    Run,
    SATISFIED,
    thrift_checkout,
    TIP,
    VIOLATED,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    UNDETERMINED,
)


WIDE = "tree/wide"


def _stream(
    name: str,
    description: str,
    *,
    listing: str,
    churn: bool = True,
    mode: str = "normal",
    checkout_first: bool = False,
) -> Run:
    run = Run(
        "readdir",
        name,
        description,
        basis=[
            "POSIX readdir returns each entry left unmodified across a stream "
            "exactly once, also when the stream resumes across a checkout "
            "(`TreeInode::readdirImpl`, eden/fs/inodes/TreeInode.cpp)",
        ],
        wide_dir={"name": WIDE, "entries": 4, "churn_all": churn},
    )
    names = [entry.path.removeprefix(WIDE + "/") for entry in run.layout().wide_files()]
    order = ["c", "d0", "d1"] if checkout_first else ["d0", "c", "d1"]
    for anchor in order:
        if anchor == "c":
            run.act("a", "c", "checkout", revision=1, via="thrift", mode=mode)
        elif anchor == "d0":
            run.act("a", "d0", "readdir_stream", path=WIDE, pause_after=2)
        else:
            run.act("a", "d1", "readdir_resume", stream="d0")
    for anchor in order[:2]:
        if anchor == "c":
            run.call("c", "ok", **thrift_checkout(1, conflicts=0))
        else:
            run.call("d0", "ok", pgid=4244, paused_at=2, pos_at_pause=2)
    entries = {
        "complete": names,
        "duplicate": names + names[:1],
        "duplicate-untracked": names + names[1:2],
        "thrice": names + names[:1] + names[:1],
        "foreign": names[:-1] + ["ghost.dat"],
    }[listing]
    run.call(
        "d1",
        "ok",
        stream="d0",
        entries=entries,
        pos_at_pause=2,
        pos_at_end=len(entries),
    )
    run.finish()
    return run


def readdir_runs() -> list[Run]:
    run = _stream(
        "stream-across-checkout-complete",
        "A wide-directory stream paused across a checkout lists every name once.",
        listing="complete",
    )
    for rule, reason in (
        ("readdir.duplicate-entry", "no name repeats"),
        ("readdir.unknown-entry", "every name is in every revision"),
        ("readdir.missing-entry", "every always-present name appears"),
    ):
        run.expect(rule, "d1", SATISFIED, reason, path=WIDE)
    run2 = _stream(
        "stream-across-checkout-duplicate",
        "The same stream lists its first name again after the checkout, which "
        "replaced that entry: EdenFS resumes by inode number and the replaced "
        "entry has a new, higher inode, so it may be listed under both.",
        listing="duplicate",
    )
    run2.expect(
        "readdir.missing-entry",
        "d1",
        SATISFIED,
        "every always-present name appears",
        path=WIDE,
    )
    unchanged = _stream(
        "stream-across-checkout-duplicate-unchanged",
        "A stream over entries every revision holds unchanged lists its first "
        "name again after the checkout.",
        listing="duplicate",
        churn=False,
    )
    unchanged.expect(
        "readdir.duplicate-entry",
        "d1",
        VIOLATED,
        "the resumed stream repeats a name the checkout left in place",
        path=WIDE,
        attribution=UNDETERMINED,
    )
    run3 = _stream(
        "stream-across-checkout-foreign-name",
        "The same stream drops its last name and lists one no revision has.",
        listing="foreign",
    )
    run3.expect(
        "readdir.unknown-entry",
        "d1",
        VIOLATED,
        "ghost.dat is in no revision and no mkdir made it",
        path=WIDE,
        attribution=UNDETERMINED,
    )
    run3.expect(
        "readdir.missing-entry",
        "d1",
        VIOLATED,
        "the last name is in every revision",
        path=WIDE,
        attribution=UNDETERMINED,
    )
    forced = _stream(
        "stream-across-force-checkout-duplicate-unchanged",
        "The same unchanged-entry stream repeats its first name across a FORCE "
        "checkout, which leaves an entry both trees agree on in place.",
        listing="duplicate",
        churn=False,
        mode="force",
    )
    forced.expect(
        "readdir.duplicate-entry",
        "d1",
        VIOLATED,
        "FORCE leaves an unmodified entry the two trees agree on alone "
        "(`canShortCircuitCheckout`), so its inode is unchanged",
        path=WIDE,
        attribution=UNDETERMINED,
    )
    before = _stream(
        "stream-after-checkout-duplicate",
        "The checkout replaces the first entry and completes before the stream "
        "begins; the stream lists that name twice.",
        listing="duplicate",
        checkout_first=True,
    )
    before.expect(
        "readdir.duplicate-entry",
        "d1",
        VIOLATED,
        "no replacement meets the stream's span, so the name has one inode",
        path=WIDE,
        attribution=UNDETERMINED,
    )
    thrice = _stream(
        "stream-across-checkout-triplicate",
        "The stream lists its first name three times across one checkout that "
        "replaced it.",
        listing="thrice",
    )
    thrice.expect(
        "readdir.duplicate-entry",
        "d1",
        VIOLATED,
        "one replacement gives the name at most two inodes",
        path=WIDE,
        attribution=UNDETERMINED,
    )
    untracked = _stream(
        "stream-across-checkout-duplicate-untracked",
        "The stream lists again, after the checkout, a name no action names "
        "and the revisions disagree on: the checkout replaced that entry too.",
        listing="duplicate-untracked",
    )
    untracked.expect(
        "readdir.duplicate-entry",
        "d1",
        SATISFIED,
        "the checkout meeting the span replaced the entry",
        path=WIDE,
    )
    untracked_unchanged = _stream(
        "stream-across-force-checkout-duplicate-untracked-unchanged",
        "The stream repeats a name no action names across a FORCE checkout "
        "between revisions that agree on it.",
        listing="duplicate-untracked",
        churn=False,
        mode="force",
    )
    untracked_unchanged.expect(
        "readdir.duplicate-entry",
        "d1",
        VIOLATED,
        "FORCE leaves an entry the two trees agree on alone",
        path=WIDE,
        attribution=UNDETERMINED,
    )
    untracked_before = _stream(
        "stream-after-checkout-duplicate-untracked",
        "The checkout completes before the stream begins; the stream lists a "
        "name no action names twice.",
        listing="duplicate-untracked",
        checkout_first=True,
    )
    untracked_before.expect(
        "readdir.duplicate-entry",
        "d1",
        VIOLATED,
        "no replacement meets the stream's span",
        path=WIDE,
        attribution=UNDETERMINED,
    )
    return [
        run,
        run2,
        unchanged,
        forced,
        before,
        thrice,
        untracked,
        untracked_unchanged,
        untracked_before,
        run3,
    ]


READDIR = "edenfs/properties/readdir.py; a listing is no snapshot"

WIDE_DIR = {"name": WIDE, "entries": 4, "churn_all": True}


def _made(name: str, description: str, *, listed: bool) -> Run:
    run = Run("readdir", name, description, basis=[READDIR], wide_dir=WIDE_DIR)
    names = [entry.path.removeprefix(WIDE + "/") for entry in run.layout().wide_files()]
    run.act("a", "m", "mkdir", path=f"{WIDE}/sub")
    run.act("a", "d0", "readdir_stream", path=WIDE, pause_after=2)
    run.act("a", "d1", "readdir_resume", stream="d0")
    run.call("m", "ok", path=f"{WIDE}/sub")
    run.call("d0", "ok", pgid=4244, paused_at=2, pos_at_pause=2)
    entries = names + (["sub"] if listed else [])
    run.call(
        "d1",
        "ok",
        stream="d0",
        entries=entries,
        pos_at_pause=2,
        pos_at_end=len(entries),
    )
    run.finish()
    return run


def made_directory_runs() -> list[Run]:
    listed = _made(
        "stream-lists-made-directory",
        "mkdir makes tree/wide/sub before a wide-directory stream, which lists it "
        "with every generated name.",
        listed=True,
    )
    for rule, reason in (
        ("readdir.unknown-entry", "sub is the acked mkdir's"),
        ("readdir.missing-entry", "every bound name appears"),
        ("readdir.duplicate-entry", "no name repeats"),
    ):
        listed.expect(rule, "d1", SATISFIED, reason, path=WIDE)
    omitted = _made(
        "stream-omits-made-directory",
        "mkdir makes tree/wide/sub before a wide-directory stream, which omits it.",
        listed=False,
    )
    omitted.expect(
        "readdir.missing-entry",
        "d1",
        VIOLATED,
        "the acked mkdir bound sub before the stream started, and nothing removed it",
        path=WIDE,
        attribution="undetermined",
    )
    return [listed, omitted]


def collector_runs() -> list[Run]:
    found: list[Run] = []

    run = Run(
        "readdir",
        "entry-that-is-no-name-is-unknown",
        "An entry that is no name is unknown.",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
        wide_dir={"name": "tree/wide", "entries": 2, "churn_all": False},
    )
    run.act("a", "d0", "readdir_stream", path="tree/wide", pause_after=1)
    run.act("a", "d1", "readdir_resume", stream="d0")
    run.call("d0", "ok", pgid=1, paused_at=1, pos_at_pause=1)
    run.call(
        "d1",
        "ok",
        stream="d0",
        entries=[
            "000000-entry-whose-long-name-spreads-the-listing-over-several-responses.dat",
            "000001-entry-whose-long-name-spreads-the-listing-over-several-responses.dat",
            "a/b",
        ],
        pos_at_pause=1,
        pos_at_end=3,
    )
    run.finish()
    run.expect(
        "readdir.unknown-entry",
        "d1",
        VIOLATED,
        "readdir/fuse: unknown-entry: the listing names 'a/b', which nothing binds in tree/wide over its span",
        path="tree/wide",
        identity={},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "readdir",
        "rebound-but-never-unbound-entry-is-required",
        "The relevant representative entry changes bytes at the checkout yet stays bound.",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
        wide_dir={"name": "tree/wide", "entries": 4, "churn_all": True},
    )
    run.act("a", "d0", "readdir_stream", path="tree/wide", pause_after=2)
    run.act("a", "c", "checkout", revision=1, via="thrift", mode="normal")
    run.act("a", "d1", "readdir_resume", stream="d0")
    run.call("d0", "ok", pgid=1, paused_at=2, pos_at_pause=2)
    run.call("c", "ok", **thrift_checkout(1, conflicts=0))
    run.call(
        "d1",
        "ok",
        stream="d0",
        entries=[
            "000001-entry-whose-long-name-spreads-the-listing-over-several-responses.dat",
            "000002-entry-whose-long-name-spreads-the-listing-over-several-responses.dat",
            "000003-entry-whose-long-name-spreads-the-listing-over-several-responses.dat",
        ],
        pos_at_pause=2,
        pos_at_end=3,
    )
    run.finish()
    run.expect(
        "readdir.missing-entry",
        "d1",
        VIOLATED,
        "readdir/fuse: missing-entry: the listing omits 'tree/wide/000000-entry-whose-long-name-spreads-the-listing-over-several-responses.dat', which the directory binds throughout its span",
        path="tree/wide",
        identity={},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "readdir",
        "entry-absent-throughout-the-stream-is-unknown",
        "A mkdir binds `sub` only after the stream drained, so listing it is unknown.",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
        wide_dir={"name": "tree/wide", "entries": 2, "churn_all": False},
    )
    run.act("a", "d0", "readdir_stream", path="tree/wide", pause_after=1)
    run.act("a", "d1", "readdir_resume", stream="d0")
    run.act("a", "m", "mkdir", path="tree/wide/sub")
    run.call("d0", "ok", pgid=1, paused_at=1, pos_at_pause=1)
    run.call(
        "d1",
        "ok",
        stream="d0",
        entries=[
            "000000-entry-whose-long-name-spreads-the-listing-over-several-responses.dat",
            "000001-entry-whose-long-name-spreads-the-listing-over-several-responses.dat",
            "sub",
        ],
        pos_at_pause=1,
        pos_at_end=3,
    )
    run.call("m", "ok", path="tree/wide/sub")
    run.finish()
    run.expect(
        "readdir.unknown-entry",
        "d1",
        VIOLATED,
        "readdir/fuse: unknown-entry: the listing names 'sub', which nothing binds in tree/wide over its span",
        path="tree/wide",
        identity={},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "readdir",
        "fuse-listing-beside-an-unrelated-read-is-rejected",
        "One FUSE listing of a name nothing binds.",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("a", "l0", "readdir", path="tree/registers", channel="fuse")
    run.act("a", "r", "read", requires=[("l0", "finished")], path=R1, channel="fuse")
    run.call("l0", "ok", channel="fuse", entries=["bogus-name"])
    run.call("r", "ok", **read_ok(committed(R1, 3)))
    run.finish()
    run.expect(
        "readdir.missing-entry",
        "l0",
        VIOLATED,
        "readdir/fuse: missing-entry: the listing omits 'tree/registers/r0.reg', which the directory binds throughout its span",
        path="tree/registers",
        identity={},
        attribution="undetermined",
    )
    run.expect(
        "readdir.unknown-entry",
        "l0",
        VIOLATED,
        "readdir/fuse: unknown-entry: the listing names 'bogus-name', which nothing binds in tree/registers over its span",
        path="tree/registers",
        identity={},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "readdir",
        "legal-one-shot-listings-are-accepted-one-shot-fuse",
        "Legal one shot listings are accepted (one-shot listings).",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("a", "l0", "readdir", path="tree/registers", channel="fuse")
    run.call("l0", "ok", channel="fuse", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.finish()
    for rule, reason in (
        ("readdir.duplicate-entry", "no duplicate entry"),
        ("readdir.missing-entry", "no missing entry"),
        ("readdir.unknown-entry", "no unknown entry"),
    ):
        run.expect(rule, "l0", SATISFIED, reason, path="tree/registers")
    found.append(run)

    run = Run(
        "readdir",
        "legal-one-shot-listings-are-accepted-one-shot-thrift",
        "Legal one shot listings are accepted (one-shot listings).",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("a", "l0", "readdir", path="tree/registers", channel="thrift")
    run.call("l0", "ok", channel="thrift", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.finish()
    for rule, reason in (
        ("readdir.duplicate-entry", "no duplicate entry"),
        ("readdir.missing-entry", "no missing entry"),
        ("readdir.unknown-entry", "no unknown entry"),
    ):
        run.expect(rule, "l0", SATISFIED, reason, path="tree/registers")
    found.append(run)

    run = Run(
        "readdir",
        "legal-one-shot-listings-are-accepted-one-shot-fuse-thrift",
        "Legal one shot listings are accepted (one-shot listings).",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("a", "l0", "readdir", path="tree/registers", channel="fuse")
    run.act("a", "l1", "readdir", path="tree/registers", channel="thrift")
    run.call("l0", "ok", channel="fuse", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.call("l1", "ok", channel="thrift", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.finish()
    for rule, anchor, reason in (
        ("readdir.duplicate-entry", "l0", "no duplicate entry"),
        ("readdir.duplicate-entry", "l1", "no duplicate entry"),
        ("readdir.missing-entry", "l0", "no missing entry"),
        ("readdir.missing-entry", "l1", "no missing entry"),
        ("readdir.unknown-entry", "l0", "no unknown entry"),
        ("readdir.unknown-entry", "l1", "no unknown entry"),
    ):
        run.expect(rule, anchor, SATISFIED, reason, path="tree/registers")
    found.append(run)

    run = Run(
        "readdir",
        "duplicate-one-shot-entry-is-rejected",
        "A duplicate one shot entry is rejected.",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("a", "l0", "readdir", path="tree/registers", channel="thrift")
    run.call(
        "l0", "ok", channel="thrift", entries=["r0.reg", "r1.reg", "r2.reg", "r0.reg"]
    )
    run.finish()
    run.expect(
        "readdir.duplicate-entry",
        "l0",
        VIOLATED,
        "readdir/thrift: duplicate-entry: the listing names 'r0.reg' 2 times",
        path="tree/registers",
        identity={},
        attribution="undetermined",
    )
    found.append(run)

    run = Run(
        "readdir",
        "undecided-one-shot-path-is-judged-by-the-engine-1",
        "A rename races the listing; another observation pins it outside the call (a listing across an in-flight rename).",
        basis=[
            "edenfs/properties/readdir.py: existence.present-after-remove, readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("b", "m", "rename", source=R0, destination=MOVED)
    run.act("c", "g", "getattr", path=R0)
    run.act("a", "l0", "readdir", path="tree/registers", channel="fuse")
    run.inv("m", "l0")
    run.done("l0", "ok", channel="fuse", entries=["r1.reg", "r2.reg", "moved.reg"])
    run.call(
        "g",
        "ok",
        mtime=1.0,
        mtime_history=1.0,
        size=64,
        mode=420,
        fuse_type="file",
        thrift_type="file",
    )
    run.done("m", "ok", source=R0, destination=MOVED)
    run.finish()
    run.expect(
        "existence.present-after-remove",
        "g",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={"observation-kind": "getattr"},
    )
    run.expect(
        "readdir.missing-entry",
        "l0",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path="tree/registers",
        identity={},
    )
    run.expect(
        "readdir.unknown-entry",
        "l0",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path="tree/registers",
        identity={},
    )
    found.append(run)

    run = Run(
        "readdir",
        "undecided-one-shot-path-is-judged-by-the-engine-2",
        "A rename races the listing; another observation pins it outside the call (a listing across an in-flight rename).",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("b", "m", "rename", source=R0, destination=MOVED)
    run.act("c", "g", "getattr", path=R0)
    run.act("a", "l0", "readdir", path="tree/registers", channel="fuse")
    run.inv("m", "l0")
    run.done("l0", "ok", channel="fuse", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.call(
        "g",
        "ok",
        mtime=1.0,
        mtime_history=1.0,
        size=64,
        mode=420,
        fuse_type="file",
        thrift_type="file",
    )
    run.done("m", "ok", source=R0, destination=MOVED)
    run.finish()
    for rule, reason in (
        ("readdir.duplicate-entry", "no duplicate entry"),
        ("readdir.missing-entry", "no missing entry"),
        ("readdir.unknown-entry", "no unknown entry"),
    ):
        run.expect(rule, "l0", SATISFIED, reason, path="tree/registers")
    found.append(run)

    run = Run(
        "readdir",
        "undecided-one-shot-path-is-judged-by-the-engine-3",
        "A rename races the listing; another observation pins it outside the call (a listing across an in-flight rename).",
        basis=[
            "edenfs/properties/readdir.py: existence.missing-after-create, readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("b", "m", "rename", source=R0, destination=MOVED)
    run.act("c", "g", "getattr", path=R0)
    run.act("a", "l0", "readdir", path="tree/registers", channel="fuse")
    run.inv("m", "g")
    run.done("g", "fail", **errno_fail(errno.ENOENT, "stat"))
    run.call("l0", "ok", channel="fuse", entries=["r0.reg", "r1.reg", "r2.reg"])
    run.done("m", "ok", source=R0, destination=MOVED)
    run.finish()
    run.expect(
        "existence.missing-after-create",
        "g",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path=R0,
        identity={},
    )
    run.expect(
        "readdir.missing-entry",
        "l0",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path="tree/registers",
        identity={},
    )
    run.expect(
        "readdir.unknown-entry",
        "l0",
        VIOLATED,
        "no explanation of the run's effects allows the observations",
        path="tree/registers",
        identity={},
    )
    found.append(run)

    run = Run(
        "readdir",
        "undecided-one-shot-path-is-judged-by-the-engine-4",
        "A rename races the listing; another observation pins it outside the call (a listing across an in-flight rename).",
        basis=[
            "edenfs/properties/readdir.py: readdir.duplicate-entry, readdir.missing-entry, readdir.unknown-entry"
        ],
    )
    run.act("b", "m", "rename", source=R0, destination=MOVED)
    run.act("c", "g", "getattr", path=R0)
    run.act("a", "l0", "readdir", path="tree/registers", channel="fuse")
    run.inv("m", "g")
    run.done("g", "fail", **errno_fail(errno.ENOENT, "stat"))
    run.call("l0", "ok", channel="fuse", entries=["r1.reg", "r2.reg", "moved.reg"])
    run.done("m", "ok", source=R0, destination=MOVED)
    run.finish()
    for rule, reason in (
        ("readdir.duplicate-entry", "no duplicate entry"),
        ("readdir.missing-entry", "no missing entry"),
        ("readdir.unknown-entry", "no unknown entry"),
    ):
        run.expect(rule, "l0", SATISFIED, reason, path="tree/registers")
    found.append(run)
    return found


MEMBERSHIP = (
    "Membership the engine decides (one-shot listings by observation, "
    "streams by a satisfiability probe over the span)"
)
WIDE_PAIR = {"name": WIDE, "entries": 2, "churn_all": False}
REGISTERS = "tree/registers"
X = f"{WIDE}/x"
SUB = f"{WIDE}/sub"


def _membership_stream(
    name: str,
    description: str,
    mutation: tuple[str, str, dict[str, Any], str],
    *,
    before: bool,
    extra: tuple[str, ...] = (),
) -> Run:
    """A two-entry wide-directory stream and one mutation on its own actor.

    `mutation` is (action id, kind, payload, outcome); it completes before the
    stream when `before`, else it is invoked after the stream completes. The
    stream lists the generated names plus `extra`.
    """
    run = Run("readdir", name, description, basis=[MEMBERSHIP], wide_dir=WIDE_PAIR)
    names = [entry.path.removeprefix(WIDE + "/") for entry in run.layout().wide_files()]
    action, kind, payload, outcome = mutation
    run.act("a" if before else "b", action, kind, **payload)
    run.act("c", "d0", "readdir_stream", path=WIDE, pause_after=1)
    run.act("c", "d1", "readdir_resume", stream="d0")
    facts = payload if outcome == "ok" else {"reason": "timed out"}
    if before:
        run.call(action, outcome, **facts)
    run.call("d0", "ok", pgid=1, paused_at=1, pos_at_pause=1)
    entries = [*names, *extra]
    run.call(
        "d1",
        "ok",
        stream="d0",
        entries=entries,
        pos_at_pause=1,
        pos_at_end=len(entries),
    )
    if not before:
        run.call(action, outcome, **facts)
    run.finish()
    return run


def _membership_listing(
    name: str,
    description: str,
    mutation: tuple[str, str, dict[str, Any]],
    channel: str,
    entries: list[str],
) -> Run:
    """An acked mutation of r0, then a one-shot listing of the registers' directory."""
    run = Run("readdir", name, description, basis=[MEMBERSHIP])
    action, kind, payload = mutation
    run.act("a", action, kind, **payload)
    run.act("c", "l", "readdir", path=REGISTERS, channel=channel)
    run.call(action, "ok", **payload)
    run.call("l", "ok", channel=channel, entries=entries)
    run.finish()
    return run


def _membership(
    run: Run, anchor: str, directory: str, violated: str | None = None, **kwargs: str
) -> Run:
    """Declare the three membership rules: `violated` with `kwargs`, the rest satisfied."""
    for rule in (
        "readdir.duplicate-entry",
        "readdir.missing-entry",
        "readdir.unknown-entry",
    ):
        if rule == violated:
            run.expect(
                rule, anchor, VIOLATED, kwargs.pop("reason"), path=directory, **kwargs
            )
        else:
            run.expect(
                rule, anchor, SATISFIED, "the engine admits the listing", path=directory
            )
    return run


def membership_runs() -> list[Run]:
    """Membership the engine decides."""
    rename_in = ("m", "rename", {"source": R0, "destination": X}, "ok")
    unlink = ("u", "unlink", {"path": R0})
    rename = ("m", "rename", {"source": R0, "destination": MOVED})
    registers = ["r0.reg", "r1.reg", "r2.reg"]
    s7 = _membership_stream(
        "stream-omits-indeterminate-made-directory",
        "mkdir tree/wide/sub times out before a stream that omits sub.",
        ("k", "mkdir", {"path": SUB}, "info"),
        before=True,
    )
    for rule in ("readdir.duplicate-entry", "readdir.unknown-entry"):
        s7.expect(rule, "d1", SATISFIED, "the engine admits the listing", path=WIDE)
    s7.expect(
        "readdir.missing-entry",
        "d1",
        NOT_EVALUATED,
        "the timed-out mkdir may bind sub before, during, or never; no single claim "
        "over the span holds in every explanation",
        path=WIDE,
        not_evaluated_reason="readdir.membership-undecided",
    )
    return [
        _membership(
            _membership_stream(
                "stream-lists-renamed-destination",
                "r0 is renamed to tree/wide/x before a stream that lists x.",
                rename_in,
                before=True,
                extra=("x",),
            ),
            "d1",
            WIDE,
        ),
        _membership(
            _membership_stream(
                "stream-omits-renamed-destination",
                "r0 is renamed to tree/wide/x before a stream that omits x.",
                rename_in,
                before=True,
            ),
            "d1",
            WIDE,
            "readdir.missing-entry",
            reason="the acked rename bound x before the stream started",
            attribution=UNDETERMINED,
        ),
        _membership(
            _membership_stream(
                "stream-lists-later-rename-destination",
                "A stream lists tree/wide/x, which a rename invoked after it makes.",
                rename_in,
                before=False,
                extra=("x",),
            ),
            "d1",
            WIDE,
            "readdir.unknown-entry",
            reason="nothing binds x until after the stream completed",
            attribution=UNDETERMINED,
        ),
        _membership(
            _membership_stream(
                "stream-omits-later-made-directory",
                "A stream omits tree/wide/sub, which a mkdir invoked after it makes.",
                ("k", "mkdir", {"path": SUB}, "ok"),
                before=False,
            ),
            "d1",
            WIDE,
        ),
        s7,
        _membership(
            _membership_listing(
                "one-shot-fuse-lists-unlinked-register",
                "r0 is unlinked; a FUSE listing of the registers lists it.",
                unlink,
                "fuse",
                registers,
            ),
            "l",
            REGISTERS,
            "readdir.unknown-entry",
            reason="the acked unlink removed r0 before the listing",
        ),
        _membership(
            _membership_listing(
                "one-shot-fuse-omits-unlinked-register",
                "r0 is unlinked; a FUSE listing of the registers omits it.",
                unlink,
                "fuse",
                registers[1:],
            ),
            "l",
            REGISTERS,
        ),
        _membership(
            _membership_listing(
                "one-shot-thrift-omits-renamed-destination",
                "r0 is renamed to moved.reg; a Thrift listing omits moved.reg.",
                rename,
                "thrift",
                registers[1:],
            ),
            "l",
            REGISTERS,
            "readdir.missing-entry",
            reason="the acked rename bound moved.reg before the listing",
        ),
        _membership(
            _membership_listing(
                "one-shot-thrift-lists-renamed-destination",
                "r0 is renamed to moved.reg; a Thrift listing lists it.",
                rename,
                "thrift",
                [*registers[1:], "moved.reg"],
            ),
            "l",
            REGISTERS,
        ),
        *retype_listing_runs(),
    ]


def retype_listing_runs() -> list[Run]:
    """A checkout replacing a directory with a file unbinds the name before binding the file."""
    runs = []
    for during in (True, False):
        run = Run(
            "readdir",
            f"one-shot-fuse-omits-retyped-file-{'during' if during else 'after'}-checkout",
            "r2 is a directory at rev3 and a file at rev2. A FUSE listing of the "
            "registers omits r2 "
            + (
                "while a Thrift NORMAL checkout to rev2 runs."
                if during
                else "after a Thrift NORMAL checkout to rev2 completed."
            ),
            basis=[
                MEMBERSHIP,
                "TreeInode::finalizeDirectoryRemoval removes the directory and "
                "inserts the file under two contents locks",
            ],
            type_changes=[{"path": R2, "at_revision": TIP}],
        )
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.act("b" if during else "a", "l", "readdir", path=REGISTERS, channel="fuse")
        if during:
            run.inv("c")
            run.call("l", "ok", channel="fuse", entries=["r0.reg", "r1.reg"])
            run.done("c", "ok", **thrift_checkout(2, conflicts=0))
        else:
            run.call("c", "ok", **thrift_checkout(2, conflicts=0))
            run.call("l", "ok", channel="fuse", entries=["r0.reg", "r1.reg"])
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
            runs.append(_membership(run, "l", REGISTERS))
        else:
            runs.append(
                _membership(
                    run,
                    "l",
                    REGISTERS,
                    "readdir.missing-entry",
                    reason="the acked checkout bound the file before the listing",
                )
            )
    return runs


def rename_over_stream_runs() -> list[Run]:
    """A rename onto a wide entry while a two-request stream runs, or after it."""
    found = []
    for during in (True, False):
        run = Run(
            "readdir",
            (
                "stream-skips-name-renamed-over-between-requests"
                if during
                else "stream-omits-name-renamed-over-after-it"
            ),
            "r1 is renamed onto the first wide entry "
            + (
                "between a stream's two requests; the resumed stream omits that "
                "name: it now carries r1's inode, which sorts before the resume "
                "offset."
                if during
                else "only after a two-request stream finished; the stream omits "
                "that name anyway."
            ),
            basis=[
                "EdenFS resumes a listing at an inode-number offset "
                "(`TreeInode::readdir`); rename keeps the source's inode"
            ],
            wide_dir={"name": WIDE, "entries": 2, "churn_all": False},
        )
        names = [e.path.removeprefix(WIDE + "/") for e in run.layout().wide_files()]
        destination = f"{WIDE}/{names[0]}"
        run.act("c", "d0", "readdir_stream", path=WIDE, pause_after=1)
        if during:
            run.act(
                "b",
                "m",
                "rename",
                source=R1,
                destination=destination,
                requires=[("d0", "finished")],
            )
            run.act(
                "c", "d1", "readdir_resume", stream="d0", requires=[("m", "finished")]
            )
        else:
            run.act("c", "d1", "readdir_resume", stream="d0")
            run.act(
                "b",
                "m",
                "rename",
                source=R1,
                destination=destination,
                requires=[("d1", "finished")],
            )
        run.call("d0", "ok", pgid=1, paused_at=1, pos_at_pause=1)
        if during:
            run.call("m", "ok", source=R1, destination=destination)
        run.call(
            "d1",
            "ok",
            stream="d0",
            entries=names[1:],
            pos_at_pause=1,
            pos_at_end=len(names) - 1,
        )
        if not during:
            run.call("m", "ok", source=R1, destination=destination)
        run.finish()
        if during:
            run.expect(
                "readdir.missing-entry",
                "d1",
                SATISFIED,
                "the name was replaced between the stream's requests",
                path=WIDE,
            )
        else:
            run.expect(
                "readdir.missing-entry",
                "d1",
                VIOLATED,
                "the name was bound, unchanged, throughout the stream",
                path=WIDE,
                attribution=UNDETERMINED,
            )
        found.append(run)
    return found


def listing_runs() -> list[Run]:
    names = sorted(path.rsplit("/", 1)[1] for path in (R0, R1, R2))
    runs = []
    for agree in (True, False):
        run = Run(
            "existence",
            "listing-channels-agree" if agree else "listing-channels-disagree",
            "Two listings of tree/registers, over FUSE then via Thrift, with nothing "
            "in between, " + ("agree." if agree else "differ by r2.reg."),
            basis=["edenfs/properties/listing.py"],
        )
        run.act("a", "f", "readdir", path="tree/registers", channel="fuse")
        run.act("a", "t", "readdir", path="tree/registers", channel="thrift")
        run.call("f", "ok", channel="fuse", entries=names)
        run.call("t", "ok", channel="thrift", entries=names if agree else names[:-1])
        run.finish()
        if agree:
            run.expect(
                "listing.channel-disagreement",
                "t",
                SATISFIED,
                "equal name sets",
                path="tree/registers",
            )
        else:
            run.expect(
                "listing.channel-disagreement",
                "t",
                VIOLATED,
                "no producer or nemesis runs between the listings",
                path="tree/registers",
            )
            # The Thrift listing omits r2.reg, bound throughout: the readdir
            # rules judge each one-shot listing too.
            run.expect(
                "readdir.missing-entry",
                "t",
                VIOLATED,
                "r2.reg is bound throughout the listing",
                path="tree/registers",
                attribution="undetermined",
            )
        runs.append(run)
    return runs


def runs() -> list[Run]:
    return [
        *readdir_runs(),
        *made_directory_runs(),
        *collector_runs(),
        *membership_runs(),
        *rename_over_stream_runs(),
        *listing_runs(),
    ]
