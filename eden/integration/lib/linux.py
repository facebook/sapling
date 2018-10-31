#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import os.path
import pathlib
import typing


ProcessID = int


class LinuxCgroup:
    def __init__(self, name: bytes) -> None:
        super().__init__()
        if name[0:1] != b"/":
            raise ValueError(f"Cgroup name {repr(name)} is not absolute")
        if b"/../" in name:
            raise ValueError(
                f"Cgroup name {repr(name)} should not contain "
                f"potentially-malicious .. components"
            )
        self.__name = name

    def query_process_ids(self) -> typing.Sequence[ProcessID]:
        pids_str = (self.__path / "cgroup.procs").read_text()
        return [int(line) for line in pids_str.splitlines()]

    @property
    def __path(self) -> pathlib.PosixPath:
        return pathlib.PosixPath(os.fsdecode(b"/sys/fs/cgroup" + self.__name))

    def __repr__(self) -> str:
        return f"LinuxCgroup({repr(self.__name)})"
