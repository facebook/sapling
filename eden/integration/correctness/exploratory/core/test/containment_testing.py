# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Supervisor test helpers: a private temp root for run dirs, and a deadline that expires
on awaited runner output."""

from __future__ import annotations

import os
import shutil
import tempfile
import threading
import time
import unittest
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from typing import Any
from unittest import mock

from .. import containment


def private_temp_root(test: unittest.TestCase) -> str:
    """Point `tempfile.gettempdir()` at a fresh directory for the rest of ``test``.

    Every supervisor first sweeps the temp root (`containment.sweep_orphaned_run_dirs`),
    closing each run dir no live process holds the lock of. A test that leaks a run dir
    on purpose, or reads a run's setup facts, gets its own root, so a supervisor running
    concurrently in another test never sweeps its dirs and it never sweeps theirs.
    """
    root = tempfile.mkdtemp(prefix="containment-test.")
    patcher = mock.patch.object(tempfile, "tempdir", root)
    patcher.start()
    test.addCleanup(shutil.rmtree, root, ignore_errors=True)
    test.addCleanup(patcher.stop)
    return root


class ModuleProxy:
    """A module stand-in that overrides some attributes and forwards the rest."""

    def __init__(self, module: Any, **overrides: Any) -> None:
        self._module = module
        self.__dict__.update(overrides)

    def __getattr__(self, name: str) -> Any:
        return getattr(self._module, name)


@contextmanager
def deadline_after(ready: Callable[[bytes], bool]) -> Iterator[None]:
    """Expire the supervisor's runner deadline once ``ready`` accepts the runner's output.

    The supervisor reads its clock again after every read, so the timeout happens
    right after the awaited output instead of racing it. Until then the spec's
    deadline applies, as a hang guard for a runner that never writes.
    """
    seen = bytearray()
    expired = threading.Event()
    real_read = os.read

    def read(fd: int, size: int) -> bytes:
        data = real_read(fd, size)
        seen.extend(data)
        if ready(bytes(seen)):
            expired.set()
        return data

    def monotonic() -> float:
        return time.monotonic() + (1e9 if expired.is_set() else 0.0)

    with (
        mock.patch.object(containment, "os", ModuleProxy(os, read=read)),
        mock.patch.object(containment, "time", ModuleProxy(time, monotonic=monotonic)),
    ):
        yield
