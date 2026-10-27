# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The four known tasks' reproducers: translated synthetic histories under `test/corpus/known/`.

Each `test/corpus/known/<task>/` holds the task's hand-authored reproducer
from the frozen tip, mechanically translated into the strict result
vocabulary: a `run-spec.json` and its `history.jsonl`. They are synthetic,
never live or calibration evidence. `decks_test` judges them to name the
canonical key each reject deck must yield.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from eden.integration.correctness.exploratory.core.history import Event, event_from_json
from eden.integration.correctness.exploratory.core.runspec import from_json, RunSpec
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET

KNOWN_DIRECTORY = Path(__file__).with_name("corpus") / "known"
KNOWN_TASKS = ("T287852915", "T287885560", "T288573788", "T288573794")
COMPLETED = {"status": "completed"}


@dataclass(frozen=True)
class Reproducer:
    """One known task's translated reproducer, validated under `EDENFS_TARGET`."""

    task: str
    spec: RunSpec
    events: tuple[Event, ...]


def reproducers() -> tuple[Reproducer, ...]:
    """Return the four known tasks' translated reproducers, in task order."""
    found = []
    for task in KNOWN_TASKS:
        directory = KNOWN_DIRECTORY / task
        spec = from_json((directory / "run-spec.json").read_text(), EDENFS_TARGET)
        events = tuple(
            event_from_json(json.loads(line))
            for line in (directory / "history.jsonl").read_text().splitlines()
        )
        found.append(Reproducer(task, spec, events))
    return tuple(found)
