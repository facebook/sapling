# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Builders shared by the environment fixture tests: single-action specs, the
environment target, getattr actions, and the hang guard.
"""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    Environment as SpecEnvironment,
    RunSpec,
)
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET


SEED = 7


REGISTER = "tree/registers/r0.reg"


# Real seconds before a test gives up waiting; a hang guard, not a measurement.
HANG_GUARD = 60.0


def environment_target(
    *,
    case_sensitive: bool = True,
    materialize: bool = False,
    scatter_dirty: int = 0,
    file_count: int = 32,
    register: str = REGISTER,
    memory_max_mb: int | None = None,
    warm_readdir: bool | None = None,
    aged_overlay: int | None = None,
) -> dict[str, Any]:
    """Return the target-owned environment payload for one live test."""
    profile: dict[str, Any] = {
        "materialize": materialize,
        "scatter_dirty": scatter_dirty,
    }
    for name, value in (
        ("memory_max_mb", memory_max_mb),
        ("warm_readdir", warm_readdir),
        ("aged_overlay", aged_overlay),
    ):
        if value is not None:
            profile[name] = value
    return {
        "repository": {
            "file_count": file_count,
            "commit_count": 4,
            "registers": [register],
        },
        "channel": "fuse",
        "case_sensitive": case_sensitive,
        "fixture_source": "backing",
        "setup_profile": profile,
    }


def single_action_spec(action: Action, environment: dict[str, Any]) -> RunSpec:
    """Return a live spec containing one modeled action."""
    return RunSpec(
        seed=SEED,
        target=RECORDING_TARGET.name,
        environment=SpecEnvironment(target=environment),
        # Hang guards: these tests check outcomes, not how long a loaded host
        # takes to start a daemon and reach them.
        budgets=Budgets(120.0, 60.0, 540.0),
        actors=(Actor(id="actor", actions=(action.id,)),),
        actions=(action,),
    )


def getattr_action(path: str = REGISTER) -> Action:
    return Action(
        id="g1", kind="getattr", deadline=DEADLINE_OPERATION, payload={"path": path}
    )
