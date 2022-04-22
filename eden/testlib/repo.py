# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import json
import os
from pathlib import Path

from .hg import hg
from .workingcopy import WorkingCopy


class Repo:
    root: Path
    _wc: WorkingCopy
    hg: hg

    def __init__(self, root: Path) -> None:
        self.root = root
        self._wc = WorkingCopy(self, root)
        self.hg = hg(self.root)

    def working_copy(self) -> WorkingCopy:
        # TODO: Eden support & new-work-dir support
        return self._wc
