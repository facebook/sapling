#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import subprocess
from typing import Callable, List, Optional


class FakeHgRepo:
    commit_checker: Optional[Callable[[str], bool]] = None
    source: str

    def __init__(self, source: str) -> None:
        self.source = source

    def get_commit_hash(self, commit: str, stderr_output=None) -> str:
        commit_checker = self.commit_checker

        if not commit_checker or commit_checker(commit):
            return commit

        cmd = " ".join(["log", "-r", commit, "-T{node}"])
        output = f"RepoLookupError: unknown revision {commit}"
        raise subprocess.CalledProcessError(
            returncode=255, cmd=cmd, output=str.encode(output)
        )

    def _run_hg(self, args: List[str], stderr_output=None) -> None:
        pass
