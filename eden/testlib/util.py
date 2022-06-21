# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import os
import threading
from contextlib import contextmanager
from pathlib import Path
from typing import Dict, Generator, Optional

from eden.test_support.temporary_directory import TempFileManager

from .generators import RepoGenerator


class GlobalTestState(threading.local):
    temp_mgr: TempFileManager
    repo_gen: RepoGenerator
    test_tmp: Path
    env: Dict[str, str]
    debug: bool

    def __init__(self) -> None:
        # These are needed to satisfy pyre, but should never be used.
        self.temp_mgr = TempFileManager()
        self.repo_gen = RepoGenerator()
        self.test_tmp = Path("")
        self.env = {}
        self.debug = os.environ.get("HGTEST_DEBUG") is not None

    def setup(self) -> None:
        self.temp_mgr = TempFileManager()
        self.repo_gen = RepoGenerator()
        self.test_tmp = self.temp_mgr.make_temp_dir()

        hgrc_path = os.path.join(new_dir(), "global_hgrc")
        self.env = {
            "HGRCPATH": hgrc_path,
            "TESTTMP": str(self.test_tmp),
            "TEST_PROD_CONFIGS": "true",
        }

    def cleanup(self) -> None:
        if not self.debug:
            self.temp_mgr.cleanup()


# Global state makes it easier to hand common objects around, like the temp
# directory manager and the test environment. In the future we might want to run
# tests in parallel, so let's make this global state be thread local.
test_globals = GlobalTestState()


def new_dir(label: Optional[str] = None) -> Path:
    temp_dir = test_globals.temp_mgr.make_temp_dir()
    if label and test_globals.debug:
        print(f"{label}: {temp_dir}")
    return temp_dir


def new_file() -> Path:
    temp = test_globals.temp_mgr
    with temp.make_temp_file() as f:
        return Path(f.name)


tracing: Optional[str] = os.environ.get("HGTEST_TRACING")


def trace(value: str) -> None:
    if tracing:
        print(value)


@contextmanager
def override_environ(values: Dict[str, str]) -> Generator[None, None, None]:
    backup = {}
    for key, value in values.items():
        old = os.environ.get(key, None)
        if old is not None:
            backup[key] = old
        os.environ[key] = value
    yield
    for key in values.keys():
        os.environ.pop(key)
        old = backup.get(key, None)
        if old is not None:
            os.environ[key] = old
