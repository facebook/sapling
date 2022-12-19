# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import subprocess
from typing import List

from edenscm import error
from edenscm.i18n import _


def run_git_command(args: List[str], gitdir: str) -> bytes:
    """Returns stdout as a bytes if the command is successful."""
    full_args = ["git", "--git-dir", gitdir] + args
    proc = subprocess.run(full_args, capture_output=True)
    if proc.returncode == 0:
        return proc.stdout
    else:
        raise error.Abort(
            _("`%s` failed with exit code %d: %s")
            % (
                " ".join(full_args),
                proc.returncode,
                f"stdout: {proc.stdout.decode()}\nstderr: {proc.stderr.decode()}\n",
            )
        )
