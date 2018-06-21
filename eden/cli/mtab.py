#!/usr/bin/env python3
# Copyright (c) 2018-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import logging
import os
import subprocess
from typing import List, NamedTuple, Union


log = logging.getLogger("eden.cli.mtab")


MountInfo = NamedTuple(
    "MountInfo", [("device", bytes), ("mount_point", bytes), ("vfstype", bytes)]
)


MTStat = NamedTuple("MTStat", [("st_uid", int), ("st_dev", int), ("st_mode", int)])


class MountTable(abc.ABC):
    @abc.abstractmethod
    def read(self) -> List[MountInfo]:
        "Returns the list of system mounts."

    @abc.abstractmethod
    def unmount_lazy(self, mount_point: bytes) -> bool:
        "Corresponds to `umount -l` on Linux."

    @abc.abstractmethod
    def unmount_force(self, mount_point: bytes) -> bool:
        "Corresponds to `umount -f` on Linux."

    @abc.abstractmethod
    def lstat(self, path: Union[bytes, str]) -> MTStat:
        "Returns a subset of the results of os.lstat."

    @abc.abstractmethod
    def create_bind_mount(self, source_path, dest_path) -> bool:
        "Creates a bind mount from source_path to dest_path."


def parse_mtab(contents: bytes) -> List[MountInfo]:
    mounts = []
    for line in contents.splitlines():
        # columns split by space or tab per man page
        entries = line.split()
        if len(entries) != 6:
            log.warning(f"mount table line has {len(entries)} entries instead of 6")
            continue
        device, mount_point, vfstype, opts, freq, passno = entries
        mounts.append(
            MountInfo(device=device, mount_point=mount_point, vfstype=vfstype)
        )
    return mounts


class LinuxMountTable(MountTable):
    def read(self) -> List[MountInfo]:
        # What's the most portable mtab path? I've seen both /etc/mtab and
        # /proc/self/mounts.  CentOS 6 in particular does not symlink /etc/mtab
        # to /proc/self/mounts so go directly to /proc/self/mounts.
        # This code could eventually fall back to /proc/mounts and /etc/mtab.
        with open("/proc/self/mounts", "rb") as f:
            return parse_mtab(f.read())

    def unmount_lazy(self, mount_point: bytes) -> bool:
        # MNT_DETACH
        return 0 == subprocess.call(["sudo", "umount", "-l", mount_point])

    def unmount_force(self, mount_point: bytes) -> bool:
        # MNT_FORCE
        return 0 == subprocess.call(["sudo", "umount", "-f", mount_point])

    def lstat(self, path: Union[bytes, str]) -> MTStat:
        st = os.lstat(path)
        return MTStat(st_uid=st.st_uid, st_dev=st.st_dev, st_mode=st.st_mode)

    def create_bind_mount(self, source_path, dest_path) -> bool:
        return 0 == subprocess.check_call(
            ["sudo", "mount", "-o", "bind", source_path, dest_path]
        )
