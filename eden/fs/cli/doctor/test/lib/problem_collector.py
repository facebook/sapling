# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import List

from eden.fs.cli.doctor.problem import ProblemBase, ProblemTracker


class ProblemCollector(ProblemTracker):
    problems: List[ProblemBase]

    def __init__(self) -> None:
        super().__init__()
        self.problems = []

    def add_problem(self, problem: ProblemBase) -> None:
        self.problems.append(problem)
