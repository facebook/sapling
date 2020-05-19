#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import subprocess
import sys
from pathlib import Path
from typing import List, Optional

from . import proc_utils_win
from .config import EdenInstance


def start_edenfs_service(_cmd: List[str]) -> int:
    script = Path(sys.executable).parent.parent / "scripts" / "startservice.ps1"
    cmd = ["powershell.exe", str(script)]
    return subprocess.call(cmd)


def run_edenfs_foreground(cmd: List[str]) -> int:
    """Run EdenFS in the "foreground" of the user's terminal.  It will log directly to
    our stdout/stderr, and we'll wait for it to exit before we return.
    """
    process = subprocess.Popen(cmd)
    while True:
        try:
            return process.wait()
        except KeyboardInterrupt:
            # Catch the exception if the user interrupts EdenFS with Ctrl-C.
            # The interrupt will have also been delivered to EdenFS, so it should shut
            # down.  Continue around the while loop to keep waiting for it to exit, and
            # still pass through its return code.
            continue
