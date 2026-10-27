# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Cases waiting on a task: `waiting_on: "T<number>"`.

A deck or a corpus case declares `waiting_on` in its RunSpec metadata; a
spec validation refuses carries it as `SpecError.waiting_on`. A waiting
case that fails is skipped (`waiting on <task>`) and counted per task; one
that passes prints a notice to remove the marker and does not fail.
"""

from __future__ import annotations

import contextlib
import unittest
from collections import Counter
from collections.abc import Iterator

WAITING_ON = "waiting_on"


class Waiting:
    """Per-task counts of waiting cases."""

    def __init__(self) -> None:
        self.counts: Counter[str] = Counter()

    def total(self) -> int:
        return sum(self.counts.values())

    def skip(self, test: unittest.TestCase, task: str) -> None:
        """Count one waiting case and skip it."""
        self.counts[task] += 1
        test.skipTest(f"waiting on {task}")

    @contextlib.contextmanager
    def case(
        self, test: unittest.TestCase, name: str, task: str | None
    ) -> Iterator[None]:
        """Run one case; when it waits on `task`, a failure skips it and a pass prints a notice."""
        if task is None:
            yield
            return
        try:
            yield
        except test.failureException:
            self.skip(test, task)
        else:
            print(f"{name} now passes; remove the marker")

    def report(self) -> None:
        """Print the per-task count of waiting cases."""
        for task, count in sorted(self.counts.items()):
            print(f"waiting on {task}: {count}")
