# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import fnmatch
import io
import os
from typing import BinaryIO, List

from .bufio import BufIO
from .types import ShellFS


class TestFS(ShellFS):
    """In-memory fs for testing without writing actual files"""

    @property
    def _paths(self):
        return self.state

    def open(self, path: str, mode: str) -> BinaryIO:
        path = self._absjoin(path)
        if path == "/dev/null":
            return BufIO.devnull()
        if "r" in mode and path not in self._paths:
            raise FileNotFoundError(f"{path} is not found among {sorted(self._paths)}")
        if "w" in mode or ("a" in mode and path not in self._paths):
            # create, or truncate
            self._paths[path] = BufIO()
        f = self._paths[path]
        if "r" in mode:
            # read from start
            f.seek(0)
        if "a" in mode:
            # append from end
            f.seek(0, 2)
        return f

    def glob(self, pat: str) -> List[str]:
        prefix = self._absjoin("")
        paths = [p[len(prefix) :] for p in self._paths if p.startswith(prefix)]
        paths = fnmatch.filter(paths, pat)
        return paths

    def _absjoin(self, path: str) -> str:
        """join and make a path absolute"""
        path = os.path.join(self.cwd(), path)
        if not path.startswith("/"):
            path = f"/{path}"
        return path
