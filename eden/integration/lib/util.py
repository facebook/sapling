#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import sys
from typing import Callable, List, Optional

if sys.platform == "win32":
    import ctypes
    from ctypes.wintypes import DWORD as _DWORD, HANDLE as _HANDLE, LPCWSTR as _LPCWSTR

    from eden.fs.cli.proc_utils_win import Handle


def gen_tree(
    path: str,
    fanouts: List[int],
    leaf_function: Callable[[str], None],
    internal_function: Optional[Callable[[str], None]] = None,
) -> None:
    """
    Helper function for recursively building a large branching directory
    tree.

    path is the leading path prefix to put before all directory names.

    fanouts is an array of integers specifying the directory fan-out
    dimensions.  One layer of directories will be created for each element
    in this array.  e.g., [3, 4] would create 3 subdirectories inside the
    top-level directory, and 4 subdirectories in each of those 3
    directories.

    Calls leaf_function on all leaf directories.
    Calls internal_function on all internal (non-leaf) directories.
    """
    for n in range(fanouts[0]):
        subdir = os.path.join(path, "dir{:02}".format(n + 1))
        sub_fanouts = fanouts[1:]
        if sub_fanouts:
            if internal_function is not None:
                internal_function(subdir)
            gen_tree(subdir, fanouts[1:], leaf_function, internal_function)
        else:
            leaf_function(subdir)


if sys.platform == "win32":

    def open_locked(path: str, directory: bool = False) -> Handle:
        win32 = ctypes.windll.kernel32
        win32.CreateFileW.argtypes = [
            _LPCWSTR,
            _DWORD,
            _DWORD,
            ctypes.c_void_p,
            _DWORD,
            _DWORD,
            ctypes.c_void_p,
        ]
        win32.CreateFileW.restype = _HANDLE

        GENERIC_READ = 0x80000000
        OPEN_EXISTING = 3
        FILE_ATTRIBUTE_NORMAL = 0x80
        FILE_FLAG_BACKUP_SEMANTICS = 0x02000000
        INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

        flags = FILE_ATTRIBUTE_NORMAL
        if directory:
            flags |= FILE_FLAG_BACKUP_SEMANTICS
        fhandle = win32.CreateFileW(
            path, GENERIC_READ, 0, None, OPEN_EXISTING, flags, None
        )
        if fhandle == INVALID_HANDLE_VALUE:
            raise ctypes.WinError(ctypes.get_last_error())
        return Handle(fhandle)
