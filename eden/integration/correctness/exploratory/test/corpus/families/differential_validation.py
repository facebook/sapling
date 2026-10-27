# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Corpus family `differential-validation`: RunSpecs the differential validator
admits or rejects."""

from __future__ import annotations

import copy
from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import (
    R0,
    Run,
    SpecCase,
)


def _differential_spec(
    actions: dict[str, Any], actors: list[dict[str, Any]]
) -> dict[str, Any]:
    base = Run("differential-validation", "spec", "", reference=True)
    document = base.spec_document()
    document["actions"] = actions
    document["actors"] = actors
    document["metadata"] = {
        "corpus_case": "differential-validation",
        "origin": "synthetic",
    }
    return document


def _op(kind: str, **payload: Any) -> dict[str, Any]:
    return {"kind": kind, "deadline": "operation", **payload}


def spec_cases() -> list[SpecCase]:
    shared = {
        "w": _op("write", path=R0, value_id="w"),
        "r": _op("read", path=R0, channel="fuse"),
        "s": _op("status", channel="sl"),
        "c": _op("checkout", revision=2, via="goto", mode="normal"),
    }
    admitted = _differential_spec(
        shared, [{"id": "a", "actions": ["w", "r", "s", "c"]}]
    )

    def variant(extra: dict[str, Any], actor_actions: list[str]) -> dict[str, Any]:
        document = copy.deepcopy(admitted)
        document["actions"].update(extra)
        document["actors"].append({"id": "b", "actions": actor_actions})
        return document

    unordered = copy.deepcopy(admitted)
    unordered["actors"] = [
        {"id": "a", "actions": ["w", "s", "c"]},
        {"id": "b", "actions": ["r"]},
    ]
    return [
        SpecCase(
            "shared-ordered-admitted",
            "Write, read, sl status, and sl goto in one "
            "actor's order are shared and comparable.",
            admitted,
            True,
        ),
        SpecCase(
            "thrift-checkout-rejected",
            "A Thrift checkout exists only on EdenFS.",
            variant(
                {
                    "f": _op(
                        "checkout",
                        revision=1,
                        via="thrift",
                        mode="normal",
                        requires=[["c", "finished"]],
                    )
                },
                ["f"],
            ),
            False,
            "eden-only-channel",
        ),
        SpecCase(
            "goto-force-checkout-admitted",
            "A FORCE `sl goto` runs on both sides.",
            variant(
                {
                    "f": _op(
                        "checkout",
                        revision=1,
                        via="goto",
                        mode="force",
                        requires=[["c", "finished"]],
                    )
                },
                ["f"],
            ),
            True,
        ),
        SpecCase(
            "thrift-read-rejected",
            "A Thrift read exists only on EdenFS.",
            variant(
                {
                    "t": _op(
                        "read",
                        path=R0,
                        channel="thrift",
                        sync_timeout_seconds=60,
                        requires=[["c", "finished"]],
                    )
                },
                ["t"],
            ),
            False,
            "eden-only-channel",
        ),
        SpecCase(
            "journal-rejected",
            "Journal observations exist only on EdenFS.",
            variant({"j": _op("journal_window", requires=[["c", "finished"]])}, ["j"]),
            False,
            "eden-only-observation",
        ),
        SpecCase(
            "fault-rejected",
            "Fault nemeses exist only on EdenFS.",
            variant(
                {
                    "f": _op(
                        "fault_install",
                        fault_id="F",
                        site="overlay-write",
                        target_action="w",
                        behavior="block",
                    )
                },
                ["f"],
            ),
            False,
            "eden-only-nemesis",
        ),
        SpecCase(
            "unordered-conflict-rejected",
            "A read of r0 unordered against the write "
            "to r0 is not a comparable schedule.",
            unordered,
            False,
            "unordered-conflict",
        ),
    ]
