# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import random
import time
from multiprocessing.connection import Connection
from typing import Union

from eden.fs.cli import mtab
from eden.fs.cli.doctor.test.lib.fake_mount_table import FakeMountTable
from eden.fs.cli.mp import get_context


def lstat_process_hang(path: Union[bytes, str], result_writer: Connection) -> None:
    time.sleep(600000)


class FakeHangMountTable(FakeMountTable):
    """
    Test only fake mount table that will hang
    """

    def create_lstat_process(
        self,
        path: bytes,
    ) -> mtab.LstatProcess:
        context = get_context()
        result_reader, result_writer = context.Pipe(duplex=False)
        return mtab.LstatProcess(
            process=context.Process(
                target=lstat_process_hang,
                args=(
                    os.path.join(path, hex(random.getrandbits(32))[2:].encode()),
                    result_writer,
                ),
            ),
            result_reader=result_reader,
            result_writer=result_writer,
        )

    def check_path_access(
        self,
        path: bytes,
        mount_type: bytes,
    ) -> None:
        mount_type_str = "fuse"
        mtab.MountTable.check_path_access(self, path, bytes(mount_type_str, "utf-8"))
