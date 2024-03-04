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
from typing import List, Optional, TYPE_CHECKING

from .prompt import prompt_confirmation

if TYPE_CHECKING:
    from .config import EdenInstance


class FileReleaseStatus:
    def __init__(self, eden_instance: "EdenInstance", mount: Path) -> None:
        self.mount: Path = mount
        self.handle_found: bool = False
        self.keyboard_interrupt: bool = False
        self.conflict_processes: List[str] = []
        self.failed_to_kill: Optional[str] = None
        self.user_wants_to_kill: bool = False
        self.exception_raised: Optional[str] = None
        self.eden_instance = eden_instance

    def log_release_outcome(self, success: bool) -> None:
        self.eden_instance.log_sample(
            "rm_open_files",
            mount=str(self.mount),
            conflict_processes=self.conflict_processes,
            failed_to_kill=self.failed_to_kill if self.failed_to_kill else "",
            want_kill=self.user_wants_to_kill,
            exception=str(self.exception_raised) if self.exception_raised else "",
            success=success,
        )


class FileHandlerReleaser(ABC):
    def __init__(self, eden_instance: "EdenInstance") -> None:
        self.eden_instance = eden_instance

    @abstractmethod
    def get_handle_path(self) -> Optional[Path]:
        """Returns the path to the handle tool if it exists on the system, such as handle.exe on Windows or lsof on Linux."""
        pass

    @abstractmethod
    def check_handle(self, mount: Path, frs: FileReleaseStatus) -> bool:
        """Displays processes keeping an open handle to files and if possible, offers to terminate them."""
        return False

    @abstractmethod
    def try_release(self, mount: Path) -> bool:
        """If a handle tool exist, use it to display info to the user with check handle."""
        return False


if sys.platform == "win32":
    WINDOWS_HANDLE_BIN = "handle.exe"

    class WinFileHandlerReleaser(FileHandlerReleaser):
        def get_handle_path(self) -> Optional[Path]:
            handle = shutil.which(WINDOWS_HANDLE_BIN)
            if handle:
                return Path(handle)
            return None

        def check_handle(self, mount: Path, frs: FileReleaseStatus) -> bool:
            try:
                import psutil
            except Exception as e:
                frs.exception_raised = e
                return False

            handle = self.get_handle_path()
            if not handle:
                return False

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
                frs.keyboard_interrupt = True
                return False

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
                print(
                    "No processes found. They may be running under a different user.\n"
                )
                return False

            print("The following processes are still using the repo.\n")

            for executable, _, pid, _, _type, _, path in parsed:
                print(f"{executable}({pid}): {path}")

            frs.conflict_processes = [parse[0] for parse in parsed]

            if prompt_confirmation("Do you want to kill these processes?"):
                frs.user_wants_to_kill = True
                print("Attempting to kill all processes...")
                for executable, _, pid, _, _type, _, _path in parsed:
                    try:
                        proc = psutil.Process(int(pid))
                        proc.kill()
                        proc.wait()
                    except Exception as e:
                        print(f"Failed to kill process {executable} {pid}: {e}")
                        frs.failed_to_kill = executable
                        frs.exception_raised = e
                        return False
            else:
                print(
                    f"Once you have exited those processes, delete {mount} manually.\n"
                )
                return False
            print()
            return True

        def try_release(self, mount: Path) -> bool:
            try:
                frs: FileReleaseStatus = FileReleaseStatus(self.eden_instance, mount)
                if self.get_handle_path():
                    frs.handle_found = True
                    ret = self.check_handle(mount, frs)
                    frs.log_release_outcome(ret)
                    return ret
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
                    frs.log_release_outcome(False)
                    return False
            except (
                Exception
            ) as e:  # Hopefully never here but let's give us a chance to log it
                print(f"Exception raised when trying to release file: {e}")
                frs.exception_raised = e
                frs.log_release_outcome(False)
                raise
