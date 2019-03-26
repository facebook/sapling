# perftrace.py - Module for tracing performance
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
import time

from contextlib import contextmanager

from . import error

spans = []

finished_traces = []


class Span(object):
    __slots__ = ["name", "start", "end", "children", "flags", "values"]

    def __init__(self, name):
        self.name = name
        self.start = -1
        self.end = -1
        self.children = []
        self.flags = set()
        self.values = {}

    def duration(self):
        return self.end - self.start


def traces():
    return finished_traces


@contextmanager
def trace(name):
    try:
        latest = None
        if spans:
            latest = spans[-1]

        span = Span(name)
        spans.append(span)
        if latest:
            latest.children.append(span)

        span.start = time.time()
        yield
    finally:
        span.end = time.time()
        spans.pop(-1)
        if not spans:
            finished_traces.append(span)


def traceflag(flagname):
    """Records the given flag name as being associated with the latest trace
    point."""
    if not spans:
        raise error.ProgrammingError("Attempting to set flag without starting a trace")

    latest = spans[-1]
    latest.flags.add(flagname)


def tracevalue(name, value):
    """Records the given name=value as being associated with the latest trace
    point. This will overwrite any previous value with that name."""
    if not spans:
        raise error.ProgrammingError("Attempting to set value without starting a trace")

    latest = spans[-1]

    # TODO: report when overwriting a value
    latest.values[name] = value


def tracebytes(name, value):
    """Records the given name=value as being associated with the latest trace
    point. The value is treated as bytes and will be added to any previous value
    set to the same name."""
    if not spans:
        raise error.ProgrammingError(
            "Attempting to set bytes value without starting a trace"
        )

    latest = spans[-1]
    latest.values[name] = latest.values.get(name, 0) + value


def tracefunc(name):
    """Decorator that indicates this entire function should be wrapped in a
    trace."""

    def wrapper(func):
        def executer(*args, **kwargs):
            with trace(name):
                return func(*args, **kwargs)

        return executer

    return wrapper
