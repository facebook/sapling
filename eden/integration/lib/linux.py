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
import subprocess
import typing


ProcessID = int


_cgroup_mount = pathlib.PosixPath("/sys/fs/cgroup")


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
        return pathlib.PosixPath(os.fsdecode(bytes(_cgroup_mount) + self.__name))

    def __repr__(self) -> str:
        return f"LinuxCgroup({repr(self.__name)})"


def is_cgroup_v2_mounted() -> bool:
    return _get_filesystem_statfs_type(_cgroup_mount) == _StatfsType.CGROUP2_SUPER_MAGIC


class _StatfsType:
    """statfs.f_type constants for Linux. See the statfs(2) man page.
    """

    CGROUP2_SUPER_MAGIC = 0x63677270


def _get_filesystem_statfs_type(path: pathlib.Path) -> int:
    """Get the type of the filesystem which the named file resides on.

    See _StatfsType for values which can be returned.
    """
    # TODO(strager): Call the statfs C API directly.
    filesystem_type_hex = subprocess.check_output(
        ["/bin/stat", "--file-system", "--printf=%t", "--", path],
        stderr=subprocess.STDOUT,
    )
    return int(filesystem_type_hex, 16)
