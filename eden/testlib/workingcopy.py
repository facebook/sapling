# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import os
from pathlib import Path
from typing import TYPE_CHECKING

from .hg import hg

if TYPE_CHECKING:
    from .repo import Repo


class WorkingCopy:
    repo: Repo
    root: Path
    hg: hg

    def __init__(self, repo: Repo, root: Path) -> None:
        self.repo = repo
        self.root = root
        self.hg = hg(self.root)
