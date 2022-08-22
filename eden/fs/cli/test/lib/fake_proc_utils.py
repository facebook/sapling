#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import datetime
import errno
import os
import stat
import sys
import time
from pathlib import Path
from typing import Dict, List, Optional, Union

from eden.fs.cli.proc_utils import BuildInfo, EdenFSProcess, LinuxProcUtils


FAKE_UID = 1234


class FakeEdenFSProcess(EdenFSProcess):
    build_info: BuildInfo = BuildInfo()

    def get_build_info(self) -> BuildInfo:
        return self.build_info


_default_start_age = datetime.timedelta(hours=1)


FakeProcUtilsBase = LinuxProcUtils
if sys.platform == "win32":
    from eden.fs.cli.proc_utils_win import WinProcUtils

    FakeProcUtilsBase = WinProcUtils


class FakeProcUtils(FakeProcUtilsBase):
    _file_contents: Dict[Path, Union[bytes, Exception]] = {}
    _process_stat: Dict[int, os.stat_result] = {}
    _default_uid: int
    _build_info: Dict[int, BuildInfo] = {}

    def __init__(self, tmp_dir: str, default_uid: Optional[int] = None) -> None:
        self.proc_path = Path(tmp_dir)
        self._default_uid = FAKE_UID if default_uid is None else default_uid

    def add_process(
        self,
        pid: int,
        cmdline: List[str],
        uid: Optional[int] = None,
        comm: Optional[str] = None,
        fds: Optional[Dict[int, str]] = None,
        build_info: Optional[BuildInfo] = None,
        ppid: int = 1,
        start_age: datetime.timedelta = _default_start_age,
    ) -> None:
        pid_dir = self.proc_path / str(pid)
        pid_dir.mkdir()

        if comm is None:
            comm = os.path.basename(cmdline[0])[:15]
        (pid_dir / "comm").write_bytes(comm.encode("utf-8") + b"\n")

        cmdline_bytes = b"".join((arg.encode("utf-8") + b"\0") for arg in cmdline)
        (pid_dir / "cmdline").write_bytes(cmdline_bytes)

        start_time = time.time() - start_age.total_seconds()
        self._process_stat[pid] = self._make_fake_process_metadata(
            pid, uid=uid, start_time=start_time
        )

        fd_dir = pid_dir / "fd"
        fd_dir.mkdir()
        if fds:
            for fd, contents in fds.items():
                (fd_dir / str(fd)).symlink_to(contents)

        if build_info:
            self._build_info[pid] = build_info

        stat_contents = self.make_fake_proc_stat_contents(
            pid, command=comm, ppid=ppid, start_time=start_time
        )
        (pid_dir / "stat").write_text(stat_contents)

    def add_edenfs(
        self,
        pid: int,
        eden_dir: str,
        uid: Optional[int] = None,
        set_lockfile: bool = True,
        cmdline: Optional[List[str]] = None,
        build_time: int = 0,
        start_age: datetime.timedelta = _default_start_age,
    ) -> BuildInfo:
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
        self.add_process(
            pid,
            cmdline,
            uid=uid,
            fds=edenfs_fds,
            build_info=build_info,
            start_age=start_age,
        )

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
            ppid=pid,
            start_age=start_age,
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
        self, pid: int, uid: Optional[int], start_time: float
    ) -> os.stat_result:
        if uid is None:
            uid = self._default_uid

        return os.stat_result(
            (
                stat.S_IFDIR | 0o555,  # mode
                pid,  # inode.  We just use the pid for convenience
                4,  # dev
                9,  # nlink
                uid,  # uid
                uid,  # gid
                0,  # size
                int(start_time),  # atime
                int(start_time),  # mtime
                int(start_time),  # ctime
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
        self,
        pid: int,
        uid: int,
        cmdline: List[bytes],
        eden_dir: Optional[Path],
        holding_lock: Optional[bool],
    ) -> FakeEdenFSProcess:
        build_info = self._build_info.get(pid, BuildInfo())
        p = FakeEdenFSProcess(
            pid=pid,
            cmdline=cmdline,
            eden_dir=eden_dir,
            uid=uid,
            holding_lock=holding_lock,
        )
        p.build_info = build_info
        return p

    def _read_system_uptime(self) -> float:
        # Report an uptime of 1 year when running tests.
        # This helps ensure that we don't test with any process start times that
        # are older than the reported system uptime.
        return 60 * 60 * 24 * 365

    def make_fake_proc_stat_contents(
        self, pid: int, command: str, ppid: int, start_time: float
    ) -> str:
        time_after_boot = start_time - self.get_system_boot_time()
        assert time_after_boot >= 0
        start_time_jiffies = int(time_after_boot * self.get_jiffies_per_sec())
        stat_fields = [
            # (1) pid
            # (2) comm
            # (3) state
            ppid,  # ppid
            ppid,  # pgrp
            ppid,  # session
            0,  # tty_nr
            -1,  # tpgid
            0x400040,  # flags
            0,  # minflt
            0,  # cminflt
            0,  # majflt
            0,  # cmajflt
            5432,  # utime
            1234,  # stime
            0,  # cutime
            0,  # cstime
            20,  # priority
            0,  # nice
            1,  # num_threads
            0,  # itrealvalue
            start_time_jiffies,  # starttime
            114826723328,  # vsize
            1708100,  # rss
            0xFFFFFFFFFFFFFFFF,  # rsslim
            0x400000,  # startcode
            0x468F794,  # endcode
            0x7FFDF3C91AE0,  # startstack
            0,  # kstkesp
            0,  # kstkeip
            0,  # signal
            0,  # blocked
            0,  # sigignore
            0x4005CEE,  # sigcatch
            0,  # wchan
            0,  # nswap
            0,  # cnswap
            17,  # exit_signal
            1,  # processor
            0,  # rt_priority
            0,  # policy
            0,  # delayacct_blkio_ticks
            0,  # guest_time
            0,  # cguest_time
            73871424,  # start_data
            73987988,  # end_data
            77119488,  # start_brk
            140728693501496,  # arg_start
            140728693501673,  # arg_end
            140728693501673,  # env_start
            140728693501913,  # env_end
            0,  # exit_code
        ]
        stat_fields_str = " ".join(str(n) for n in stat_fields)
        return f"{pid} ({command}) S {stat_fields_str}\n"

    def is_process_alive(self, pid: int) -> bool:
        comm_path = self.proc_path / str(pid) / "comm"
        return comm_path.exists()

    def _get_process_command(self, pid: int) -> Optional[str]:
        comm_path = self.proc_path / str(pid) / "comm"
        try:
            return comm_path.read_text().rstrip()
        except FileNotFoundError:
            return None


def make_edenfs_build_info(build_time: int) -> BuildInfo:
    if build_time == 0:
        # Return an empty BuildInfo if the build time is 0
        return BuildInfo()

    utc = datetime.timezone.utc
    build_datetime = datetime.datetime.fromtimestamp(build_time, tz=utc)
    revision = "1" * 40
    return BuildInfo(
        package_name="fb-eden",
        package_version=build_datetime.strftime("%Y%m%d"),
        package_release=build_datetime.strftime("%H%M%S"),
        revision=revision,
        upstream_revision=revision,
        build_time=build_time,
    )
