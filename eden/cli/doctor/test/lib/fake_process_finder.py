#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import os
from pathlib import Path
from typing import Dict, List, Union

from eden.cli import process_finder


class FakeProcessFinder(process_finder.LinuxProcessFinder):
    def __init__(self, tmp_dir: str) -> None:
        self.proc_path = Path(tmp_dir)
        self._file_contents: Dict[Path, Union[bytes, Exception]] = {}

    def add_process(self, pid: int, cmdline: List[str]) -> None:
        pid_dir = self.proc_path / str(pid)
        pid_dir.mkdir()

        command = os.path.basename(cmdline[0])
        (pid_dir / "comm").write_bytes(command.encode("utf-8") + b"\n")

        cmdline_bytes = b"".join((arg.encode("utf-8") + b"\0") for arg in cmdline)
        (pid_dir / "cmdline").write_bytes(cmdline_bytes)

    def add_edenfs(self, pid: int, eden_dir: str, set_lockfile: bool = True) -> None:
        if set_lockfile:
            self.set_file_contents(Path(eden_dir) / "lock", f"{pid}\n".encode("utf-8"))

        cmdline = [
            "/usr/bin/edenfs",
            "--edenfs",
            "--edenDir",
            eden_dir,
            "--etcEdenDir",
            "/etc/eden",
            "--configPath",
            "/home/user/.edenrc",
        ]
        self.add_process(pid, cmdline)

    def set_file_contents(self, path: Union[Path, str], contents: bytes) -> None:
        self._file_contents[Path(path)] = contents

    def set_file_exception(self, path: Union[Path, str], exception: Exception) -> None:
        self._file_contents[Path(path)] = exception

    def read_lock_file(self, path: Path) -> bytes:
        contents = self._file_contents.get(path, None)
        if contents is None:
            raise FileNotFoundError(errno.ENOENT, str(path))
        if isinstance(contents, Exception):
            raise contents
        return contents
