#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import os

from . import util


class FsUtil(abc.ABC):
    @abc.abstractmethod
    def mkdir_p(self, path: str) -> str:
        """Performs `mkdir -p <path>` and returns the path."""

    @abc.abstractmethod
    def statvfs(self, path: str) -> os.statvfs_result:
        """Calls os.statvfs on the mount"""


class LinuxFsUtil(FsUtil):
    def mkdir_p(self, path: str) -> str:
        return util.mkdir_p(path)

    def statvfs(self, path: str) -> os.statvfs_result:
        return os.statvfs(path)
