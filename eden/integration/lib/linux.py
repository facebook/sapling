#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import ctypes
import ctypes.util
import os
import pathlib
from typing import Sequence, Type


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

    @classmethod
    def from_sys_fs_cgroup_path(cls, path: pathlib.Path) -> "LinuxCgroup":
        relative_path = path.relative_to(_cgroup_mount)
        return LinuxCgroup(b"/" + bytes(relative_path))

    @classmethod
    def from_current_process(cls) -> "LinuxCgroup":
        proc_file_content = pathlib.Path("/proc/self/cgroup").read_bytes()
        name = cls._parse_proc_file(proc_file_content)
        return cls(name)

    def query_child_cgroups(self) -> Sequence["LinuxCgroup"]:
        return [
            LinuxCgroup.from_sys_fs_cgroup_path(child)
            for child in self.sys_fs_cgroup_path.iterdir()
            if child.is_dir()
        ]

    def query_process_ids(self) -> Sequence[ProcessID]:
        pids_str = self.__cgroup_procs_path.read_text()
        return [int(line) for line in pids_str.splitlines()]

    def add_current_process(self) -> None:
        pid = os.getpid()
        with open(self.__cgroup_procs_path, "ab") as file:
            file.write(str(pid).encode("utf-8") + b"\n")

    def delete(self) -> None:
        self.sys_fs_cgroup_path.rmdir()

    def delete_recursive(self) -> None:
        for child_cgroup in self.query_child_cgroups():
            child_cgroup.delete_recursive()
        self.delete()

    @property
    def name(self) -> bytes:
        return self.__name

    @property
    def sys_fs_cgroup_path(self) -> pathlib.PosixPath:
        return pathlib.PosixPath(os.fsdecode(bytes(_cgroup_mount) + self.__name))

    @property
    def __cgroup_procs_path(self) -> pathlib.PosixPath:
        return self.sys_fs_cgroup_path / "cgroup.procs"

    def __repr__(self) -> str:
        return f"LinuxCgroup({repr(self.__name)})"

    @staticmethod
    def _parse_proc_file(file_content: bytes) -> bytes:
        lines = [line for line in file_content.split(b"\n") if line]
        if not lines:
            raise ValueError("Unexpected empty /proc/*/cgroup file")
        if len(lines) > 1:
            raise NotImplementedError(
                "Parsing /proc/*/cgroup for cgroups v1 is not supported"
            )
        [_hierarchy_id, _controller_list, name] = lines[0].split(b":", 3)
        return name


def is_cgroup_v2_mounted() -> bool:
    return _get_filesystem_statfs_type(_cgroup_mount) == _StatfsType.CGROUP2_SUPER_MAGIC


class _StatfsType:
    """statfs.f_type constants for Linux. See the statfs(2) man page."""

    CGROUP2_SUPER_MAGIC = 0x63677270


# https://github.com/python/typeshed/pull/3945
_libc = ctypes.CDLL(None, use_errno=True)
_fsword_t = ctypes.c_int64  # type: Type[ctypes._CData]
_fsblkcnt_t = ctypes.c_uint64
_fsfilcnt_t = ctypes.c_uint64
_fsid_t = ctypes.c_uint64


class _Statfs(ctypes.Structure):
    _fields_ = [
        ("f_type", _fsword_t),
        ("f_bsize", _fsword_t),
        ("f_blocks", _fsblkcnt_t),
        ("f_bfree", _fsblkcnt_t),
        ("f_bavail", _fsblkcnt_t),
        ("f_files", _fsfilcnt_t),
        ("f_ffree", _fsfilcnt_t),
        ("f_fsid", _fsid_t),
        ("f_namelen", _fsword_t),
        ("f_frsize", _fsword_t),
        ("f_flags", _fsword_t),
        ("f_spare", _fsword_t * 8),
    ]


_libc.statfs.argtypes = [ctypes.c_char_p, ctypes.POINTER(_Statfs)]
_libc.statfs.restype = ctypes.c_int


def _get_filesystem_statfs_type(path: pathlib.Path) -> int:
    """Get the type of the filesystem which the named file resides on.

    See _StatfsType for values which can be returned.
    """
    s = _Statfs()
    rc = _libc.statfs(bytes(path), ctypes.byref(s))
    if rc != 0:
        errno = ctypes.get_errno()
        raise Exception(f"failed to check filesystem type: {errno}")
    return s.f_type
