# perftrace.py - Module for tracing performance
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
import time
from contextlib import contextmanager

from . import encoding, error, util


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


class StringValue(object):
    __slots__ = ["value"]

    def __init__(self, value):
        self.value = value


class ByteValue(object):
    __slots__ = ["value"]

    def __init__(self, value):
        self.value = value


def traces():
    return finished_traces


lasttime = 0


def gettime():
    # Make it "run-tests.py -i" friendly
    if util.istest():
        global lasttime
        lasttime += 1
        return lasttime
    return time.time()


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

        span.start = gettime()
        yield
    finally:
        span.end = gettime()
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
    latest.values[name] = StringValue("%s" % value)


def tracebytes(name, value):
    """Records the given name=value as being associated with the latest trace
    point. The value is treated as bytes and will be added to any previous value
    set to the same name."""
    if not spans:
        raise error.ProgrammingError(
            "Attempting to set bytes value without starting a trace"
        )

    latest = spans[-1]
    if name in latest.values:
        latest.values[name].value += value
    else:
        latest.values[name] = ByteValue(value)


def tracefunc(name):
    """Decorator that indicates this entire function should be wrapped in a
    trace."""

    def wrapper(func):
        def executer(*args, **kwargs):
            with trace(name):
                return func(*args, **kwargs)

        return executer

    return wrapper


def asciirender(span):
    return _AsciiRenderer(span).render()


class _AsciiRenderer(object):
    def __init__(self, span):
        self.indentamount = 2
        self.span = span
        self.start = self.span.start

        # Width of the start column, so we can right justify everything
        self.start_width = len("{0:0.1f}".format(self.span.end - self.span.start))

        # Seconds of missing data to consider as a gap
        self.gap_threshold = 1

    def render(self):
        output = []
        self._render(output, self.span, 0)
        duration = self.span.end - self.span.start
        output.append("{0:0.1f}".format(duration))

        return "\n".join(output) + "\n"

    def _render(self, output, span, indent):
        start = span.start - self.start
        duration = span.duration()

        details = _format_duration(duration)
        if span.flags:
            details += "; %s" % ("; ".join(sorted(span.flags)))

        output.append(
            "{start} {indent} {name} ({details})".format(
                start=("{0:0.1f}".format(start)).rjust(self.start_width),
                indent=" " * indent,
                name=span.name,
                details=details,
            )
        )

        for name, value in sorted(span.values.iteritems()):
            if isinstance(value, ByteValue):
                value = value.value
                quantity = util.inttosize(value)
                speed = _format_bytes_per_sec(value, duration)
                value = "%s (%s)" % (quantity, speed)
            else:
                value = value.value

            output.append(
                "{mark} {indent} * {name}: {value}".format(
                    mark=":".rjust(self.start_width),
                    indent=" " * (indent + self.indentamount),
                    name=name,
                    value=value,
                )
            )

        last = span.start
        for child in span.children:
            gap = child.start - last
            self._render_gap(output, last, gap, indent + self.indentamount)
            self._render(output, child, indent + self.indentamount)
            last = child.end
        if len(span.children) > 0:
            gap = span.end - last
            self._render_gap(output, last, gap, indent + self.indentamount)

    def _render_gap(self, output, start, gap, indent):
        if gap > self.gap_threshold:
            output.append(
                "{start} {indent} {name} ({duration})".format(
                    start=("{0:0.1f}".format(start - self.start)).rjust(
                        self.start_width
                    ),
                    indent=" " * indent,
                    name="--missing--",
                    duration=_format_duration(gap),
                )
            )


def _format_duration(seconds):
    if seconds < 60:
        return "{0:0.1f}s".format(seconds)
    if seconds < 3600:
        return "{0}m {1}s".format(int(seconds / 60), seconds % 60)
    return "{0}h {1}m".format(int(seconds / 3600), int((seconds % 3600) / 60))


def _format_bytes_per_sec(value, time):
    persec = value / float(time)
    return "%s/s" % util.inttosize(persec)
