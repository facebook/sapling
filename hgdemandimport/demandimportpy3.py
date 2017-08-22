# demandimportpy3 - global demand-loading of modules for Mercurial
#
# Copyright 2017 Facebook Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Lazy loading for Python 3.6 and above.

This uses the new importlib finder/loader functionality available in Python 3.5
and up. The code reuses most of the mechanics implemented inside importlib.util,
but with a few additions:

* Allow excluding certain modules from lazy imports.
* Expose an interface that's substantially the same as demandimport for
  Python 2.

This also has some limitations compared to the Python 2 implementation:

* Much of the logic is per-package, not per-module, so any packages loaded
  before demandimport is enabled will not be lazily imported in the future. In
  practice, we only expect builtins to be loaded before demandimport is
  enabled.
"""

# This line is unnecessary, but it satisfies test-check-py3-compat.t.
from __future__ import absolute_import

import contextlib
import importlib.abc
import importlib.machinery
import importlib.util
import sys

_deactivated = False

class _lazyloaderex(importlib.util.LazyLoader):
    """This is a LazyLoader except it also follows the _deactivated global and
    the ignore list.
    """
    def exec_module(self, module):
        """Make the module load lazily."""
        if _deactivated or module.__name__ in ignore:
            self.loader.exec_module(module)
        else:
            super().exec_module(module)

# This is 3.6+ because with Python 3.5 it isn't possible to lazily load
# extensions. See the discussion in https://python.org/sf/26186 for more.
_extensions_loader = _lazyloaderex.factory(
    importlib.machinery.ExtensionFileLoader)
_bytecode_loader = _lazyloaderex.factory(
    importlib.machinery.SourcelessFileLoader)
_source_loader = _lazyloaderex.factory(importlib.machinery.SourceFileLoader)

def _makefinder(path):
    return importlib.machinery.FileFinder(
        path,
        # This is the order in which loaders are passed in in core Python.
        (_extensions_loader, importlib.machinery.EXTENSION_SUFFIXES),
        (_source_loader, importlib.machinery.SOURCE_SUFFIXES),
        (_bytecode_loader, importlib.machinery.BYTECODE_SUFFIXES),
    )

ignore = []

def init(ignorelist):
    global ignore
    ignore = ignorelist

def isenabled():
    return _makefinder in sys.path_hooks and not _deactivated

def disable():
    try:
        while True:
            sys.path_hooks.remove(_makefinder)
    except ValueError:
        pass

def enable():
    sys.path_hooks.insert(0, _makefinder)

@contextlib.contextmanager
def deactivated():
    # This implementation is a bit different from Python 2's. Python 3
    # maintains a per-package finder cache in sys.path_importer_cache (see
    # PEP 302). This means that we can't just call disable + enable.
    # If we do that, in situations like:
    #
    #   demandimport.enable()
    #   ...
    #   from foo.bar import mod1
    #   with demandimport.deactivated():
    #       from foo.bar import mod2
    #
    # mod2 will be imported lazily. (The converse also holds -- whatever finder
    # first gets cached will be used.)
    #
    # Instead, have a global flag the LazyLoader can use.
    global _deactivated
    demandenabled = isenabled()
    if demandenabled:
        _deactivated = True
    try:
        yield
    finally:
        if demandenabled:
            _deactivated = False
