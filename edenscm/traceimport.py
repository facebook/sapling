# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

# Attention: Modules imported are not traceable. Keep the list minimal.
import sys

import bindings


class ModuleLoader(object):
    # load_module: (fullname) -> module
    # See find_module below for why it's implemented in this way.
    load_module = sys.modules.__getitem__


class TraceImporter(object):
    """Trace time spent on importing modules.

    In additional, wrap functions so they get traced.
    """

    def __init__(self):
        # Function parameters are used below for performance.
        # They changed LOAD_GLOBAL to LOAD_FAST.

        _modules = sys.modules
        _loader = ModuleLoader()
        _attempted = set()
        _import = bindings.tracing.wrapfunc(
            __import__,
            meta=lambda name: [("name", "import %s" % name), ("cat", "import")],
        )

        # importer.find_module(fullname, path=None) is defined by PEP 302.
        # Note: Python 3.4 introduced find_spec, and deprecated this API.
        def find_module(
            fullname,
            path=None,
            _import=_import,
            _attempted=_attempted,
            _loader=_loader,
            _modules=_modules,
        ):
            # Example arguments:
            # - fullname = "contextlib", path = None
            # - fullname = "io", path = None
            # - fullname = "edenscm.mercurial.blackbox", path = ["/data/edenscm"]
            # - fullname = "email.errors", path = ["/lib/python/email"]

            # PEP 302 says "find_module" returns either None or a "loader" that has
            # "load_module(fullname)" to actually load the module.
            #
            # Abuse the interface by actually importing the module now.
            if fullname not in _attempted:
                assert fullname not in _modules
                _attempted.add(fullname)
                _import(fullname)
                # Since we just imported the module (to sys.modules).
                # The loader can read it from sys.modules directly.
                return _loader

            # Try the next importer.
            return None

        self.find_module = find_module


def enable():
    sys.meta_path.insert(0, TraceImporter())
