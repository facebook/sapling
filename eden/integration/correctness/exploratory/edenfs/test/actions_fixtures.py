# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Builders shared by the action declaration tests: run spec text over a nominal target,
single actions, and the differential spec.
"""

from __future__ import annotations

import json
from typing import Any

from eden.integration.correctness.exploratory.core.runspec import Action
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.edenfs.actions import ACTION_DEFS
from eden.integration.correctness.exploratory.edenfs.test.testing import (
    ENVIRONMENT_FIELDS,
)


def unused_factory(*_args: Any) -> Any:
    raise AssertionError("schema tests do not instantiate target resources")


def nominal_target(actions: dict[str, Any] | None = None) -> Target:
    return Target(
        name="nominal",
        actions=actions or dict(ACTION_DEFS),
        sites={},
        properties=(),
        install_witnessed=unused_factory,
        environment_fields=ENVIRONMENT_FIELDS,
        make_environment=unused_factory,
        make_backend=unused_factory,
        red_controls=frozenset(),
    )


def nominal_spec_text(
    actions: dict[str, dict[str, Any]], actors: list[dict[str, Any]]
) -> str:
    return json.dumps(
        {
            "schema": "edenfs-exploratory-run",
            "schema_version": 1,
            "seed": 1,
            "target": "nominal",
            "environment": {
                "target": {
                    "repository": {
                        "file_count": 1,
                        "commit_count": 2,
                        "registers": ["tree/registers/r0.reg"],
                    },
                    "channel": "fuse",
                    "case_sensitive": True,
                    "fixture_source": "backing",
                    "setup_profile": {"materialize": False, "scatter_dirty": 0},
                }
            },
            "budgets": {
                "operation_deadline_seconds": 1,
                "cleanup_deadline_seconds": 1,
                "runner_deadline_seconds": 2,
            },
            "actors": actors,
            "actions": actions,
        }
    )


def one_action_spec(action: dict[str, Any]) -> str:
    return nominal_spec_text({"a": action}, [{"id": "actor", "actions": ["a"]}])


WIDE_DIR = {"name": "tree/wide", "entries": 4, "churn_all": True}


R0 = "tree/registers/r0.reg"


def kind_action(kind: str, action_id: str = "a", **payload: Any) -> Action:
    return Action(id=action_id, kind=kind, deadline="operation", payload=payload)


def differential_spec_text(
    actions: dict[str, dict[str, Any]], actors: list[dict[str, Any]], **profile: Any
) -> str:
    raw = json.loads(nominal_spec_text(actions, actors))
    raw["environment"]["target"]["setup_profile"].update(
        reference_clone=True, **profile
    )
    return json.dumps(raw)


def action_payload(kind: str, **payload: Any) -> dict[str, Any]:
    return {"kind": kind, "deadline": "operation", **payload}
