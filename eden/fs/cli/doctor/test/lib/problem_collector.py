# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from typing import List

from eden.fs.cli.config import AbstractEdenInstance
from eden.fs.cli.doctor.problem import ProblemBase, ProblemTracker


class ProblemCollector(ProblemTracker):
    problems: List[ProblemBase]

    def __init__(self, instance: AbstractEdenInstance) -> None:
        super().__init__(instance)
        self.problems = []

    def add_problem_impl(self, problem: ProblemBase) -> None:
        self.problems.append(problem)
