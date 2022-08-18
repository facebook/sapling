# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import glob
import os
import shutil
from typing import BinaryIO, List

from .testfs import NullIO
from .types import ShellFS


class OSFS(ShellFS):
    """filesystem by the current OS"""

    def open(self, path: str, mode: str) -> BinaryIO:
        if path == "/dev/null":
            return NullIO()
        path = self._absjoin(path)
        if "b" not in mode:
            mode += "b"
        # pyre-fixme[7]: Expected `BinaryIO` but got `IO[typing.Any]`.
        return open(path, mode)

    def glob(self, pat: str) -> List[str]:
        prefix = self._absjoin("")
        prefixlen = len(prefix) + 1  # 1: '/'
        fullpat = self._absjoin(pat)
        paths = glob.glob(fullpat, recursive="**" in pat)
        if not os.path.isabs(pat):
            paths = [_to_posix(p[prefixlen:]) for p in paths]
        return sorted(paths)

    def _absjoin(self, path: str) -> str:
        """join and make a path absolute"""
        path = os.path.normpath(os.path.join(self.cwd(), path))
        assert os.path.isabs(path)
        return path

    def chdir(self, path: str):
        path = self._absjoin(path)
        self._setcwd(path)

    def chmod(self, path: str, mode: int):
        path = self._absjoin(path)
        os.chmod(path, mode)

    def stat(self, path: str):
        path = self._absjoin(path)
        return os.stat(path)

    def isdir(self, path: str):
        path = self._absjoin(path)
        return os.path.isdir(path)

    def isfile(self, path: str):
        path = self._absjoin(path)
        return os.path.isfile(path)

    def exists(self, path: str):
        path = self._absjoin(path)
        return os.path.exists(path)

    def listdir(self, path: str) -> List[str]:
        path = self._absjoin(path)
        return os.listdir(path)

    def mkdir(self, path: str):
        path = self._absjoin(path)
        return os.makedirs(path, exist_ok=True)

    def mv(self, src: str, dst: str):
        src = self._absjoin(src)
        dst = self._absjoin(dst)
        if os.name == "nt":
            try:
                os.unlink(dst)
            except FileNotFoundError:
                pass
        os.rename(src, dst)

    def rm(self, path: str):
        path = self._absjoin(path)
        try:
            os.unlink(path)
        except (IsADirectoryError, PermissionError):
            # on macOS, unlink(dir) produces PermissionError
            shutil.rmtree(path)
        except FileNotFoundError:
            pass

    def cp(self, src: str, dst: str):
        src = self._absjoin(src)
        dst = self._absjoin(dst)
        try:
            shutil.copy2(src, dst)
        except (IsADirectoryError, PermissionError):
            # on Windows, PermissionError could mean "is a directory"
            shutil.copytree(src, dst, symlinks=True)

    def link(self, src: str, dst: str):
        src = self._absjoin(src)
        dst = self._absjoin(dst)
        os.link(src, dst)

    def symlink(self, src: str, dst: str):
        dst = self._absjoin(dst)
        os.symlink(src, dst)

    def utime(self, path: str, time: int):
        path = self._absjoin(path)
        os.utime(path, (time, time))


if os.name == "nt":

    def _to_posix(path: str):
        return path.replace("\\", "/")

else:

    def _to_posix(path: str):
        return path
