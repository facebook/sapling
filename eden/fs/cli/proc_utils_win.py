#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import ctypes
import sys
from ctypes.wintypes import (
    BOOL as _BOOL,
    DWORD as _DWORD,
    HANDLE as _HANDLE,
    LPSTR as _LPSTR,
    WORD as _WORD,
)
from typing import Iterable

from . import proc_utils


if sys.platform == "win32":
    _win32 = ctypes.windll.kernel32
    _win32.OpenProcess.argtypes = [_DWORD, _BOOL, _DWORD]
    _win32.OpenProcess.restype = _HANDLE
else:
    # This entire file is only ever imported in Windows.  However on our continuous
    # integration environments Pyre currently does all of its type checking assuming
    # Linux.  Define a fake kernel32 module just to allow it to still perform some type
    # checking for us.
    class _win32:
        @staticmethod
        def OpenProcess(desired_access: int, inherit_handle: bool, pid: int) -> _HANDLE:
            ...

        @staticmethod
        def GetLastError() -> int:
            ...

        @staticmethod
        def CloseHandle(handle: _HANDLE) -> None:
            ...

        @staticmethod
        def CreateProcessW(*args) -> bool:
            ...


_PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
_ERROR_ACCESS_DENIED = 5
_CREATE_NO_WINDOW = 0x08000000


class _STARTUPINFO(ctypes.Structure):
    _fields_ = [
        ("cb", _DWORD),
        ("lpReserved", _LPSTR),
        ("lpDesktop", _LPSTR),
        ("lpTitle", _LPSTR),
        ("dwX", _DWORD),
        ("dwY", _DWORD),
        ("dwXSize", _DWORD),
        ("dwYSize", _DWORD),
        ("dwXCountChars", _DWORD),
        ("dwYCountChars", _DWORD),
        ("dwFillAttribute", _DWORD),
        ("dwFlags", _DWORD),
        ("wShowWindow", _WORD),
        ("cbReserved2", _WORD),
        ("lpReserved2", ctypes.c_char_p),
        ("hStdInput", _HANDLE),
        ("hStdOutput", _HANDLE),
        ("hStdError", _HANDLE),
    ]


class _PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("hProcess", _HANDLE),
        ("hThread", _HANDLE),
        ("dwProcessId", _DWORD),
        ("dwThreadId", _DWORD),
    ]


def create_process_shim(cmd_str: str) -> None:
    si = _STARTUPINFO()
    si.cb = ctypes.sizeof(_STARTUPINFO)  # pyre-ignore[16]
    pi = _PROCESS_INFORMATION()

    res = _win32.CreateProcessW(
        None,
        cmd_str,
        None,
        None,
        False,
        _CREATE_NO_WINDOW,
        None,
        None,
        ctypes.byref(si),
        ctypes.byref(pi),
    )

    if not res:
        raise ctypes.WinError()  # pyre-ignore[16]

    _win32.CloseHandle(pi.hProcess)
    _win32.CloseHandle(pi.hThread)


class WinProcUtils(proc_utils.ProcUtils):
    def get_edenfs_processes(self) -> Iterable[proc_utils.EdenFSProcess]:
        # TODO: Finding all EdenFS processes is not yet implemented on Windows
        # This function is primarily used by `eden doctor` and other tools looking for
        # stale EdenFS instances on the system.  Returning an empty list for now will
        # allow those tools to run but just not find any stale processes.
        return []

    def get_process_start_time(self, pid: int) -> float:
        raise NotImplementedError(
            "Windows does not currently implement get_process_start_time()"
        )

    def is_process_alive(self, pid: int) -> bool:
        handle = _win32.OpenProcess(_PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
        if handle is None:
            error = _win32.GetLastError()
            if error == _ERROR_ACCESS_DENIED:
                # The process exists, but we don't have permission to query it.
                return True
            return False
        _win32.CloseHandle(handle)
        return True

    def is_edenfs_process(self, pid: int) -> bool:
        # For now we just check that the process exists.
        # In the future it might be nice to also check if the command line or executable
        # looks like EdenFS, but this is sufficient for now.
        return self.is_process_alive(pid)
