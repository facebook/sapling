#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import logging
import os
import shlex
import sys
from pathlib import Path
from typing import Dict, List, Optional

from eden.fs.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker
from eden.fs.cli.proc_utils import EdenFSProcess, ProcessID, ProcUtils


log: logging.Logger = logging.getLogger("eden.fs.cli.doctor.check_rogue_edenfs")


def find_rogue_processes(
    proc_utils: ProcUtils, uid: Optional[int] = None
) -> List[EdenFSProcess]:
    # Build a dictionary of eden directory to list of running PIDs,
    # so that below we can we only check each eden directory once even if there are
    # multiple processes that appear to be running for it.
    info_by_eden_dir: Dict[Path, List[EdenFSProcess]] = {}
    if sys.platform == "win32":
        user_id = 0
    else:
        user_id = os.getuid() if uid is None else uid
    for info in proc_utils.get_edenfs_processes():
        # Ignore processes not owned by the current user
        if info.uid != user_id:
            continue

        # Ignore processes if we could not figure out the EdenFS state directory.
        # This shouldn't normally happen for real EdenFS processes.
        eden_dir = info.eden_dir
        if eden_dir is None:
            continue

        if eden_dir not in info_by_eden_dir:
            info_by_eden_dir[eden_dir] = []
        info_by_eden_dir[eden_dir].append(info)

    log.debug(f"List of processes per eden_dir output: {info_by_eden_dir}")

    # Filter this list to only ones that we can confirm shouldn't be running
    rogue_processes: List[EdenFSProcess] = []
    for eden_dir, info_list in info_by_eden_dir.items():
        # Only bother checking for rogue processes if we found more than one EdenFS
        # instance for this directory.
        #
        # The check below is inherently racy: it can misdetect state if edenfs
        # processes are currently starting/stopping/restarting while it runs.
        # Therefore we only want to try and report this if we actually find multiple
        # edenfs processes for the same state directory.
        if len(info_list) <= 1:
            continue

        lockfile = eden_dir / "lock"
        try:
            lock_pid = ProcessID(proc_utils.read_lock_file(lockfile).strip())
        except OSError:
            log.warning(f"Lock file cannot be read for {eden_dir}", exc_info=True)
            continue
        except ValueError:
            log.warning(
                f"lock file contains data that cannot be parsed for PID: "
                f"{lockfile}",
                exc_info=True,
            )
            continue

        for info in info_list:
            if info.pid != lock_pid:
                rogue_processes.append(info)

    return rogue_processes


def check_many_edenfs_are_running(
    tracker: ProblemTracker, proc_utils: ProcUtils, uid: Optional[int] = None
) -> None:
    rogue_processes = find_rogue_processes(proc_utils, uid=uid)
    if len(rogue_processes) > 0:
        rogue_pids = [p.pid for p in rogue_processes]
        rogue_pids_problem = ManyEdenFsRunning(rogue_pids)
        tracker.add_problem(rogue_pids_problem)


class ManyEdenFsRunning(Problem):
    _rogue_pids_list: List[ProcessID]

    def __init__(self, rogue_pids_list: List[ProcessID]) -> None:
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
