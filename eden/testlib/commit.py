# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

from typing import List

from .repo import Repo


class Commit:
    hash: str
    repo: Repo

    def __init__(self, repo: Repo, hash: str) -> None:
        self.hash = hash
        self.repo = repo

    def ancestor(self, idx: int) -> Commit:
        commit = self
        # This could be more efficient, instead of execing hg for every step of
        # the parent.
        while idx > 0:
            idx -= 1
            parents = self.parents()
            if len(parents) == 0:
                raise ValueError("reached end of history when traversing parents")
            commit = parents[0]
        return commit

    def parents(self) -> List[Commit]:
        raw = self.repo.hg.log(rev=f"parents({self.hash})", template="{node}\n").stdout
        lines = raw.split("\n")
        return [Commit(self.repo, hash) for hash in lines[:-1]]
