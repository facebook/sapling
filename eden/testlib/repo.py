# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import json
import os
from pathlib import Path
from typing import Any, Dict, IO, List, Optional

from .commit import Commit
from .config import Config
from .errors import AmbiguousCommitError, MissingCommitError
from .generators import RepoGenerator
from .hg import hg
from .workingcopy import EdenWorkingCopy, WorkingCopy


class Repo:
    root: Path
    hg: hg
    url: str
    name: str

    def __init__(self, root: Path, url: str, name: str) -> None:
        self.root = root
        self.hg = hg(self.root)
        self.config = Config(self._join("hgrc"))
        self.url = url
        self.name = name

    # pyre-ignore[3] - pyre doesn't like that this can return str and bytes
    def _open(self, path: str, mode: str = "r") -> IO[Any]:
        return open(self._join(path), mode)

    def _join(self, path: str) -> Path:
        # pyre-fixme[7]: Expected `Path` but got `str`.
        return os.path.join(self.root, ".hg", path)

    def new_working_copy(
        self, path: Optional[Path] = None, eden: bool = False
    ) -> WorkingCopy:
        if path is None:
            if eden:
                raise ValueError("cannot get the default working copy as EdenFS")
            return WorkingCopy(self, self.root)
        else:
            if not eden:
                raise ValueError("non-eden shared working copies is not yet supported")
            return EdenWorkingCopy(self, path)

    def __getitem__(self, hash: str) -> Commit:
        return self.commits(hash)[0]

    def commits(self, commits: str, allowempty: bool = False) -> List[Commit]:
        output = self.hg.log(rev=commits, template="{node}\n").stdout
        lines = output.split("\n")[:-1]
        if not allowempty and len(lines) == 0:
            raise MissingCommitError("unknown commit %s" % commits)
        return [Commit(self, hash) for hash in lines]

    def commit(self, commit: str) -> Commit:
        commitlist = self.commits(commit)
        if len(commitlist) > 1:
            raise AmbiguousCommitError(
                f"ambiguous identifier {commit} - matched {len(commitlist)} commits"
            )
        return commitlist[0]

    def bookmarks(self) -> Dict[str, Commit]:
        output = json.loads(self.hg.bookmarks(template="json").stdout)
        bookmarks = {}
        for entry in output:
            bookmarks[entry["bookmark"]] = Commit(self, entry["node"])
        return bookmarks

    def remote_bookmarks(self) -> Dict[str, str]:
        output = json.loads(
            self.hg.bookmarks(list_subcriptions=True, template="json").stdout
        )
        bookmarks = {}
        for entry in output:
            name = entry["remotebookmark"]
            remote, name = name.split("/", 1)
            bookmarks[name] = Commit(self, entry["node"])
        return bookmarks

    def hide(self, commit: Commit) -> None:
        self.hg.hide(commit)

    def drawdag(self, text: str) -> None:
        self.hg.debugdrawdag(stdin=text)
