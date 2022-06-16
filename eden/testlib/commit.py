# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

from typing import List, TYPE_CHECKING

if TYPE_CHECKING:
    from .repo import Repo


from .errors import MissingCommitError
from .status import Status


class Commit:
    hash: str
    repo: Repo

    def __init__(self, repo: Repo, hash: str) -> None:
        self.hash = hash
        self.repo = repo

    def __repr__(self) -> str:
        return "Commit-%s" % self.hash

    def __eq__(self, other: Commit) -> bool:
        if isinstance(other, Commit):
            return self.hash == other.hash
        return super().__eq__(other)

    def ancestor(self, idx: int) -> Commit:
        try:
            return self.repo.commit(f"ancestors({self.hash}, {idx}, {idx})")
        except MissingCommitError:
            raise MissingCommitError(f"ancestor with depth {idx} does not exist")

    def status(self) -> Status:
        return Status(self.repo.hg.status(change=self.hash, template="json").stdout)

    def parents(self) -> List[Commit]:
        raw = self.repo.hg.log(rev=f"parents({self.hash})", template="{node}\n").stdout
        lines = raw.split("\n")
        return [Commit(self.repo, hash) for hash in lines[:-1]]
