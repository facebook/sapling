#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import ctypes
import enum
import sys
from pathlib import Path

if sys.platform == "win32":
    import ctypes.wintypes
    from ctypes.wintypes import LPCWSTR


class PRJ_FILE_STATE(enum.IntFlag):
    Placeholder = 1
    HydratedPlaceholder = 2
    DirtyPlaceholder = 4
    Full = 8
    Tombstone = 16


if sys.platform == "win32":
    prjfs = ctypes.windll.projectedfslib

    _PrjGetOnDiskFileState = prjfs.PrjGetOnDiskFileState
    _PrjGetOnDiskFileState.restype = ctypes.HRESULT
    _PrjGetOnDiskFileState.argtypes = [
        ctypes.wintypes.LPCWSTR,
        ctypes.POINTER(ctypes.c_int),
    ]

    def PrjGetOnDiskFileState(path: Path) -> PRJ_FILE_STATE:
        state = ctypes.c_int(0)
        result = _PrjGetOnDiskFileState(LPCWSTR(str(path)), ctypes.byref(state))

        # Python will automatically throw OSError when result is non-zero
        if result == 0:
            return PRJ_FILE_STATE(state.value)

        # It should never reach here, but just to make typechecker happy
        raise RuntimeError("Windows Error: " + result)
