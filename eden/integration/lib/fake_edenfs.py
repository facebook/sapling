#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import contextlib
import os
import pathlib
import signal
import subprocess
import typing

from .find_executables import FindExe


class FakeEdenFS(typing.ContextManager[int]):
    """A running fake_edenfs process."""

    @staticmethod
    def spawn(
        eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
    ) -> "FakeEdenFS":
        command = [FindExe.FAKE_EDENFS, "--edenDir", str(eden_dir)]
        command.extend(extra_arguments)
        subprocess.check_call(command)
        edenfs_pid = int((eden_dir / "lock").read_text())
        return FakeEdenFS(process_id=edenfs_pid)

    def __init__(self, process_id: int) -> None:
        super().__init__()
        self.process_id = process_id

    def __enter__(self) -> int:
        return self.process_id

    def __exit__(self, exc_type, exc_val, exc_tb):
        with contextlib.suppress(ProcessLookupError):
            os.kill(self.process_id, signal.SIGTERM)
        return None
