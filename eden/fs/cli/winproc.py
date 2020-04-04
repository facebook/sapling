#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import subprocess
from typing import List, Optional

from . import proc_utils_win
from .config import EdenInstance


def start_edenfs(
    instance: EdenInstance, daemon_binary: str, edenfs_args: Optional[List[str]] = None
) -> None:
    cmd = [
        daemon_binary,
        "--edenDir",
        str(instance._config_dir),
        "--etcEdenDir",
        str(instance._etc_eden_dir),
        "--configPath",
        str(instance._user_config_path),
    ]
    cmd_str = subprocess.list2cmdline(cmd)

    proc_utils_win.create_process_shim(cmd_str)

    print("Edenfs started")
