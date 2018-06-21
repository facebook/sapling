#!/usr/bin/env python3
# Copyright (c) 2018-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc

from . import util


class FsUtil(abc.ABC):
    @abc.abstractmethod
    def mkdir_p(self, path: str) -> str:
        """Performs `mkdir -p <path>` and returns the path."""


class LinuxFsUtil(FsUtil):
    def mkdir_p(self, path: str) -> str:
        return util.mkdir_p(path)
