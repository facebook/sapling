# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import os
from pathlib import Path
from typing import Any, IO, Optional, TYPE_CHECKING, Union

from .file import File
from .hg import hg
from .types import PathLike

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

    def file(
        self,
        path: Optional[PathLike] = None,
        content: Optional[Union[bytes, str]] = None,
        add: bool = True,
    ) -> File:
        default_path = self.repo.gen.gen_file_name()
        if path is None:
            path = default_path
        if content is None:
            content = str(path)

        file = self[path]
        file.write(content)

        if add:
            self.add(path)

        return file

    def __getitem__(self, path: PathLike) -> File:
        return File(self.root, Path(str(path)))

    def add(self, path: PathLike) -> None:
        self.hg.add(str(path))

    def remove(self, path: PathLike) -> None:
        self.hg.remove(str(path), force=True)

    # pyre-ignore[3] - pyre doesn't like that this can return str and bytes
    def open(self, path: PathLike, mode: str = "r") -> IO[Any]:
        return self[path].open(mode)

    def write(self, path: PathLike, content: str) -> None:
        self[path].write(content)

    def join(self, path: PathLike) -> Path:
        return os.path.join(self.root, str(path))
