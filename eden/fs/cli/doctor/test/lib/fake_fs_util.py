#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


import os
import shutil

from eden.fs.cli.filesystem import FsUtil


class FakeFsUtil(FsUtil):
    def mkdir_p(self, path: str) -> str:
        return path

    # pyre-fixme[4]: Attribute must be annotated.
    total = 1000000 * 4096
    # pyre-fixme[4]: Attribute must be annotated.
    used = 500000 * 4096
    # pyre-fixme[4]: Attribute must be annotated.
    free = 500000 * 4096

    def disk_usage(self, path: str) -> shutil._ntuple_diskusage:
        # A made up filesystem with 50% free, but with other fields
        # defaulted from an EdenFS mount on Linux.
        return shutil._ntuple_diskusage(self.total, self.used, self.free)

    def rmdir(self, path: str, keep_root: bool) -> bool:
        return True
