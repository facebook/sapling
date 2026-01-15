# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import platform
import sys

# Fix for Windows DLL loading issue - DLLs in PAR need to be found
# See D78185397 and D78508209 for similar fixes
if platform.system() == "Windows":
    # pyre-ignore[16]: Suppress lint error for undefined attribute
    if add_dll_directory := getattr(os, "add_dll_directory", None):
        for p in sys.path:
            abs_path = os.path.abspath(p)
            if (
                ("pex" in abs_path or "config" in abs_path)
                and os.path.exists(abs_path)
                and os.path.isdir(abs_path)
            ):
                add_dll_directory(abs_path)
