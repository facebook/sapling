# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

# Attention: Modules imported are not traceable. Keep the list minimal.
import sys
import types

# pyre-fixme[21]: Could not find `bindings`.
import bindings


class ModuleLoader(object):
    # load_module: (fullname) -> module
    # See find_module below for why it's implemented in this way.
    load_module = sys.modules.__getitem__


class TraceImporter(object):
    """Trace time spent on importing modules.

    In additional, wrap functions so they get traced.
    """

    # Blacklisted modules - Tracing them might yield huge amount of
    # uninteresting data.
    _blocklist = {
        # unicodedata.east_asian_width can be called very frequently.
        "unicodedata",
        # parsers.isasciistr can be called very frequently.
        "edenscmnative.parsers",
        # encoding.tolocal can be called very frequently.
        "edenscm.mercurial.encoding",
    }

    def __init__(self, shouldtrace=lambda _name: True):
        """
        shouldtrace: (name) -> bool.

        If shouldtrace(modulename) is True, trace functions in module.
        If shouldtrace("import") is True, trace import statements.
        """
        # Function parameters are used below for performance.
        # They changed LOAD_GLOBAL to LOAD_FAST.

        _modules = sys.modules
        _loader = ModuleLoader()
        _attempted = set()

        def _import(
            name,
            _shouldwrap=shouldtrace,
            _rawimport=__import__,
            _get=_modules.__getitem__,
            _wrap=tracemodule,
            _blocklist=self._blocklist,
        ):
            _rawimport(name)

            if _shouldwrap(name) and name not in _blocklist:
                mod = _get(name)
                _wrap(mod)
                try:
                    pass
                except Exception as ex:
                    # Failing to wrap the module is not fatal. More importantly,
                    # Do not translate this into an ImportError, which might
                    # trigger surprising behaviors, including importing (aka.
                    # executing code) on an already imported module again.
                    # While most modules are fine, some modules are definitely
                    # not ready for it. For example:
                    #
                    #    # module foo.py
                    #    import time
                    #    origtime = time.time
                    #    def newtime():
                    #        return origtime() + 1
                    #    time.time = newtime
                    #
                    # When importing again, `newtime()` will stack overflow,
                    # since `origtime = time.time` gets executed, and `origtime`
                    # used in `newtime` is `newtime` itself.
                    #
                    # The same effect can be achieved using `reload` from
                    # stdlib: `reload(foo)`. But most modules are not tested
                    # about `reload` friendliness.

                    # But, still surface the error, since normally traceimport
                    # should be able to wrap modules just fine.
                    sys.stderr.write(
                        "traceimport: fail to instrument module %s: %r\n" % (name, ex)
                    )
                    sys.stderr.flush()

        if shouldtrace("import"):
            _import = bindings.tracing.wrapfunc(
                _import,
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


_functypes = (types.FunctionType, types.BuiltinFunctionType)
_isheaptype = bindings.tracing.isheaptype
_tracedclasses = {object, type, types.ModuleType, dict}
_wrapfunc = bindings.tracing.wrapfunc


def traceclass(cls,):
    """Annotate functions in a class so they get traced."""
    bases = getattr(cls, "__mro__", [])
    for obj in bases:
        # It's possible to have recursive classes (ex. ctypes). So avoid
        # wrapping a same class again.
        if obj in _tracedclasses:
            continue
        _tracedclasses.add(obj)
        # Don't bother with non-heap types. `setattr` does not work on them.
        if not isinstance(obj, type) or not _isheaptype(obj):
            continue
        container = obj.__dict__
        name = obj.__name__
        for k, v in container.items():
            if isinstance(v, type):
                traceclass(v)
            elif isinstance(v, _functypes):
                # `container` is likely a read-only `dict_proxy`.
                # So `container[k] = v` does not work. Use `setattr` instead.
                # See https://stackoverflow.com/questions/25440694.
                setattr(obj, k, _wrapfunc(v, classname=name))


def tracemodule(mod,):
    """Annotate functions and classes in a module so they get traced."""
    modname = mod.__name__
    container = mod.__dict__

    for k, v in container.items():
        if getattr(v, "__module__", None) != modname:
            continue
        if isinstance(v, type):
            traceclass(v)
        elif isinstance(v, _functypes):
            container[k] = _wrapfunc(v)


def enable(config=None):
    """Enable traceimport.

    'config' is space separated names.

    Space separated names. A name can be one of the following forms:
    - "import": Trace import.
    - "foo.bar": Attempt to trace functions in module "foo.bar" without
      its submodules.
    - "foo.bar.*": Attempt to trace functions in module"foo.bar" and its
      submodules.
    - "*": Attempt to trace everything.

    If config is not specified, it's read from `os.getenv("EDENSCM_TRACE_PY")`.

    If 'printatexit' is True, print a ASCII graph at the end of program
    (for quick-adhoc performance analysis).
    """
    if config is None:
        import os

        config = os.getenv("EDENSCM_TRACE_PY")
    if config in {None, ""}:
        return

    names = config.split()
    prefixes = [n[:-1] for n in names if n.endswith("*")]
    exactnames = {n for n in names if not n.endswith("*")}

    if "" in prefixes:

        def shouldtrace(name):
            return True

    else:

        def shouldtrace(name, _exact=exactnames, _prefix=prefixes, _any=any):
            if name in _exact:
                return True

            startswith = name.startswith
            return _any(startswith(p) for p in _prefix)

    sys.meta_path.insert(0, TraceImporter(shouldtrace))


def registeratexit(threshold=20000):
    """Register an atexit handler that prints ASCII tracing output.

    This is for quick ad-hoc performance analysis.
    """
    import atexit

    def printtrace():
        tracer = bindings.tracing.singleton
        sys.stderr.write(tracer.ascii(threshold))
        sys.stderr.flush()

    atexit.register(printtrace)
