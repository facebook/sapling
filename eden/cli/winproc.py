#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import ctypes
import os
from ctypes.wintypes import (
    DWORD as _DWORD,
    HANDLE as _HANDLE,
    LPSTR as _LPSTR,
    WORD as _WORD,
)
from typing import List, Optional

from .config import EdenInstance


EDENFS_DEFAULT = os.path.join("C:\\", "tools", "eden", "edenfs", "edenfs.exe")


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


_CREATE_NO_WINDOW = 0x08000000
_SW_HIDE = 0


def start_process(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
):
    edenfs_bin = EDENFS_DEFAULT
    if daemon_binary:
        edenfs_bin = daemon_binary

    if not os.path.isfile(edenfs_bin):
        raise Exception("edenfs is not found: {}".format(edenfs_bin))

    cmd = [
        edenfs_bin,
        "--edenDir",
        str(instance._config_dir),
        "--etcEdenDir",
        str(instance._etc_eden_dir),
        "--configPath",
        str(instance._user_config_path),
    ]
    cmd_str = " ".join(cmd)

    si = _STARTUPINFO()
    si.cb = ctypes.sizeof(_STARTUPINFO)
    pi = _PROCESS_INFORMATION()

    res = ctypes.windll.kernel32.CreateProcessW(
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
        raise ctypes.WinError()

    ctypes.windll.kernel32.CloseHandle(pi.hProcess)
    ctypes.windll.kernel32.CloseHandle(pi.hThread)
    print("Edenfs started")
