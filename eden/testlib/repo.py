# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import json
import os
from pathlib import Path
from typing import Any, IO, BinaryIO, TextIO, Union

from .generators import RepoGenerator
from .hg import hg
from .workingcopy import WorkingCopy


class Repo:
    root: Path
    _wc: WorkingCopy
    hg: hg

    def __init__(self, root: Path) -> None:
        self.root = root
        self._wc = WorkingCopy(self, root)
        self.gen = RepoGenerator()
        self.hg = hg(self.root)

    def add_config(self, section: str, key: str, value: str) -> None:
        with self._open("hgrc", mode="a+") as f:
            f.write(
                f"""
[{section}]
{key}={value}
"""
            )

    # pyre-ignore[3] - pyre doesn't like that this can return str and bytes
    def _open(self, path: str, mode: str = "r") -> IO[Any]:
        return open(self._join(path), mode)

    def _join(self, path: str) -> Path:
        return os.path.join(self.root, ".hg", path)

    def working_copy(self) -> WorkingCopy:
        # TODO: Eden support & new-work-dir support
        return self._wc
