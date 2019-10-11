#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import os
import subprocess
import typing
from typing import Dict, List, Optional, Set, Union

from eden.cli import mtab


class FakeMountTable(mtab.MountTable):
    def __init__(self) -> None:
        self.mounts: List[mtab.MountInfo] = []
        self.unmount_lazy_calls: List[bytes] = []
        self.unmount_force_calls: List[bytes] = []
        self.unmount_lazy_fails: Set[bytes] = set()
        self.unmount_force_fails: Set[bytes] = set()
        self.stats: Dict[str, Union[mtab.MTStat, Exception]] = {}
        self._next_dev: int = 10
        self.bind_mount_success_paths: Dict[str, str] = {}

    def add_mount(
        self,
        path: str,
        uid: Optional[int] = None,
        dev: Optional[int] = None,
        mode: Optional[int] = None,
        device: str = "edenfs",
        vfstype: str = "fuse",
    ) -> None:
        if uid is None:
            uid = os.getuid()
        if dev is None:
            dev = self._next_dev
        self._next_dev += 1
        if mode is None:
            mode = 16877

        self._add_mount_info(path, device=device, vfstype=vfstype)
        self.stats[path] = mtab.MTStat(st_uid=uid, st_dev=dev, st_mode=mode)
        if device == "edenfs":
            self.stats[os.path.join(path, ".eden")] = mtab.MTStat(
                st_uid=uid, st_dev=dev, st_mode=mode
            )

    def add_stale_mount(
        self, path: str, uid: Optional[int] = None, dev: Optional[int] = None
    ) -> None:
        # Stale mounts are always edenfs FUSE mounts
        self.add_mount(path, uid=uid, dev=dev)
        # Stale mounts still successfully respond to stat() calls for the root
        # directory itself, but fail stat() calls to any other path with
        # ENOTCONN
        self.fail_access(os.path.join(path, ".eden"), errno.ENOTCONN)

    def fail_access(self, path: str, errnum: int) -> None:
        self.stats[path] = OSError(errnum, os.strerror(errnum))

    def _add_mount_info(self, path: str, device: str, vfstype: str) -> None:
        self.mounts.append(
            mtab.MountInfo(
                device=device.encode("utf-8"),
                mount_point=os.fsencode(path),
                vfstype=vfstype.encode("utf-8"),
            )
        )

    def fail_unmount_lazy(self, *mounts: bytes) -> None:
        self.unmount_lazy_fails |= set(mounts)

    def fail_unmount_force(self, *mounts: bytes) -> None:
        self.unmount_force_fails |= set(mounts)

    def read(self) -> List[mtab.MountInfo]:
        return self.mounts

    def unmount_lazy(self, mount_point: bytes) -> bool:
        self.unmount_lazy_calls.append(mount_point)

        if mount_point in self.unmount_lazy_fails:
            return False
        self._remove_mount(mount_point)
        return True

    def unmount_force(self, mount_point: bytes) -> bool:
        self.unmount_force_calls.append(mount_point)

        if mount_point in self.unmount_force_fails:
            return False
        self._remove_mount(mount_point)
        return True

    def lstat(self, path: Union[bytes, str]) -> mtab.MTStat:
        # If the input is bytes decode it to a string
        if isinstance(path, bytes):
            path = os.fsdecode(path)

        try:
            result = self.stats[path]
        except KeyError:
            raise OSError(errno.ENOENT, f"no path {path}")

        if isinstance(result, BaseException):
            raise result
        else:
            # pyre-fixme[22]: The cast is redundant.
            return typing.cast(mtab.MTStat, result)

    def check_path_access(self, path: bytes) -> None:
        path_str = os.fsdecode(path)

        try:
            result = self.stats[path_str]
        except KeyError:
            raise OSError(errno.ENOENT, f"no path {path_str}")

        if isinstance(result, BaseException):
            raise result

    def _remove_mount(self, mount_point: bytes) -> None:
        self.mounts[:] = [
            mount_info
            for mount_info in self.mounts
            if mount_info.mount_point != mount_point
        ]

    def create_bind_mount(self, source_path, dest_path) -> bool:
        if (
            source_path in self.bind_mount_success_paths
            and dest_path == self.bind_mount_success_paths[source_path]
        ):
            return True

        cmd = " ".join(["sudo", "mount", "-o", "bind", source_path, dest_path])
        output = "Command returned non-zero error code"
        raise subprocess.CalledProcessError(returncode=1, cmd=cmd, output=output)
