# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import multiprocessing
import os
import random
import time
from multiprocessing import Process
from typing import Union

from eden.fs.cli import mtab

from eden.fs.cli.doctor.test.lib.fake_mount_table import FakeMountTable


def lstat_process_hang(path: Union[bytes, str]) -> None:
    time.sleep(600000)
    exit(0)


class FakeHangMountTable(FakeMountTable):
    """
    Test only fake mount table that will hang
    """

    def create_lstat_process(
        self,
        path: bytes,
    ) -> Process:
        return multiprocessing.Process(
            target=lstat_process_hang,
            args=(os.path.join(path, hex(random.getrandbits(32))[2:].encode()),),
        )

    def check_path_access(
        self,
        path: bytes,
        mount_type: bytes,
    ) -> None:
        mount_type_str = "fuse"
        mtab.MountTable.check_path_access(self, path, bytes(mount_type_str, "utf-8"))
