#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import shutil
import subprocess
import sys
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Optional

from .prompt import prompt_confirmation


class FileHandlerReleaser(ABC):
    @abstractmethod
    def get_handle_path(self) -> Optional[Path]:
        """Returns the path to the handle tool if it exists on the system, such as handle.exe on Windows or lsof on Linux."""
        pass

    @abstractmethod
    def check_handle(self, mount: Path) -> None:
        """Displays processes keeping an open handle to files and if possible, offers to terminate them."""
        pass

    @abstractmethod
    def try_release(self, mount: Path) -> None:
        """If a handle tool exist, use it to display info to the user with check handle."""
        pass


if sys.platform == "win32":
    import psutil

    WINDOWS_HANDLE_BIN = "handle.exe"

    class WinFileHandlerReleaser(FileHandlerReleaser):
        def get_handle_path(self) -> Optional[Path]:
            return None

            handle = shutil.which(WINDOWS_HANDLE_BIN)
            if handle:
                return Path(handle)
            return None

        def check_handle(self, mount: Path) -> None:
            handle = self.get_handle_path()

            if not handle:
                return

            print(
                f"Checking handle.exe for processes using '{mount}'. This can take a while..."
            )
            print("Press ctrl+c to skip.")
            try:
                output = subprocess.check_output(
                    [
                        handle,
                        "-nobanner",
                        "/accepteula",
                        mount,
                    ]  # / vs - is importart for accepteula, otherwise it won't find handles (??)
                )
            except KeyboardInterrupt:
                print("Handle check interrupted.\n")
                print(
                    "If you want to find out which process is still using the repo, run:"
                )
                print(f"    handle.exe {mount}\n")
                return
            parsed = [
                line.split()
                for line in output.decode(errors="ignore").splitlines()
                if line
            ]
            non_edenfs_process = any(
                filter(lambda x: x[0].lower() != "edenfs.exe", parsed)
            )

            # When no handle is found in the repo, handle.exe will report `"No
            # matching handles found."`, which will be 4 words.
            if not non_edenfs_process or not parsed or len(parsed[0]) == 4:
                # Nothing other than edenfs.exe is holding handles to files from
                # the repo, we can proceed with the removal
                return

            print("The following processes are still using the repo.\n")

            pids = set()

            for executable, _, pid, _, _type, _, path in parsed:
                print(f"{executable}({pid}): {path}")
                pids.add(int(pid))

            if prompt_confirmation("Do you want to kill these processes?"):
                print("Attempting to kill all processes...")
                for pid in pids:
                    try:
                        proc = psutil.Process(pid)
                        proc.kill()
                    except Exception as e:
                        print(f"Failed to kill process {pid}: {e}")
            else:
                print(
                    f"Once you have exited those processes, delete {mount} manually.\n"
                )
            print()
            return

        def try_release(self, mount: Path) -> None:
            if self.get_handle_path():
                self.check_handle(mount)
            else:
                print(
                    f"""\
    It looks like {mount} is still in use by another process. If you need help to
    figure out which process, please try `handle.exe` from sysinternals:

    handle.exe {mount}

    """
                )
                print(
                    f"After terminating the processes, please manually delete {mount}.\n"
                )
                print()
