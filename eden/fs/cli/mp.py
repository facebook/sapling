#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

"""
Multiprocessing helpers for EdenFS CLI.

Fixes two problems that prevent multiprocessing 'spawn' children from
working correctly in standalone PAR builds on macOS/Windows:

1. __main__ re-import: multiprocessing.spawn re-imports the __main__
   module in the child process. In a PAR, this triggers the full import
   chain (main.py -> config.py -> thrift_clients -> folly.iobuf), which
   fails because the child doesn't have the PAR bootstrapper's custom
   import hooks and the PAR zip shadows the filesystem unpack directory
   for packages like folly. We prevent this by clearing __main__.__spec__
   and __file__ inside get_context() so the child skips the re-import
   entirely — it only needs the target function (e.g. lstat_process),
   not the full main module. This must be done inside get_context()
   rather than at module level because enable_lazy_imports defers
   module-level side effects.

2. Native library paths: Re-derives native library directories from
   sys.path (which still contains Buck2 link-tree paths even after the
   bootstrapper cleans LD_LIBRARY_PATH) and sets them as
   LD_LIBRARY_PATH / DYLD_LIBRARY_PATH in os.environ so that spawned
   child processes can find native .so/.dylib files like folly.iobuf.
"""

import multiprocessing
import multiprocessing.context
import os
import sys


def _native_lib_dirs() -> list[str]:
    """
    Collect valid directories from sys.path that may contain native libraries.

    Returns:
        List of absolute directory paths from sys.path.
    """
    dirs: list[str] = []
    for p in sys.path:
        abs_path = os.path.abspath(p)
        if os.path.isdir(abs_path):
            dirs.append(abs_path)
    return dirs


def _setup_library_paths() -> None:
    """
    Set platform-appropriate library search paths from sys.path entries.

    On Linux, sets LD_LIBRARY_PATH. On macOS, sets DYLD_LIBRARY_PATH.
    On Windows, calls os.add_dll_directory() for each directory.
    Called at import time so spawned children inherit the environment.
    """
    dirs = _native_lib_dirs()
    if not dirs:
        return

    combined = os.pathsep.join(dirs)

    if sys.platform == "win32":
        add_dll_directory = getattr(os, "add_dll_directory", None)
        if add_dll_directory is not None:
            for d in dirs:
                add_dll_directory(d)
    elif sys.platform == "darwin":
        existing = os.environ.get("DYLD_LIBRARY_PATH", "")
        if existing:
            os.environ["DYLD_LIBRARY_PATH"] = combined + os.pathsep + existing
        else:
            os.environ["DYLD_LIBRARY_PATH"] = combined
    else:
        existing = os.environ.get("LD_LIBRARY_PATH", "")
        if existing:
            os.environ["LD_LIBRARY_PATH"] = combined + os.pathsep + existing
        else:
            os.environ["LD_LIBRARY_PATH"] = combined


def _prevent_main_reimport() -> None:
    """
    Prevent multiprocessing.spawn from re-importing __main__ in children.

    In standalone PAR builds, the child process is a bare Python interpreter
    without the PAR bootstrapper's custom import hooks. When the child tries
    to re-import __main__ (eden.fs.cli.main), it triggers the full import
    chain including native extensions like folly.iobuf. These fail because
    the PAR zip claims packages like 'folly' via zipimport, but zipimport
    cannot load .so extension modules — so folly.iobuf is never found
    despite existing in the PAR's unpack directory.

    By clearing __main__.__spec__ and __file__, multiprocessing.spawn's
    get_preparation_data() won't include 'init_main_from_name' or
    'init_main_from_path', and the child will skip the __main__ re-import
    entirely. The child can still import the target function's module
    (e.g. eden.fs.cli.mtab for lstat_process) without issues since those
    modules don't trigger the problematic import chain.

    We access __main__ via sys.modules to bypass the lazy import proxy
    that enable_lazy_imports creates.
    """
    main_mod = sys.modules.get("__main__")
    if main_mod is None:
        return

    if getattr(main_mod, "__spec__", None) is not None:
        main_mod.__spec__ = None  # type: ignore[assignment]

    if hasattr(main_mod, "__file__"):
        try:
            del main_mod.__file__
        except AttributeError:
            pass


_setup_library_paths()


def get_context() -> multiprocessing.context.DefaultContext:
    """
    Return the platform-default multiprocessing context.

    Clears __main__.__spec__ on each call to prevent multiprocessing.spawn
    from re-importing __main__ in the child process. This must be done here
    (not at module level) because enable_lazy_imports defers module-level
    side effects, and we need the fix applied before Process.start()
    captures __main__.__spec__ for the child.

    Returns:
        The default multiprocessing context for the current platform.
    """
    _prevent_main_reimport()
    # pyre-ignore[7]: multiprocessing.get_context() is typed as BaseContext
    # but actually returns DefaultContext at runtime.
    return multiprocessing.get_context()
