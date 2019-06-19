#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import shlex
from typing import List

from eden.cli import process_finder
from eden.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker


def check_many_edenfs_are_running(
    tracker: ProblemTracker, process_finder: process_finder.ProcessFinder
) -> None:
    rogue_pids_list = process_finder.find_rogue_pids()
    if len(rogue_pids_list) > 0:
        rogue_pids_problem = ManyEdenFsRunning(rogue_pids_list)
        tracker.add_problem(rogue_pids_problem)


class ManyEdenFsRunning(Problem):
    _rogue_pids_list: List[process_finder.ProcessID]

    def __init__(self, rogue_pids_list):
        self._rogue_pids_list = list(sorted(rogue_pids_list))
        self.set_manual_remediation_message()

    def description(self) -> str:
        return (
            "Many edenfs processes are running. "
            "Please keep only one for each config directory."
        )

    def severity(self) -> ProblemSeverity:
        return ProblemSeverity.ADVICE

    def set_manual_remediation_message(self) -> None:
        if self._rogue_pids_list is not None:
            kill_command = ["kill", "-9"]
            kill_command.extend(map(str, self._rogue_pids_list))
            self._remediation = " ".join(map(shlex.quote, kill_command))
