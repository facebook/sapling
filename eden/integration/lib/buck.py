# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import shlex
import subprocess
from typing import List, Optional

from eden.integration.lib.find_executables import FindExe


def build_target(target: str, cwd: Optional[str]) -> str:
    return run_buck_cmd(cmd=[FindExe.SOURCE_BUILT_BUCK, "build", target], cwd=cwd)


def run_target(target: str, cwd: Optional[str]) -> str:
    return run_buck_cmd(cmd=[FindExe.SOURCE_BUILT_BUCK, "run", target], cwd=cwd)


def run_buck_cmd(
    cmd: List[str],
    cwd: Optional[str] = None,
    encoding: str = "utf-8",
) -> str:
    # buck can get resource hungry, we only need to run some small test builds
    # so cap buck at one thread.
    cmd.append("--num-threads=1")

    try:
        # TODO we probably need to do env scrubbing?
        env = dict(os.environ)
        completed_process = subprocess.run(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=True,
            cwd=cwd,
            env=env,
            encoding=encoding,
        )
    except subprocess.CalledProcessError as ex:
        # Re-raise our own exception type so we can include the error
        # output.
        raise BuckCommandError(ex) from None
    return completed_process.stdout


class BuckCommandError(subprocess.CalledProcessError):
    def __init__(self, ex: subprocess.CalledProcessError) -> None:
        super().__init__(ex.returncode, ex.cmd, output=ex.output, stderr=ex.stderr)

    def __str__(self) -> str:
        cmd_str = " ".join(shlex.quote(arg) for arg in self.cmd)
        return "buck command returned non-zero exit status %d\n\nCommand:\n[%s]\n\nOuput:\n%s\n\nStderr:\n%s" % (
            self.returncode,
            cmd_str,
            self.output,
            self.stderr,
        )
