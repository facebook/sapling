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
from typing import Dict, List

from . import util


ProcessID = int


log = logging.getLogger("eden.cli.process_finder")


class ProcessFinder(abc.ABC):
    @abc.abstractmethod
    def find_rogue_pids(self) -> List[ProcessID]:
        """Returns a list of rogue pids for edenfs processes"""


class LinuxProcessFinder(ProcessFinder):
    def find_rogue_pids(self) -> List[ProcessID]:
        username = util.get_username()
        cmd = ["pgrep", "-aU", username, "edenfs"]

        try:
            output = subprocess.check_output(cmd)
        except subprocess.CalledProcessError:
            log.warning(f"Error running command: {cmd}\nIt exited with failure status.")
            return []

        if len(output) == 0:
            log.warning(f"No output received from the OS for cmd: {cmd}")
            return []

        log.debug(f"Output for cmd {cmd}\n{output}")
        return self.keep_only_rogue_pids(output)

    def keep_only_rogue_pids(self, output: bytes) -> List[ProcessID]:
        pid_list: List[ProcessID] = []

        pid_config_dict: Dict[bytes, List[ProcessID]] = {}
        # find all potential pids
        for line in output.splitlines():
            # line looks like: "PID<SPACE>CMDLINE".
            # We're looking for "--edenDir SOMETHING" in the CMDLINE.
            entries = line.split()
            process_name = entries[1].split(bytes(os.sep, "utf-8"))[-1]
            if process_name != b"edenfs":
                continue
            pid = ProcessID(entries[0])
            eden_dir = b""
            for i in range(len(entries) - 1):
                if entries[i] == b"--edenDir":
                    eden_dir = entries[i + 1]
                    break

            if len(eden_dir) == 0:
                log.debug(
                    f"could not determine edenDir for edenfs process {pid} "
                    f"({entries[1:]})"
                )
                continue

            if eden_dir not in pid_config_dict:
                pid_config_dict[eden_dir] = []
            pid_config_dict[eden_dir].append(pid)

        log.debug(f"List of processes per eden_dir output: {pid_config_dict}")
        # find the real PID we want to save
        for dir, pid_list in pid_config_dict.items():
            try:
                lockfile = os.path.join(dir, b"lock")
                lock_pid = ProcessID(util.read_all(lockfile).strip())
                if lock_pid in pid_list:
                    pid_list.remove(ProcessID(lock_pid))
            except IOError:
                log.warning(f"Lock file cannot be read for {dir}", exc_info=True)
                pid_list[:] = []
                continue
            except ValueError:
                log.warning(
                    f"lock file contains data that cannot be parsed for PID: \
{lockfile}",
                    exc_info=True,
                )
                pid_list[:] = []
                continue

        # flatten all lists from dict's values
        pid_list = [v for sublist in pid_config_dict.values() for v in sublist]

        log.debug(f"List of rogue processes : {pid_list}")
        return pid_list
