# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `task-key`: twins whose canonical error sets share or split a
task key."""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    R0,
    R1,
    R2,
    read_ok,
    Run,
    SATISFIED,
    sl_status,
    TIP,
    value,
    VIOLATED,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.corpus.families.common import (
    inputs_ok,
    omitted,
    stale_read,
)


def _stale(
    name: str,
    description: str,
    *,
    path: str = R0,
    writer: str = "a",
    ids: tuple[str, str] = ("w", "r"),
    value_id: str = "w",
    gap: int = 0,
    redundant: bool = False,
    harmless: bool = False,
    status: bool = False,
    fresh: bool = False,
) -> Run:
    run = Run(
        "task-key",
        name,
        description,
        basis=["the task key a canonical error set maps to"],
    )
    write_id, read_id = ids
    run.act(writer, write_id, "write", path=path, value_id=value_id)
    run.act(writer, read_id, "read", path=path, channel="fuse")
    if redundant:
        run.act(writer, "again", "read", path=path, channel="fuse")
    if status:
        run.act(writer, "s", "status", channel="sl")
    if harmless:
        run.act("other", "hw", "write", path=R2, value_id="hw")
        run.act("other", "hr", "read", path=R2, channel="fuse")
    run.call(write_id, "ok", **WRITE_OK)
    run.wait(gap)
    if fresh:
        run.call(read_id, "ok", **read_ok(value(writer, value_id)))
        stale_read(run, read_id, path, SATISFIED, "the acked write's bytes")
    else:
        run.call(read_id, "ok", **read_ok(committed(path, TIP)))
    reason = "the acked write superseded rev3's bytes before the read"
    if not fresh:
        stale_read(run, read_id, path, VIOLATED, reason)
    inputs_ok(run, write_id, read_id)
    if redundant:
        run.call("again", "ok", **read_ok(committed(path, TIP)))
        stale_read(run, "again", path, VIOLATED, reason)
    if status:
        run.call("s", "ok", **sl_status({}))
        omitted(
            run, "s", path, VIOLATED, "r0 holds the write's bytes", producers="write"
        )
    if harmless:
        run.call("hw", "ok", **WRITE_OK)
        run.call("hr", "ok", **read_ok(value("other", "hw")))
    run.finish()
    return run


def task_key_runs() -> list[Run]:
    return [
        _stale("stale-read-base", "An acked write to r0, then a read of rev3's bytes."),
        _stale(
            "stale-read-renamed-actions",
            "The base case with every action and actor ID renamed.",
            writer="client7",
            ids=("put9", "get9"),
            value_id="w",
        ),
        _stale(
            "stale-read-witness-change",
            "The base case with another value ID and a longer gap.",
            value_id="other-value",
            gap=5,
        ),
        _stale("stale-read-other-path", "The base case on r1 instead of r0.", path=R1),
        _stale(
            "stale-read-redundant-observation",
            "The base case plus a second identical stale read.",
            redundant=True,
        ),
        _stale(
            "stale-read-harmless-independent-action",
            "The base case plus an unrelated legal write and read of r2.",
            harmless=True,
        ),
        _stale(
            "fresh-read-control",
            "Legal control: the base case's read returns the "
            "write's bytes, so no error set exists.",
            fresh=True,
        ),
        _stale(
            "stale-read-plus-status-omission",
            "The base case plus a status omitting r0: "
            "a superset error set, so a different task key.",
            status=True,
        ),
    ]


def twins() -> dict[str, Any]:
    return {
        "schema": "eden-correctness-corpus-twins-v1",
        "groups": [
            {
                "group": "stale-read",
                "base": "task-key/stale-read-base",
                "same_key": {
                    "task-key/stale-read-renamed-actions": "action-ID and actor renaming",
                    "task-key/stale-read-witness-change": "witness change",
                    "task-key/stale-read-other-path": "concrete path change",
                    "task-key/stale-read-redundant-observation": "redundant observation",
                    "task-key/stale-read-harmless-independent-action": "harmless independent action",
                },
                "different_key": {
                    "task-key/stale-read-plus-status-omission": "superset error set",
                },
            },
            {
                "group": "sl-abort-lists-clean-path",
                "base": "checkout/sl-goto-abort-lists-clean-path",
                "same_key": {
                    "checkout/sl-goto-abort-lists-clean-path-beside-blocker": "a "
                    "path that did block is listed too",
                },
                "different_key": {
                    "checkout/sl-goto-missing-removed-succeeds": "the update "
                    "succeeds over a path that blocks",
                },
            },
        ],
    }


def runs() -> list[Run]:
    return [
        *task_key_runs(),
    ]
