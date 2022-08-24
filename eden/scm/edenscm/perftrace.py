# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# perftrace.py - Module for tracing performance

import inspect
import threading
from contextlib import contextmanager

from bindings import tracing

from . import util


# Native tracing utilities

tracer = util.tracer

threadlocal = threading.local()


def editspan(
    meta,
    _edit=tracer.edit,
    _currentframe=inspect.currentframe,
    _wrapfunc=tracing.wrapfunc,
    _spanid=tracing.wrapfunc.spanid,
):
    """Edit the current native span. meta: [(key, value)]"""
    stack = threadlocal.__dict__.get("stack", [])
    if not stack:
        return
    spanid = stack[-1]
    _edit(spanid, meta)


# PerfTrace wrappers


@contextmanager
def trace(name):
    spanid = tracer.span([("name", name), ("cat", "perftrace")])
    threadlocal.__dict__.setdefault("stack", []).append(spanid)
    tracer.enter(spanid)
    try:
        yield
    finally:
        tracer.exit(spanid)
        threadlocal.stack.pop()


def traceflag(flagname):
    """Records the given flag name as being associated with the latest trace
    point."""
    # XXX: No multi-flag support for now.
    editspan([("%s" % flagname, "true")])


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

        def pushspan(spanid):
            threadlocal.__dict__.setdefault("stack", []).append(spanid)

        def popspan():
            threadlocal.__dict__.setdefault("stack", []).pop()

        return tracing.wrapfunc(func, push_callback=pushspan, pop_callback=popspan)

    return wrapper
