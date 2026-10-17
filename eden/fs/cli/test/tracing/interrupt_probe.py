# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import enum
import importlib
import os
import sys
import threading
import time
import types
from unittest.mock import patch


def main() -> None:
    if sys.platform == "win32":
        prjfs = types.ModuleType("eden.fs.cli.prjfs")
        prjfs.PRJ_FILE_STATE = enum.IntFlag(
            "PRJ_FILE_STATE",
            [
                "Placeholder",
                "HydratedPlaceholder",
                "DirtyPlaceholder",
                "Full",
                "Tombstone",
            ],
        )
        sys.modules["eden.fs.cli.prjfs"] = prjfs
    from eden.fs.cli import main as edenfsctl

    native = importlib.import_module("artillery.art_cli_lite.art_cli_lite_py")
    faults = {
        "finish": contextlib.nullcontext(),
        "start_failure": patch.object(
            threading.Thread, "start", side_effect=RuntimeError("no thread")
        ),
        "second_interrupt": patch.object(
            threading.Thread, "join", side_effect=KeyboardInterrupt
        ),
        "hang": patch.object(
            native, "finish_process", side_effect=lambda: time.sleep(30)
        ),
    }
    fault = faults[sys.argv[1]]
    sys.argv = ["edenfsctl", "list"]
    try:
        with (
            patch.object(edenfsctl, "asyncio_run", side_effect=KeyboardInterrupt),
            fault,
        ):
            with open(os.environ["INTERRUPT_PROBE_STARTED"], "w") as f:
                f.write(repr(time.monotonic()))
            edenfsctl.zipapp_main()
    finally:
        os._exit(99)
