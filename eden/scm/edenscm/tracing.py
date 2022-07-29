# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

"""
Tracing APIs similar to the Rust equivalent defined by the tracing crate.
"""

import os
import sys
from functools import partial

import bindings

# The Rust bindings (pytracing crate)
_tracing = bindings.tracing


LEVEL_TRACE = _tracing.LEVEL_TRACE
LEVEL_DEBUG = _tracing.LEVEL_DEBUG
LEVEL_INFO = _tracing.LEVEL_INFO
LEVEL_WARN = _tracing.LEVEL_WARN
LEVEL_ERROR = _tracing.LEVEL_ERROR

disabletracing = False

# ---- instrument ----

"""Decorate a function for tracing

Example::

    @instrument
    def f(x, y):
        # x and y will be logged if they are str or int

    @instrument(level=LEVEL_WARN, target="bar", skip=["y"])
    def f(x, y):
        # skip: do not log specified args
"""

if os.getenv("EDENSCM_NO_PYTHON_INSTRUMENT"):

    def instrument(func=None, **kwargs):
        return func or instrument

else:

    def instrument(func=None, **kwargs):
        if disabletracing:
            return func or instrument

        return _tracing.instrument(func, **kwargs)


# ---- event ----


def event(message, name=None, target=None, level=LEVEL_INFO, depth=0, **meta):
    """Log an event to the Rust tracing eco-system

    name, target, and meta keys are stored in the callsite metadata, meaning
    that a callsite, once defined, won't be able to change them.

    depth can be used to adjust callsite definition. For a utility function
    that wraps 'event', it might want to set depth to 1 so callsite is not
    that utility function but the one calling it. In Rust, such utility
    functions would need to be implemented in macros.

    Example::

        info(f"{n} files downloaded in {t} seconds", request_id=reqid)

    """
    if disabletracing:
        return

    frame = sys._getframe(1 + depth)
    ident = (id(frame.f_code), frame.f_lineno)
    callsite = _callsites.get(ident)
    if callsite is None:
        # Create the callsite.
        # The field name "message" matches Rust tracing macros behavior.
        fieldnames = ["message"]
        if meta:
            fieldnames += sorted(meta)
        callsite = _insertcallsite(
            ident,
            _tracing.EventCallsite(
                obj=frame,
                name=name,
                target=target,
                level=level,
                fieldnames=fieldnames,
            ),
        )

    frame = None  # break cycles
    values = [message]
    if meta:
        values += [v for _k, v in sorted(meta.items())]

    callsite.event(values)


trace = partial(event, level=LEVEL_TRACE)
debug = partial(event, level=LEVEL_DEBUG)
info = partial(event, level=LEVEL_INFO)
warn = partial(event, level=LEVEL_WARN)
error = partial(event, level=LEVEL_ERROR)

# ---- span ----


class _stubspan(object):
    """Stub for a real span for when Python tracing is disabled."""

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def record(self, _name, _value):
        pass

    def is_disabled(self):
        return True

    def id(self):
        return None


def span(name, target=None, level=LEVEL_INFO, depth=0, **meta):
    """Open a span in the Rust tracing eco-system.

    The returned span works as a context manager, and meta can be dynamically
    updated via 'span.record(name=value)'. Note: all meta field names must
    be predefined statically.

    name, target, and meta keys are stored in the callsite metadata, meaning
    that a callsite, once defined, won't be able to change them.

    depth can be used to adjust callsite definition. For a utility function
    that wraps 'span', it might want to set depth to 1 so callsite is not
    that utility function but the one calling it. In Rust, such utility
    functions would need to be implemented in macros.

    Example::

        with info_span("Downloading Files", n=100, tx=None) as span:
            ...
            span.record("tx", txbytes)
    """

    if disabletracing:
        return _stubspan()

    frame = sys._getframe(1 + depth)
    ident = (id(frame.f_code), frame.f_lineno)
    callsite = _callsites.get(ident)
    if callsite is None:
        fieldnames = meta and sorted(meta)
        callsite = _insertcallsite(
            ident,
            _tracing.SpanCallsite(
                obj=frame,
                name=name,
                target=target,
                level=level,
                fieldnames=fieldnames,
            ),
        )

    frame = None  # break cycles
    values = meta and [v for _k, v in sorted(meta.items())]
    return callsite.span(values)


trace_span = partial(span, level=LEVEL_TRACE)
debug_span = partial(span, level=LEVEL_DEBUG)
info_span = partial(span, level=LEVEL_INFO)
warn_span = partial(span, level=LEVEL_WARN)
error_span = partial(span, level=LEVEL_ERROR)


# ---- test if a callsite is enabled ----


def isenabled(level, name=None, target=None, depth=0):
    """Test if a callsite is enabled."""
    if disabletracing:
        return False

    frame = sys._getframe(1 + depth)
    ident = (id(frame.f_code), frame.f_lineno)
    callsite = _callsites.get(ident)
    if callsite is None:
        # Create the callsite.
        # The field name "message" matches Rust tracing macros behavior.
        fieldnames = []
        callsite = _insertcallsite(
            ident,
            _tracing.EventCallsite(
                obj=frame,
                name=name,
                target=target,
                level=level,
                fieldnames=fieldnames,
            ),
        )
    return callsite.isenabled()


# ---- local cache of callsites ----

_callsites = {}


def _insertcallsite(key, callsite):
    _callsites[key] = callsite
    return callsite
