#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre_strict

import datetime
import errno
import os
import stat
import time
from pathlib import Path
from typing import Dict, List, Optional, Union

from eden.cli import process_finder


class FakeEdenFSProcess(process_finder.EdenFSProcess):
    build_info: process_finder.BuildInfo = process_finder.BuildInfo()

    def get_build_info(self) -> process_finder.BuildInfo:
        return self.build_info


class FakeProcessFinder(process_finder.LinuxProcessFinder):
    _file_contents: Dict[Path, Union[bytes, Exception]] = {}
    _process_stat: Dict[int, os.stat_result] = {}
    _default_uid: int
    _build_info: Dict[int, process_finder.BuildInfo] = {}

    def __init__(self, tmp_dir: str, default_uid: Optional[int] = None) -> None:
        self.proc_path = Path(tmp_dir)
        self._default_uid = os.getuid() if default_uid is None else default_uid

    def add_process(
        self,
        pid: int,
        cmdline: List[str],
        uid: Optional[int] = None,
        comm: Optional[str] = None,
        fds: Optional[Dict[int, str]] = None,
        build_info: Optional[process_finder.BuildInfo] = None,
    ) -> None:
        pid_dir = self.proc_path / str(pid)
        pid_dir.mkdir()

        if comm is None:
            comm = os.path.basename(cmdline[0])[:15]
        (pid_dir / "comm").write_bytes(comm.encode("utf-8") + b"\n")

        cmdline_bytes = b"".join((arg.encode("utf-8") + b"\0") for arg in cmdline)
        (pid_dir / "cmdline").write_bytes(cmdline_bytes)

        self._process_stat[pid] = self._make_fake_process_metadata(pid, uid)

        fd_dir = pid_dir / "fd"
        fd_dir.mkdir()
        if fds:
            for fd, contents in fds.items():
                (fd_dir / str(fd)).symlink_to(contents)

        if build_info:
            self._build_info[pid] = build_info

    def add_edenfs(
        self,
        pid: int,
        eden_dir: str,
        uid: Optional[int] = None,
        set_lockfile: bool = True,
        cmdline: Optional[List[str]] = None,
        build_time: int = 0,
    ) -> process_finder.BuildInfo:
        """Add a fake EdenFS instance.
        Note that this will add 2 processes: the main EdenFS process with the
        specified PID, and its corresponding privhelper process using PID+1
        """
        lock_path = Path(eden_dir) / "lock"
        lock_symlink = str(lock_path)
        if set_lockfile:
            self.set_file_contents(lock_path, f"{pid}\n".encode("utf-8"))
        else:
            lock_symlink += " (deleted)"

        if cmdline is None:
            cmdline = [
                "/usr/bin/edenfs",
                "--edenfs",
                "--edenfsctlPath",
                "/usr/local/bin/edenfsctl",
                "--edenDir",
                eden_dir,
                "--etcEdenDir",
                "/etc/eden",
                "--configPath",
                "/home/user/.edenrc",
            ]
        log_path = str(Path(eden_dir) / "logs" / "edenfs.log")
        edenfs_fds = {
            0: "/dev/null",
            1: log_path,
            2: log_path,
            4: "socket:[1234]",
            8: lock_symlink,
        }

        build_info = make_edenfs_build_info(build_time)

        # Add the main EdenFS process
        self.add_process(pid, cmdline, uid=uid, fds=edenfs_fds, build_info=build_info)

        # Also add a privhelper process
        # Newer versions of EdenFS name this process "edenfs_privhelp", but older
        # versions call it just "edenfs".  Continue calling it "edenfs" here for now
        # until we know all privhelper processes using the old "edenfs" have been
        # restarted.
        privhelper_fds = {0: "/dev/null", 1: log_path, 2: log_path, 5: "socket:[1235]"}
        self.add_process(
            pid + 1,
            cmdline,
            uid=0,
            comm="edenfs",
            fds=privhelper_fds,
            build_info=build_info,
        )
        return build_info

    def set_file_contents(self, path: Union[Path, str], contents: bytes) -> None:
        self._file_contents[Path(path)] = contents

    def set_file_exception(self, path: Union[Path, str], exception: Exception) -> None:
        self._file_contents[Path(path)] = exception

    def stat_process_dir(self, path: Path) -> os.stat_result:
        try:
            if path.parent != self.proc_path:
                raise ValueError()
            pid = int(path.name)
            return self._process_stat[pid]
        except (ValueError, KeyError):
            raise FileNotFoundError(errno.ENOENT, "No such file or directory")

    def _make_fake_process_metadata(
        self, pid: int, uid: Optional[int]
    ) -> os.stat_result:
        if uid is None:
            uid = self._default_uid
        start_time = int(time.time())

        return os.stat_result(
            (
                stat.S_IFDIR | 0o555,  # mode
                pid,  # inode.  We just use the pid for convenience
                4,  # dev
                9,  # nlink
                uid,  # uid
                uid,  # gid
                0,  # size
                start_time,  # atime
                start_time,  # mtime
                start_time,  # ctime
            )
        )

    def read_lock_file(self, path: Path) -> bytes:
        contents = self._file_contents.get(path, None)
        if contents is None:
            raise FileNotFoundError(errno.ENOENT, str(path))
        if isinstance(contents, Exception):
            raise contents
        return contents

    def make_edenfs_process(
        self, pid: int, cmdline: List[bytes], eden_dir: Optional[Path], uid: int
    ) -> FakeEdenFSProcess:
        build_info = self._build_info.get(pid, process_finder.BuildInfo())
        p = FakeEdenFSProcess(pid=pid, cmdline=cmdline, eden_dir=eden_dir, uid=uid)
        p.build_info = build_info
        return p


def make_edenfs_build_info(build_time: int) -> process_finder.BuildInfo:
    if build_time == 0:
        # Return an empty BuildInfo if the build time is 0
        return process_finder.BuildInfo()

    utc = datetime.timezone.utc
    build_datetime = datetime.datetime.fromtimestamp(build_time, tz=utc)
    revision = "1" * 40
    return process_finder.BuildInfo(
        package_name="fb-eden",
        package_version=build_datetime.strftime("%Y%m%d"),
        package_release=build_datetime.strftime("%H%M%S"),
        revision=revision,
        upstream_revision=revision,
        build_time=build_time,
    )
