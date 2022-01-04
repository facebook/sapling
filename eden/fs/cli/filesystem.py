#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import abc
import os
import shutil

from . import util


class FsUtil(abc.ABC):
    @abc.abstractmethod
    def mkdir_p(self, path: str) -> str:
        """Performs `mkdir -p <path>` and returns the path."""

    @abc.abstractmethod
    def disk_usage(self, path: str) -> shutil._ntuple_diskusage:
        """Calls os.statvfs on the mount"""


class RealFsUtil(FsUtil):
    def mkdir_p(self, path: str) -> str:
        return util.mkdir_p(path)

    def disk_usage(self, path: str) -> shutil._ntuple_diskusage:
        return shutil.disk_usage(path)


def new() -> FsUtil:
    return RealFsUtil()
