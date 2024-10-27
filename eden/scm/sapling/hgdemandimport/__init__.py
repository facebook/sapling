# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# hgdemandimport - global demand-loading of modules for Mercurial
#
# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""demandimport - automatic demand-loading of modules"""

# This is in a separate package from mercurial because in Python 3,
# demand loading is per-package. Keeping demandimport in the mercurial package
# would disable demand loading for any modules in mercurial.

from __future__ import absolute_import

import os
import sys

from . import demandimportpy3 as demandimport

# Extensions can add to this list if necessary.
ignore = [
    "__future__",
    "_hashlib",
    # ImportError during pkg_resources/__init__.py:fixup_namespace_package
    "_imp",
    "_xmlplus",
    "fcntl",
    "nt",  # pathlib2 tests the existence of built-in 'nt' module
    "win32com.gen_py",
    "win32com.shell",  # 'appdirs' tries to import win32com.shell
    "_winreg",  # 2.7 mimetypes needs immediate ImportError
    "pythoncom",
    # imported by tarfile, not available under Windows
    "pwd",
    "grp",
    # imported by profile, itself imported by hotshot.stats,
    # not available under Windows
    "resource",
    # this trips up many extension authors
    "gtk",
    # setuptools' pkg_resources.py expects "from __main__ import x" to
    # raise ImportError if x not defined
    "__main__",
    "_ssl",  # conditional imports in the stdlib, issue1964
    "_sre",  # issue4920
    "rfc822",
    "mimetools",
    "sqlalchemy.events",  # has import-time side effects (issue5085)
    # setuptools 8 expects this module to explode early when not on windows
    "distutils.msvc9compiler",
    "__builtin__",
    "builtins",
    "urwid.command_map",  # for pudb
    # threading is locally imported by importlib.util.LazyLoader.exec_module
    "_weakrefset",
    "warnings",
    "threading",  # For RLock
    "_scandir",  # for IPython
    "collections.abc",  # for IPython - pickleshare
    "sqlite3",  # for IPython to detect missing sqlite
    "pyreadline",  # for IPython on Windows to detect missing pyreadline
    "ctypes.windll",  # for IPython on Linux to detect missing "windll"
    "sapling.progress",  # module 'sapling.progress' has no attribute 'bar'
    "multiprocessing",  # without this multiprocessing breaks on Windows pickling objects
    "multiprocessing.reduction",  # module object for 'multiprocessing.reduction' substituted in sys.modules during a lazy load
]

_pypy = "__pypy__" in sys.builtin_module_names

if _pypy:
    ignore.extend(
        [
            # _ctypes.pointer is shadowed by "from .. import pointer" (PyPy 5)
            "_ctypes.pointer"
        ]
    )

demandimport.init(ignore)

# Re-export.
isenabled = demandimport.isenabled
deactivated = demandimport.deactivated


def enable():
    # chg pre-imports modules so do not enable demandimport for it
    if (
        "CHGINTERNALMARK" not in os.environ
        and os.environ.get("HGDEMANDIMPORT") != "disable"
    ):
        demandimport.enable()
