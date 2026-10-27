# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `manifest`: manifest presence probes after checkouts."""

from __future__ import annotations

from eden.integration.correctness.exploratory.edenfs.encoding import manifest_diff
from eden.integration.correctness.exploratory.test.corpus.author import (
    Run,
    SATISFIED,
    thrift_checkout,
    TIP,
    VIOLATED,
)


def _manifest(
    name: str,
    description: str,
    *,
    stale: bool,
    absent: bool = False,
    overlapping: bool = False,
) -> Run:
    run = Run(
        "manifest",
        name,
        description,
        basis=["edenfs/properties/manifest.py; EdenMount.h:1228"],
    )
    deleted, added, _changed = manifest_diff(run.layout(), TIP, 2)
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    probe = "b" if overlapping else "a"
    run.act(probe, "p", "manifest_presence", from_revision=TIP, to_revision=2)
    if overlapping:
        run.inv("c")
    else:
        run.call("c", "ok", **thrift_checkout(2, conflicts=0))
    gone = sorted(deleted)
    run.call(
        "p",
        "ok",
        deleted_presence=[
            {"path": path, "present": stale and index == 0}
            for index, path in enumerate(gone)
        ],
        added_presence=[
            {"path": path, "present": not (absent and index == 0)}
            for index, path in enumerate(sorted(added))
        ],
    )
    if overlapping:
        run.done("c", "ok", **thrift_checkout(2, conflicts=0))
    run.finish()
    return run


def manifest_runs() -> list[Run]:
    run = _manifest(
        "presence-after-checkout",
        "A conflict-free Thrift checkout to rev2; every path rev2 deletes is absent and "
        "every path it adds is present.",
        stale=False,
    )
    run.expect(
        "manifest.checkout-mismatch",
        "p",
        SATISFIED,
        "the acked checkout applied every changed path",
    )
    run2 = _manifest(
        "deleted-path-still-present",
        "The same checkout, yet one path rev2 deletes is still present.",
        stale=True,
    )
    run2.expect(
        "manifest.checkout-mismatch",
        "p",
        VIOLATED,
        "the acked conflict-free checkout removed every path rev2 deletes",
    )
    run3 = _manifest(
        "added-path-absent-after-checkout",
        "The same checkout, yet one path rev2 adds is absent.",
        stale=False,
        absent=True,
    )
    run3.expect(
        "manifest.checkout-mismatch",
        "p",
        VIOLATED,
        "the acked conflict-free checkout added every path rev2 adds",
    )
    run4 = _manifest(
        "added-path-absent-during-checkout",
        "One path rev2 adds is absent to a probe that overlaps the checkout.",
        stale=False,
        absent=True,
        overlapping=True,
    )
    run4.expect(
        "manifest.checkout-mismatch",
        "p",
        SATISFIED,
        "the probe may read the path before the checkout adds it",
    )
    return [run, run2, run3, run4]


def runs() -> list[Run]:
    return [
        *manifest_runs(),
    ]
