#!/usr/bin/env python3
# Copyright (c) 2018-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import logging
import os
import subprocess
import sys
import typing
from pathlib import Path
from typing import Dict, Iterable, List, NamedTuple, Optional

from . import util


ProcessID = int


class ProcessInfo(NamedTuple):
    pid: ProcessID
    cmdline: List[bytes]
    eden_dir: Optional[Path]


log = logging.getLogger("eden.cli.process_finder")


class ProcessFinder(abc.ABC):
    @abc.abstractmethod
    def find_rogue_pids(self) -> List[ProcessID]:
        """Returns a list of rogue pids for edenfs processes"""


class NopProcessFinder(ProcessFinder):
    def find_rogue_pids(self) -> List[ProcessID]:
        return []


class LinuxProcessFinder(ProcessFinder):
    proc_path = Path("/proc")

    def find_rogue_pids(self) -> List[ProcessID]:
        edenfs_processes = self.get_edenfs_processes()
        return [info.pid for info in self.yield_rogue_processes(edenfs_processes)]

    def get_edenfs_processes(self) -> List[ProcessInfo]:
        """Return information about all running edenfs processes owned by the
        specified user.
        """
        user_id = os.getuid()

        edenfs_processes = []
        for entry in os.listdir(self.proc_path):
            # Ignore entries that do not look like integer process IDs
            try:
                pid = int(entry)
            except ValueError:
                continue

            pid_path = self.proc_path / entry
            try:
                # Ignore processes not owned by the current user
                st = pid_path.lstat()
                if st.st_uid != user_id:
                    continue

                # Ignore processes that aren't edenfs
                comm = (pid_path / "comm").read_bytes()
                if comm != b"edenfs\n":
                    continue

                cmdline_bytes = (pid_path / "cmdline").read_bytes()
            except OSError:
                # Ignore any errors we encounter reading from the /proc files.
                # For instance, this could happen if the process exits while we are
                # trying to read its data.
                continue

            cmdline = cmdline_bytes.split(b"\x00")
            eden_dir = self.get_eden_dir(pid, cmdline)
            edenfs_processes.append(
                ProcessInfo(pid=pid, cmdline=cmdline, eden_dir=eden_dir)
            )

        return edenfs_processes

    def read_lock_file(self, path: Path) -> bytes:
        return path.read_bytes()

    def get_eden_dir(self, pid: ProcessID, cmdline: List[bytes]) -> Optional[Path]:
        eden_dir: Optional[Path] = None
        for idx in range(1, len(cmdline) - 1):
            if cmdline[idx] == b"--edenDir":
                eden_dir = Path(os.fsdecode(cmdline[idx + 1]))
                break

        if eden_dir is None:
            log.debug(
                f"could not determine edenDir for edenfs process {pid} ({cmdline})"
            )
            return None

        if not eden_dir.is_absolute():
            # We generally expect edenfs to be invoked with an absolute path to its
            # state directory.  We cannot check relative paths here, so just skip them.
            log.debug(
                f"could not determine absolute path to edenDir for edenfs process "
                f"{pid} ({cmdline})"
            )
            return None

        return eden_dir

    def yield_rogue_processes(
        self, edenfs_processes: List[ProcessInfo]
    ) -> Iterable[ProcessInfo]:
        # Build a dictionary of eden directory to list of running PIDs,
        # so that below we can we only check each eden directory once even if there are
        # multiple processes that appear to be running for it.
        info_by_eden_dir: Dict[Path, List[ProcessInfo]] = {}
        for info in edenfs_processes:
            if info.eden_dir is None:
                continue
            if info.eden_dir not in info_by_eden_dir:
                info_by_eden_dir[info.eden_dir] = []
            info_by_eden_dir[info.eden_dir].append(info)

        log.debug(f"List of processes per eden_dir output: {info_by_eden_dir}")

        # Filter this list to only ones that we can confirm shouldn't be running
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
                lock_pid = ProcessID(self.read_lock_file(lockfile).strip())
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
                    yield info


def new():
    if sys.platform == "linux2":
        return LinuxProcessFinder()
    return NopProcessFinder()
