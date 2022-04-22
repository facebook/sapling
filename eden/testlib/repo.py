# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import json
import os
from pathlib import Path
from typing import Any, Dict, IO, BinaryIO, List, TextIO, Union

from .commit import Commit
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

    def commits(self, commits: str) -> List[Commit]:
        output = self.hg.log(rev=commits, template="{node}\n").stdout
        lines = output.split("\n")
        if len(lines) == 0:
            raise ValueError("unknown commit %s" % commits)
        return [Commit(self, hash) for hash in lines[:-1]]

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

    def drawdag(self, text: str) -> None:
        self.hg.debugdrawdag(stdin=text)
