# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Test-only EdenFS targets that never start a daemon."""

from __future__ import annotations

from collections import Counter
from typing import Any

from eden.integration.correctness.exploratory.core.effects import (
    Constraint,
    EffectGroup,
    Footprint,
    Observation,
)
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.core.spec_types import DEADLINE_OPERATION
from eden.integration.correctness.exploratory.core.target import Target, TargetSources
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    expects_reference,
    timeout_capable,
)
from eden.integration.correctness.exploratory.edenfs.sites import SITES
from eden.integration.correctness.repo import GeneratedRepo

ENVIRONMENT_FIELDS: dict[str, type] = {
    "repository": dict,
    "channel": str,
    "case_sensitive": bool,
    "fixture_source": str,
    "setup_profile": dict,
}


def _unused(*_args: Any) -> Any:
    raise AssertionError("vocabulary tests do not run EdenFS")


VOCAB_TARGET = Target(
    name="edenfs",
    actions=ACTION_DEFS,
    sites=SITES,
    properties=(),
    install_witnessed=_unused,
    environment_fields=ENVIRONMENT_FIELDS,
    make_environment=_unused,
    make_backend=_unused,
    red_controls=frozenset(),
    timeout_capable=timeout_capable,
    sources=TargetSources(implementation=__name__),
    expects_reference=expects_reference,
)


WORKED_EXAMPLE = """{"schema":"edenfs-exploratory-run","schema_version":1,"seed":6201,"target":"edenfs","environment":{"target":{"repository":{"file_count":10000,"commit_count":4,"registers":["tree/registers/r0.reg","tree/registers/r1.reg"]},"channel":"fuse","case_sensitive":true,"fixture_source":"backing","setup_profile":{"materialize":false,"scatter_dirty":0}}},"budgets":{"operation_deadline_seconds":120,"cleanup_deadline_seconds":30,"runner_deadline_seconds":600},"actors":[{"id":"nemesis","actions":["f1","h1","x1"]},{"id":"a","actions":["c0"]},{"id":"b","actions":["w1","r1","t1","r2"]}],"actions":{"c0":{"kind":"checkout","deadline":"run","requires":[["f1","finished"]],"revision":2,"via":"goto","mode":"normal"},"f1":{"kind":"fault_install","deadline":"operation","fault_id":"F","target_action":"c0","site":"checkout-begin","behavior":"block"},"h1":{"kind":"fault_await","deadline":"operation","requires":[["c0","started"]],"fault_id":"F"},"r1":{"kind":"read","deadline":"operation","path":"tree/registers/r0.reg","channel":"fuse"},"r2":{"kind":"read","deadline":"operation","requires":[["x1","finished"]],"path":"tree/registers/r0.reg","channel":"fuse"},"t1":{"kind":"read","deadline":"operation","path":"tree/registers/r0.reg","channel":"thrift","sync_timeout_seconds":60},"w1":{"kind":"write","deadline":"operation","requires":[["h1","finished"]],"path":"tree/registers/r0.reg","value_id":"w1"},"x1":{"kind":"fault_remove","deadline":"operation","requires":[["t1","finished"]],"fault_id":"F"}}}"""


class RecordingSink:
    """A contracts sink that keeps every group, observation, and constraint it is given."""

    def __init__(self) -> None:
        self.groups: list[EffectGroup] = []
        self.observations: list[Observation] = []
        self.constraints: list[Constraint] = []
        # The parsed spec, when `contract_run` built the sink.
        self.spec: Any = None

    def group(self, group: EffectGroup) -> None:
        self.groups.append(group)

    def observe(self, observation: Observation) -> None:
        self.observations.append(observation)

    def constrain(self, constraint: Constraint) -> None:
        self.constraints.append(constraint)

    def by_id(self, group_id: str) -> EffectGroup:
        return next(group for group in self.groups if group.id == group_id)


class CountingRepo(GeneratedRepo):
    """A GeneratedRepo that counts point queries and fails on any enumeration.

    `files` (the whole layout) always fails. `changed_indices` (a revision's
    changes) fails too unless `walks_changes` allows it, for code whose cost
    is meant to follow the changed indexes.
    """

    def __init__(self, *args: Any, walks_changes: bool = False, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self.calls: Counter[str] = Counter()
        self.walks_changes = walks_changes

    def file_at(self, index: int, revision: int) -> Any:
        self.calls["file_at"] += 1
        return super().file_at(index, revision)

    def content(self, generated: Any, revision: int) -> bytes:
        self.calls["content"] += 1
        return super().content(generated, revision)

    def is_directory(self, path: str, revision: int) -> bool:
        self.calls["is_directory"] += 1
        return super().is_directory(path, revision)

    def files(self, revision: int) -> Any:
        raise AssertionError("a point query enumerated the layout")

    def changed_indices(self, revision: int) -> Any:
        if not self.walks_changes:
            raise AssertionError("a point query walked a revision's changes")
        return super().changed_indices(revision)


def make_action(action_id: str, kind: str, **payload: Any) -> Action:
    """Return one operation-deadline action."""
    return Action(action_id, kind, DEADLINE_OPERATION, payload=payload)


def footprint_of(spec: RunSpec, action: Action) -> Footprint:
    """Return an action's declared footprint under the EdenFS vocabulary."""
    return ACTION_DEFS[action.kind].footprint(spec, action)


def getattr_facts(**fields: object) -> dict[str, Any]:
    """Return a successful getattr's facts for a plain 64-byte file, with `fields` replaced."""
    return {
        "mtime": 1.0,
        "mtime_history": 1.0,
        "size": 64,
        "mode": 0o644,
        "fuse_type": "file",
        "thrift_type": "file",
        **fields,
    }
