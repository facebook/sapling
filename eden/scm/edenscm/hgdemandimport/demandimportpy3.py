# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# demandimportpy3 - global demand-loading of modules for Mercurial
#
# Copyright Olivia Mackall <olivia@selenic.com> and others
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


class LazyFinder(object):
    """A wrapper around a ``MetaPathFinder`` that makes loaders lazy.

    ``sys.meta_path`` finders have their ``find_spec()`` called to locate a
    module. This returns a ``ModuleSpec`` if found or ``None``. The
    ``ModuleSpec`` has a ``loader`` attribute, which is called to actually
    load a module.

    Our class wraps an existing finder and overloads its ``find_spec()`` to
    replace the ``loader`` with our lazy loader proxy.

    We have to use __getattribute__ to proxy the instance because some meta
    path finders don't support monkeypatching.
    """

    __slots__ = ("_finder",)

    def __init__(self, finder):
        object.__setattr__(self, "_finder", finder)

    def __repr__(self):
        return "<LazyFinder for %r>" % object.__getattribute__(self, "_finder")

    # __bool__ is canonical Python 3. But check-code insists on __nonzero__ being
    # defined via `def`.
    def __nonzero__(self):
        return bool(object.__getattribute__(self, "_finder"))

    __bool__ = __nonzero__

    def __getattribute__(self, name):
        if name in ("_finder", "find_spec"):
            return object.__getattribute__(self, name)

        return getattr(object.__getattribute__(self, "_finder"), name)

    def __delattr__(self, name):
        return delattr(object.__getattribute__(self, "_finder"))

    def __setattr__(self, name, value):
        return setattr(object.__getattribute__(self, "_finder"), name, value)

    def find_spec(self, *args, **kwargs):
        finder = object.__getattribute__(self, "_finder")
        find_spec = getattr(finder, "find_spec", None)
        if find_spec:
            spec = find_spec(*args, **kwargs)
        else:
            spec = None

        # Lazy loader requires exec_module().
        if (
            spec is not None
            and spec.loader is not None
            and getattr(spec.loader, "exec_module", None) is not None
        ):
            spec.loader = _lazyloaderex(spec.loader)

        return spec


ignore = set()


def init(ignorelist):
    global ignore
    ignore = set(ignorelist)


def isenabled():
    return not _deactivated and any(
        isinstance(finder, LazyFinder) for finder in sys.meta_path
    )


def enable():
    new_finders = []
    for finder in sys.meta_path:
        new_finders.append(
            LazyFinder(finder) if not isinstance(finder, LazyFinder) else finder
        )

    sys.meta_path[:] = new_finders


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
