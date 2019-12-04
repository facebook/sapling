# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# perftrace.py - Module for tracing performance

import inspect
from contextlib import contextmanager

# pyre-fixme[21]: Could not find `bindings`.
from bindings import tracing

from . import util


# Native tracing utilities

tracer = tracing.singleton


def editspan(
    meta,
    _edit=tracer.edit,
    _currentframe=inspect.currentframe,
    _wrapfunc=tracing.wrapfunc,
    _spanid=tracing.wrapfunc.spanid,
):
    """Edit the current native span. meta: [(key, value)]"""
    # Find the "spanid" from the callsite stack
    spanid = None
    frame = _currentframe().f_back
    while frame is not None:
        funcname = frame.f_code.co_name
        func = frame.f_globals.get(funcname)
        if getattr(func, "isperftrace", False) and isinstance(func, _wrapfunc):
            # Got the function! Use wrapfunc.spanid to read the spanid.
            frame = None
            spanid = _spanid(func)
            break
        # Try the parent frame
        frame = frame.f_back
    if spanid is not None:
        _edit(spanid, meta)


# PerfTrace wrappers


@contextmanager
def trace(name):
    spanid = tracer.span([("name", name), ("cat", "perftrace")])
    tracer.enter(spanid)
    try:
        yield
    finally:
        tracer.exit(spanid)


def traceflag(flagname):
    """Records the given flag name as being associated with the latest trace
    point."""
    # XXX: No multi-flag support for now.
    editspan([("flag", flagname)])


def tracevalue(name, value):
    """Records the given name=value as being associated with the latest trace
    point. This will overwrite any previous value with that name."""
    editspan([(name, str(value))])


def tracebytes(name, value):
    """Records the given name=value as being associated with the latest trace
    point. The value is treated as bytes and will be added to any previous value
    set to the same name."""
    # XXX: Rust tracing does not do an addition - But there do not seem to be
    # any users relying on the behavior.
    editspan([(name, str(value))])


def tracefunc(name):
    """Decorator that indicates this entire function should be wrapped in a
    trace."""

    def wrapper(func):
        func.meta = [("name", name), ("cat", "perftrace")]
        if util.istest():
            func.meta.append(("line", "_"))
        func.isperftrace = True
        return tracing.wrapfunc(func)

    return wrapper
