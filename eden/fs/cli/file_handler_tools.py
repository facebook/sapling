#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import re
import shutil
import subprocess
import sys
from abc import ABC, abstractmethod
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, TYPE_CHECKING

from .prompt import prompt_confirmation

if TYPE_CHECKING:
    from .config import EdenInstance


@dataclass
class FileHandleEntry:
    process_name: str
    process_id: str
    resource_type: str
    path: str


class FileReleaseStatus:
    def __init__(self, eden_instance: "EdenInstance", mount: Path) -> None:
        self.mount: Path = mount
        self.handle_found: bool = False
        self.keyboard_interrupt: bool = False
        self.conflict_processes: List[str] = []
        self.unkillable_processes: List[str] = []
        self.user_wants_to_kill: bool = False
        self.exception_raised: Optional[str] = None
        self.eden_instance = eden_instance

    def log_release_outcome(self, success: bool) -> None:
        self.eden_instance.log_sample(
            "rm_open_files",
            mount=str(self.mount),
            conflict_processes=self.conflict_processes,
            unkillable_processes=self.unkillable_processes,
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

        def parse_handlerexe_output(self, output: str) -> List[FileHandleEntry]:
            r"""
            Parses the output of handle.exe and returns a list of FileHandleEntry objects.

            Lines that we care about look like this:
            VS Code @ FB.exe   pid: 19044  type: File           34C: C:\open\fbsource2
            Hubbub.exe         pid: 24856  type: File            40: C:\open\fbsource\ovrsource-legacy\unity\socialvr\_tools\hubbub
            Note, no tabs, process names may contain spaces, best we can do is probably regex here, naive split is not enough
            """

            p = re.compile(
                r"^\s*(.+?)\s*pid: ([0-9]+)\s*type: ([^ ]*)\s*([^ ]*)\s*(.*?): (.*)"
            )
            ret = []
            for line in output.splitlines():
                if not line:
                    continue
                m = p.findall(line)
                if m and len(m) == 1 and len(m[0]) == 6:
                    ret.append(
                        FileHandleEntry(
                            m[0][0],  # Process name
                            m[0][1],  # Process id
                            m[0][2],  # Type
                            m[0][5],  # Name
                        )
                    )
            return ret

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
                        frs.unkillable_processes.append(executable)
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

        def stop_adb_server(self) -> None:
            """adb (Android Debug Bridge) is a usual suspect hanging on to directories.
            Terminating it is harmless; it will be restarted on demand.
            """
            try:
                subprocess.check_output(
                    [
                        "adb",
                        "kill-server",
                    ]
                )
            except FileNotFoundError:
                # adb is not installed, no need to stop it.
                pass
            except subprocess.CalledProcessError as e:
                # Note that if adb-server is not running the error code will still be 0 (and a message written to console),
                # but just in case there's any other cases, log error codes.
                print(f"adb returned {e.errorcode} when trying to stop adb server.")

        def stop_buck2(self) -> None:
            """buck2 server seems to like cwd'ing into places and staying there.
            Terminating it is harmless; it will be restarted on demand.
            """
            try:
                subprocess.check_output(
                    [
                        "buck2",
                        "kill",
                    ]
                )
            except FileNotFoundError:
                # buck2d is not installed
                print("buck2 not found, this system might be really broken.")
                pass
            except subprocess.CalledProcessError as e:
                # Note that if buck2 server is not running the error code will still be 0 (and a message written to console),
                # but just in case there's any other cases, log error codes.
                print(f"buck2 returned {e.errorcode} when trying to stop buckd server.")
