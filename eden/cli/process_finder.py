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
import typing
from pathlib import Path
from typing import Dict, List, Optional

from . import util


ProcessID = int


log = logging.getLogger("eden.cli.process_finder")


class ProcessFinder(abc.ABC):
    @abc.abstractmethod
    def find_rogue_pids(self) -> List[ProcessID]:
        """Returns a list of rogue pids for edenfs processes"""


class LinuxProcessFinder(ProcessFinder):
    def find_rogue_pids(self) -> List[ProcessID]:
        try:
            output = self.get_pgrep_output()
        except Exception as ex:
            log.warning(
                f"Error determining currently running edenfs processes", exc_info=True
            )
            return []
        return self.keep_only_rogue_pids(output)

    def get_pgrep_output(self) -> bytes:
        # TODO: It would perhaps be better for this code to just manually examine
        # /proc/*/cmdline.  The caller really wants to know the argument list,
        # and this can't really be split up correctly from the pgrep output.  The
        # calling code also will choke on the output today if one of the commands
        # contains a newline in one of its arguments.
        username = util.get_username()
        cmd = ["pgrep", "-aU", username, "edenfs"]

        try:
            output = typing.cast(bytes, subprocess.check_output(cmd))
        except subprocess.CalledProcessError:
            log.warning(f"Error running command: {cmd}\nIt exited with failure status.")
            return b""

        if len(output) == 0:
            log.warning(f"No output received from the OS for cmd: {cmd}")
            return b""

        log.debug(f"Output for cmd {cmd}\n{output}")
        return output

    def read_lock_file(self, path: Path) -> bytes:
        return path.read_bytes()

    def keep_only_rogue_pids(self, output: bytes) -> List[ProcessID]:
        pid_config_dict: Dict[Path, List[ProcessID]] = {}
        # find all potential pids
        for line in output.splitlines():
            # line looks like: "PID<SPACE>CMDLINE".
            # We're looking for "--edenDir SOMETHING" in the CMDLINE.
            entries = line.split()
            process_name = entries[1].split(bytes(os.sep, "utf-8"))[-1]
            if process_name != b"edenfs":
                continue
            pid = ProcessID(entries[0])
            eden_dir: Optional[Path] = None
            for i in range(2, len(entries) - 1):
                if entries[i] == b"--edenDir":
                    eden_dir = Path(os.fsdecode(entries[i + 1]))
                    break

            # TODO: This check logic assumes eden_dir is an absolute path,
            # but does not actually verify that.
            if eden_dir is None:
                log.debug(
                    f"could not determine edenDir for edenfs process {pid} "
                    f"({entries[1:]})"
                )
                continue

            if eden_dir not in pid_config_dict:
                pid_config_dict[eden_dir] = []
            pid_config_dict[eden_dir].append(pid)

        log.debug(f"List of processes per eden_dir output: {pid_config_dict}")

        # Filter this list to only ones that we can confirm shouldn't be running
        rogue_pids: List[ProcessID] = []
        for eden_dir, pid_list in pid_config_dict.items():
            # Only bother checking for rogue processes if we found more than one EdenFS
            # instance for this directory.
            #
            # The check below is inherently racy: it can misdetect state if edenfs
            # processes are currently starting/stopping/restarting while it runs.
            # Therefore we only want to try and report this if we actually find multiple
            # edenfs processes for the same state directory.
            if len(pid_list) <= 1:
                continue

            lockfile = eden_dir / "lock"
            try:
                lock_pid = ProcessID(self.read_lock_file(lockfile).strip())
            except IOError:
                log.warning(f"Lock file cannot be read for {eden_dir}", exc_info=True)
                continue
            except ValueError:
                log.warning(
                    f"lock file contains data that cannot be parsed for PID: "
                    f"{lockfile}",
                    exc_info=True,
                )
                continue

            for pid in pid_list:
                if pid != lock_pid:
                    rogue_pids.append(pid)

        log.debug(f"List of rogue processes : {rogue_pids}")
        return rogue_pids
