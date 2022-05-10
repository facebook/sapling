# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import threading
from pathlib import Path
from typing import Dict, Optional

from eden.test_support.temporary_directory import TempFileManager


class GlobalTestState(threading.local):
    temp_mgr: TempFileManager
    test_tmp: Path
    env: Dict[str, str]

    def __init__(self) -> None:
        # These are needed to satisfy pyre, but should never be used.
        self.temp_mgr = TempFileManager()
        self.test_tmp = Path("")
        self.env = {}

    def setup(self) -> None:
        self.temp_mgr = TempFileManager()
        self.test_tmp = self.temp_mgr.make_temp_dir()

        hgrc_path = os.path.join(new_dir(), "global_hgrc")
        self.env = {
            "HGRCPATH": hgrc_path,
            "TESTTMP": str(self.test_tmp),
            "TEST_PROD_CONFIGS": "true",
        }

    def cleanup(self) -> None:
        self.temp_mgr.cleanup()


# Global state makes it easier to hand common objects around, like the temp
# directory manager and the test environment. In the future we might want to run
# tests in parallel, so let's make this global state be thread local.
test_globals = GlobalTestState()


def new_dir() -> Path:
    temp = test_globals.temp_mgr
    return temp.make_temp_dir()


def new_file() -> Path:
    temp = test_globals.temp_mgr
    with temp.make_temp_file() as f:
        return Path(f.name)


tracing: Optional[str] = os.environ.get("HGTEST_TRACING")


def trace(value: str) -> None:
    if tracing:
        print(value)
