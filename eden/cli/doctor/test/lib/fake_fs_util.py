#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from typing import Any, cast

from eden.cli.filesystem import FsUtil


class FakeFsUtil(FsUtil):
    def mkdir_p(self, path: str) -> str:
        return path

    f_bsize = 131072
    f_frsize = 4096
    f_blocks = 1000000
    f_bfree = 500000
    f_bavail = 500000
    f_files = 0
    f_ffree = 0
    f_favail = 0
    f_flag = 4098
    f_namemax = 255

    def statvfs(self, path: str) -> os.statvfs_result:
        # A made up filesystem with 50% free, but with other fields
        # defaulted from an EdenFS mount on Linux.
        return cast(Any, os.statvfs_result)(
            (
                self.f_bsize,
                self.f_frsize,
                self.f_blocks,
                self.f_bfree,
                self.f_bavail,
                self.f_files,
                self.f_ffree,
                self.f_favail,
                self.f_flag,
                self.f_namemax,
            )
        )
