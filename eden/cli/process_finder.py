#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import abc
import logging
import os
import platform
import sys
from pathlib import Path
from typing import Dict, Iterable, List, NamedTuple, Optional


log: logging.Logger = logging.getLogger("eden.cli.process_finder")
ProcessID = int


class EdenFSProcess(NamedTuple):
    pid: ProcessID
    uid: int
    cmdline: List[bytes]
    eden_dir: Optional[Path]


class ProcessFinder(abc.ABC):
    @abc.abstractmethod
    def get_edenfs_processes(self) -> Iterable[EdenFSProcess]:
        """Returns a list of running EdenFS processes on the system."""
        raise NotImplementedError()

    def read_lock_file(self, path: Path) -> bytes:
        """Read an EdenFS lock file.
        This method exists primarily to allow it to be overridden in test cases.
        """
        return path.read_bytes()


class NopProcessFinder(ProcessFinder):
    def get_edenfs_processes(self) -> Iterable[EdenFSProcess]:
        return []


class LinuxProcessFinder(ProcessFinder):
    proc_path = Path("/proc")

    def get_edenfs_processes(self) -> Iterable[EdenFSProcess]:
        """Return information about all running EdenFS processes.

        This returns information about processes owned by all users.  The returned
        `EdenFSProcess` objects indicate the UID of the user running each process.
        You can filter the results based on this if you only care about processes owned
        by a specific user.
        """
        for entry in os.listdir(self.proc_path):
            # Ignore entries that do not look like integer process IDs
            try:
                pid = int(entry)
            except ValueError:
                continue

            pid_path = self.proc_path / entry
            try:
                # Ignore processes owned by root, to avoid matching privhelper processes
                # D20199409 changes the privhelper to report its name as
                # "edenfs_privhelp", but in older versions of EdenFS the privhelper
                # process also showed up with a command name of "edenfs".  Once we are
                # sure no old privhelper processes from older versions of EdenFS remain
                # we can drop this check.
                st = self.stat_process_dir(pid_path)
                if st.st_uid == 0:
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
            yield EdenFSProcess(
                pid=pid, cmdline=cmdline, eden_dir=eden_dir, uid=st.st_uid
            )

    def stat_process_dir(self, path: Path) -> os.stat_result:
        """Call lstat() on a /proc/PID directory.
        This exists as a separate method solely to allow it to be overridden in unit
        tests.
        """
        return path.lstat()

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


def new() -> ProcessFinder:
    if platform.system() == "Linux":
        return LinuxProcessFinder()
    return NopProcessFinder()
