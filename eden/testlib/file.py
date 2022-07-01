# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from pathlib import Path
from typing import Any, IO, Union


def create_dirs(root: str, path: str) -> None:
    full_path = Path(os.path.join(root, path))
    full_path.parent.mkdir(parents=True, exist_ok=True)


class File:
    root: str
    path: str
    _abspath: str

    def __init__(self, root: Path, path: Path) -> None:
        # Internally File considers root and path to be OS formatted.
        self.root = os.path.abspath(os.path.normpath(root))
        self.path = os.path.normpath(path)
        self._abspath = os.path.abspath(os.path.join(root, path))
        assert self.root == os.path.commonprefix(
            [self.root, self._abspath]
        ), "%s is not a prefix of %s" % (self.root, self._abspath)

    def __str__(self) -> str:
        return str(self.path)

    def abspath(self) -> str:
        return self._abspath

    def basename(self) -> str:
        return Path(self.path).name

    # pyre-ignore[3] - pyre doesn't like that this can return str and bytes
    def open(self, mode: str = "r") -> IO[Any]:
        if "w" in mode or "a" in mode:
            # Create the directories if they don't already exist.
            create_dirs(self.root, self.path)

        return open(self._abspath, mode=mode)

    def content(self) -> str:
        with self.open() as f:
            return f.read()

    def binary(self) -> bytes:
        with self.open("rb") as f:
            return f.read()

    def remove(self) -> None:
        os.remove(self._abspath)

    def write(self, content: Union[bytes, str]) -> None:
        if isinstance(content, bytes):
            mode = "wb+"
        elif isinstance(content, str):
            mode = "w+"
        else:
            raise ValueError(
                "unsupported file content type %s (%s)" % (type(content), content)
            )
        with self.open(mode=mode) as f:
            f.write(content)

    def append(self, content: Union[bytes, str]) -> None:
        if isinstance(content, bytes):
            mode = "ab+"
        elif isinstance(content, str):
            mode = "a+"
        else:
            raise ValueError(
                "unsupported file content type %s (%s)" % (type(content), content)
            )
        with self.open(mode=mode) as f:
            f.write(content)

    def exists(self) -> bool:
        return os.path.lexists(self._abspath)
