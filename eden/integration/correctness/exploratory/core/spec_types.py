# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""RunSpec vocabulary: deadlines, dependency events, and the typed spec dataclasses."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Mapping


SCHEMA = "edenfs-exploratory-run"
SCHEMA_VERSION = 1

STARTED = "started"
FINISHED = "finished"
EVENTS = frozenset({STARTED, FINISHED})

DEADLINE_OPERATION = "operation"
DEADLINE_RUN = "run"
DEADLINES = frozenset({DEADLINE_OPERATION, DEADLINE_RUN})

MAX_ACTIONS = 1000


class SpecError(ValueError):
    """A RunSpec is malformed or violates a validation rule.

    `waiting_on` names the task (`T<number>`) whose fix lifts the rule.
    """

    waiting_on: str | None = None


@dataclass(frozen=True)
class Environment:
    """Target-owned environment payload."""

    target: Mapping[str, Any]


@dataclass(frozen=True)
class Budgets:
    """Execution deadlines shared by every target."""

    operation_deadline_seconds: float
    cleanup_deadline_seconds: float
    runner_deadline_seconds: float


@dataclass(frozen=True)
class Actor:
    """One sequential worker owning an ordered list of action IDs."""

    id: str
    actions: tuple[str, ...]


@dataclass(frozen=True)
class Action:
    """One typed client or fault action with its dependency edges."""

    id: str
    kind: str
    deadline: str
    requires: tuple[tuple[str, str], ...] = ()
    payload: Mapping[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class RunSpec:
    """One fully resolved exploratory run: input to runner and checker."""

    seed: int
    target: str
    environment: Environment
    budgets: Budgets
    actors: tuple[Actor, ...]
    actions: tuple[Action, ...]
    metadata: Mapping[str, Any] = field(default_factory=dict)

    def action(self, action_id: str) -> Action:
        """Return the action with the given ID, or raise SpecError."""
        for action in self.actions:
            if action.id == action_id:
                return action
        raise SpecError(f"unknown action {action_id!r}")

    def owner(self, action_id: str) -> str:
        """Return the ID of the actor that runs the given action, or raise SpecError."""
        for actor in self.actors:
            if action_id in actor.actions:
                return actor.id
        raise SpecError(f"action {action_id!r} has no actor")
