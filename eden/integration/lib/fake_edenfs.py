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


@contextlib.contextmanager
def fake_eden_daemon(
    eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
) -> typing.Iterator[int]:
    daemon_pid = spawn_fake_eden_daemon(
        eden_dir=eden_dir, extra_arguments=extra_arguments
    )
    try:
        yield daemon_pid
    finally:
        with contextlib.suppress(ProcessLookupError):
            os.kill(daemon_pid, signal.SIGTERM)


def spawn_fake_eden_daemon(
    eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
) -> int:
    command = [FindExe.FAKE_EDENFS, "--edenDir", str(eden_dir)]
    command.extend(extra_arguments)
    subprocess.check_call(command)
    daemon_pid = int((eden_dir / "lock").read_text())
    return daemon_pid
