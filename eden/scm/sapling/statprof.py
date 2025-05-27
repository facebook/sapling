# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#!/usr/bin/env python
## statprof.py
## Copyright (C) 2012 Bryan O'Sullivan <bos@serpentine.com>
## Copyright (C) 2011 Alex Fraser <alex at phatcore dot com>
## Copyright (C) 2004,2005 Andy Wingo <wingo at pobox dot com>
## Copyright (C) 2001 Rob Browning <rlb at defaultvalue dot org>

## This library is free software; you can redistribute it and/or
## modify it under the terms of the GNU Lesser General Public
## License as published by the Free Software Foundation; either
## version 2.1 of the License, or (at your option) any later version.
##
## This library is distributed in the hope that it will be useful,
## but WITHOUT ANY WARRANTY; without even the implied warranty of
## MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
## Lesser General Public License for more details.
##
## You should have received a copy of the GNU Lesser General Public
## License along with this program; if not, contact:
##
## Free Software Foundation           Voice:  +1-617-542-5942
## 59 Temple Place - Suite 330        Fax:    +1-617-542-2652
## Boston, MA  02111-1307,  USA       gnu@gnu.org

"""
statprof is intended to be a fairly simple statistical profiler for
python. It was ported directly from a statistical profiler for guile,
also named statprof, available from guile-lib [0].

[0] http://wingolog.org/software/guile-lib/statprof/

To start profiling, call statprof.start():
>>> start()

Then run whatever it is that you want to profile, for example:
>>> import test.pystone; test.pystone.pystones()

Then stop the profiling and print out the results:
>>> stop()
>>> display()
  %   cumulative      self
 time    seconds   seconds  name
 26.72      1.40      0.37  pystone.py:79:Proc0
 13.79      0.56      0.19  pystone.py:133:Proc1
 13.79      0.19      0.19  pystone.py:208:Proc8
 10.34      0.16      0.14  pystone.py:229:Func2
  6.90      0.10      0.10  pystone.py:45:__init__
  4.31      0.16      0.06  pystone.py:53:copy
    ...

All of the numerical data is statistically approximate. In the
following column descriptions, and in all of statprof, "time" refers
to the wall clock time, not execution time (both user and system).

% time
    The percent of the time spent inside the procedure itself (not
    counting children).

cumulative seconds
    The total number of seconds spent in the procedure, including
    children.

self seconds
    The total number of seconds spent in the procedure itself (not
    counting children).

name
    The name of the procedure.

By default statprof keeps the data collected from previous runs. If you
want to clear the collected data, call reset():
>>> reset()

reset() can also be used to change the sampling frequency from the
default of 1000 Hz. For example, to tell statprof to sample 50 times a
second:
>>> reset(50)

This means that statprof will sample the call stack after every 1/50 of
a second of user + system time spent running on behalf of the python
process. When your process is idle (for example, blocking in a read(),
as is the case at the listener), the clock does not advance. For this
reason statprof is not currently not suitable for profiling io-bound
operations.

The profiler uses the hash of the code object itself to identify the
procedures, so it won't confuse different procedures with the same name.
They will show up as two different rows in the output.

Right now the profiler is quite simplistic.  I cannot provide
call-graphs or other higher level information.  What you see in the
table is pretty much all there is. Patches are welcome :-)

Threading
---------

Because signals only get delivered to the main thread in Python,
statprof only profiles the main thread. However because the time
reporting function uses per-process timers, the results can be
significantly off if other threads' work patterns are not similar to the
main thread's work patterns.
"""

# no-check-code

import collections
import contextlib
import getopt
import inspect
import json
import os
import signal
import sys
import tempfile
import threading
import time

from . import encoding, util

defaultdict = collections.defaultdict
contextmanager = contextlib.contextmanager

__all__ = ["start", "stop", "reset", "display", "profile"]

skips = {
    "util.py:check",
    "extensions.py:closure",
    "color.py:colorcmd",
    "dispatch.py:checkargs",
    "dispatch.py:<lambda>",
    "dispatch.py:_runcatch",
    "dispatch.py:_dispatch",
    "dispatch.py:_runcommand",
    "pager.py:pagecmd",
    "dispatch.py:run",
    "dispatch.py:dispatch",
    "dispatch.py:runcommand",
    "hg.py:<module>",
    "evolve.py:warnobserrors",
}

###########################################################################
## Utils


def clock():
    return time.time()


###########################################################################
## Collection data structures


class ProfileState:
    def __init__(self, frequency=None):
        self.reset(frequency)

    def reset(self, frequency=None):
        # total so far
        self.accumulated_time = 0.0
        # start_time when timer is active
        self.last_start_time = None
        # a float
        if frequency:
            self.sample_interval = 1.0 / frequency
        elif not hasattr(self, "sample_interval"):
            # default to 1000 Hz
            self.sample_interval = 1.0 / 1000.0
        else:
            # leave the frequency as it was
            pass
        self.remaining_prof_time = None
        # for user start/stop nesting
        self.profile_level = 0

        self.samples = []

    def accumulate_time(self, stop_time):
        self.accumulated_time += stop_time - self.last_start_time

    def seconds_per_sample(self):
        return self.accumulated_time / len(self.samples)


state = ProfileState()


class CodeSite:
    cache = {}

    __slots__ = ("path", "lineno", "function", "source")

    def __init__(self, path, lineno, function):
        self.path = path
        self.lineno = lineno
        self.function = function
        self.source = None

    def __eq__(self, other):
        try:
            return self.lineno == other.lineno and self.path == other.path
        except:
            return False

    def __hash__(self):
        return hash((self.lineno, self.path))

    @classmethod
    def get(cls, path, lineno, function):
        k = (path, lineno)
        try:
            return cls.cache[k]
        except KeyError:
            v = cls(path, lineno, function)
            cls.cache[k] = v
            return v

    def getsource(self, length):
        if self.source is None:
            lineno = self.lineno - 1
            fp = None
            try:
                fp = open(self.path)
                for i, line in enumerate(fp):
                    if i == lineno:
                        self.source = line.strip()
                        break
            except:
                pass
            finally:
                if fp:
                    fp.close()
            if self.source is None:
                self.source = ""

        source = self.source
        if len(source) > length:
            source = source[: (length - 3)] + "..."
        return source

    def filename(self):
        return os.path.basename(self.path)


class Sample:
    __slots__ = ("stack", "time")

    def __init__(self, stack, time):
        self.stack = stack
        self.time = time

    @classmethod
    def from_frame(cls, frame, time):
        stack = []

        while frame:
            stack.append(
                CodeSite.get(
                    frame.f_code.co_filename, frame.f_lineno, frame.f_code.co_name
                )
            )
            frame = frame.f_back

        return Sample(stack, time)


###########################################################################
## SIGPROF handler


def profile_signal_handler(signum, frame):
    if state.profile_level > 0:
        now = clock()
        state.accumulate_time(now)

        state.samples.append(Sample.from_frame(frame, state.accumulated_time))

        signal.setitimer(signal.ITIMER_PROF, state.sample_interval, 0.0)
        state.last_start_time = now


stopthread = threading.Event()


def samplerthread(tid):
    while not stopthread.is_set():
        now = clock()
        state.accumulate_time(now)

        frame = sys._current_frames()[tid]
        state.samples.append(Sample.from_frame(frame, state.accumulated_time))

        state.last_start_time = now
        stopthread.wait(state.sample_interval)

    stopthread.clear()


###########################################################################
## Profiling API


def is_active():
    return state.profile_level > 0


lastmechanism = None


def start(mechanism="thread"):
    """Install the profiling signal handler, and start profiling."""
    state.profile_level += 1
    if state.profile_level == 1:
        state.last_start_time = clock()
        rpt = state.remaining_prof_time
        state.remaining_prof_time = None

        global lastmechanism
        lastmechanism = mechanism

        if mechanism == "signal":
            util.signal(signal.SIGPROF, profile_signal_handler)
            signal.setitimer(signal.ITIMER_PROF, rpt or state.sample_interval, 0.0)
        elif mechanism == "thread":
            frame = inspect.currentframe()
            tid = [k for k, f in sys._current_frames().items() if f == frame][0]
            state.thread = threading.Thread(
                target=samplerthread, args=(tid,), name="samplerthread"
            )
            state.thread.start()


def stop():
    """Stop profiling, and uninstall the profiling signal handler."""
    state.profile_level -= 1
    if state.profile_level == 0:
        if lastmechanism == "signal":
            rpt = signal.setitimer(signal.ITIMER_PROF, 0.0, 0.0)
            util.signal(signal.SIGPROF, signal.SIG_IGN)
            state.remaining_prof_time = rpt[0]
        elif lastmechanism == "thread":
            stopthread.set()
            state.thread.join()

        state.accumulate_time(clock())
        state.last_start_time = None
        statprofpath = encoding.environ.get("STATPROF_DEST")
        if statprofpath:
            save_data(statprofpath)

    return state


def save_data(path):
    with open(path, "w+") as file:
        file.write(str(state.accumulated_time) + "\n")
        for sample in state.samples:
            time = str(sample.time)
            stack = sample.stack
            sites = ["\1".join([s.path, str(s.lineno), s.function]) for s in stack]
            file.write(time + "\0" + "\0".join(sites) + "\n")


def load_data(path):
    lines = open(path, "r").read().splitlines()

    state.accumulated_time = float(lines[0])
    state.samples = []
    for line in lines[1:]:
        parts = line.split("\0")
        time = float(parts[0])
        rawsites = parts[1:]
        sites = []
        for rawsite in rawsites:
            siteparts = rawsite.split("\1")
            sites.append(CodeSite.get(siteparts[0], int(siteparts[1]), siteparts[2]))

        state.samples.append(Sample(sites, time))


def reset(frequency=None):
    """Clear out the state of the profiler.  Do not call while the
    profiler is running.

    The optional frequency argument specifies the number of samples to
    collect per second."""
    assert state.profile_level == 0, "Can't reset() while statprof is running"
    CodeSite.cache.clear()
    state.reset(frequency)


@contextmanager
def profile():
    start()
    try:
        yield
    finally:
        stop()
        display()


###########################################################################
## Reporting API


class SiteStats:
    def __init__(self, site):
        self.site = site
        self.selfcount = 0
        self.totalcount = 0

    def addself(self):
        self.selfcount += 1

    def addtotal(self):
        self.totalcount += 1

    def selfpercent(self):
        return self.selfcount / len(state.samples) * 100

    def totalpercent(self):
        return self.totalcount / len(state.samples) * 100

    def selfseconds(self):
        return self.selfcount * state.seconds_per_sample()

    def totalseconds(self):
        return self.totalcount * state.seconds_per_sample()

    @classmethod
    def buildstats(cls, samples):
        stats = {}

        for sample in samples:
            for i, site in enumerate(sample.stack):
                sitestat = stats.get(site)
                if not sitestat:
                    sitestat = SiteStats(site)
                    stats[site] = sitestat

                sitestat.addtotal()

                if i == 0:
                    sitestat.addself()

        return [s for s in stats.values()]


class DisplayFormats:
    ByLine = 0
    ByMethod = 1
    AboutMethod = 2
    Hotpath = 3
    FlameGraph = 4
    Json = 5
    Chrome = 6


def display(fp=None, format=3, data=None, **kwargs):
    """Print statistics, either to stdout or the given file object."""
    data = data or state

    if fp is None:
        import sys

        fp = sys.stdout
    if len(data.samples) == 0:
        print("No samples recorded.", file=fp)
        fp.flush()
        return

    if format == DisplayFormats.ByLine:
        display_by_line(data, fp)
    elif format == DisplayFormats.ByMethod:
        display_by_method(data, fp)
    elif format == DisplayFormats.AboutMethod:
        display_about_method(data, fp, **kwargs)
    elif format == DisplayFormats.Hotpath:
        display_hotpath(data, fp, **kwargs)
    elif format == DisplayFormats.FlameGraph:
        write_to_flame(data, fp, **kwargs)
    elif format == DisplayFormats.Json:
        write_to_json(data, fp)
    elif format == DisplayFormats.Chrome:
        write_to_chrome(data, fp, **kwargs)
    else:
        raise Exception("Invalid display format")

    if format not in (DisplayFormats.Json, DisplayFormats.Chrome):
        print("---", file=fp)
        print("Sample count: %d" % len(data.samples), file=fp)
        print("Total time: %f seconds" % data.accumulated_time, file=fp)


def display_by_line(data, fp):
    """Print the profiler data with each sample line represented
    as one row in a table.  Sorted by self-time per line."""
    stats = SiteStats.buildstats(data.samples)
    stats.sort(reverse=True, key=lambda x: x.selfseconds())

    print("%5.5s %10.10s   %7.7s  %-8.8s" % ("%  ", "cumulative", "self", ""), file=fp)
    print(
        "%5.5s  %9.9s  %8.8s  %-8.8s" % ("time", "seconds", "seconds", "name"), file=fp
    )

    for stat in stats:
        site = stat.site
        sitelabel = "%s:%d:%s" % (site.filename(), site.lineno, site.function)
        print(
            "%6.2f %9.2f %9.2f  %s"
            % (stat.selfpercent(), stat.totalseconds(), stat.selfseconds(), sitelabel),
            file=fp,
        )


def display_by_method(data, fp):
    """Print the profiler data with each sample function represented
    as one row in a table.  Important lines within that function are
    output as nested rows.  Sorted by self-time per line."""
    print("%5.5s %10.10s   %7.7s  %-8.8s" % ("%  ", "cumulative", "self", ""), file=fp)
    print(
        "%5.5s  %9.9s  %8.8s  %-8.8s" % ("time", "seconds", "seconds", "name"), file=fp
    )

    stats = SiteStats.buildstats(data.samples)

    grouped = defaultdict(list)
    for stat in stats:
        grouped[stat.site.filename() + ":" + stat.site.function].append(stat)

    # compute sums for each function
    functiondata = []
    for fname, sitestats in grouped.items():
        total_cum_sec = 0
        total_self_sec = 0
        total_percent = 0
        for stat in sitestats:
            total_cum_sec += stat.totalseconds()
            total_self_sec += stat.selfseconds()
            total_percent += stat.selfpercent()

        functiondata.append(
            (fname, total_cum_sec, total_self_sec, total_percent, sitestats)
        )

    # sort by total self sec
    functiondata.sort(reverse=True, key=lambda x: x[2])

    for function in functiondata:
        if function[3] < 0.05:
            continue
        print(
            "%6.2f %9.2f %9.2f  %s"
            % (
                function[3],  # total percent
                function[1],  # total cum sec
                function[2],  # total self sec
                function[0],
            ),  # file:function
            file=fp,
        )
        function[4].sort(reverse=True, key=lambda i: i.selfseconds())
        for stat in function[4]:
            # only show line numbers for significant locations (>1% time spent)
            if stat.selfpercent() > 1:
                source = stat.site.getsource(25)
                stattuple = (
                    stat.selfpercent(),
                    stat.selfseconds(),
                    stat.site.lineno,
                    source,
                )

                print("%33.0f%% %6.2f   line %s: %s" % stattuple, file=fp)


def display_about_method(data, fp, function=None, **kwargs):
    if function is None:
        raise Exception("Invalid function")

    filename = None
    if ":" in function:
        filename, function = function.split(":")

    relevant_samples = 0
    parents = {}
    children = {}

    for sample in data.samples:
        for i, site in enumerate(sample.stack):
            if site.function == function and (
                not filename or site.filename() == filename
            ):
                relevant_samples += 1
                if i != len(sample.stack) - 1:
                    parent = sample.stack[i + 1]
                    if parent in parents:
                        parents[parent] = parents[parent] + 1
                    else:
                        parents[parent] = 1

                if site in children:
                    children[site] = children[site] + 1
                else:
                    children[site] = 1

    parentlist = [(p, count) for p, count in parents.items()]
    parentlist.sort(reverse=True, key=lambda x: x[1])
    for parent, count in parentlist:
        print(
            "%6.2f%%   %s:%s   line %s: %s"
            % (
                count / relevant_samples * 100,
                parent.filename(),
                parent.function,
                parent.lineno,
                parent.getsource(50),
            ),
            file=fp,
        )

    stats = SiteStats.buildstats(data.samples)
    stats = [
        s
        for s in stats
        if s.site.function == function
        and (not filename or s.site.filename() == filename)
    ]

    total_cum_sec = 0
    total_self_sec = 0
    total_self_percent = 0
    total_cum_percent = 0
    for stat in stats:
        total_cum_sec += stat.totalseconds()
        total_self_sec += stat.selfseconds()
        total_self_percent += stat.selfpercent()
        total_cum_percent += stat.totalpercent()

    print(
        "\n    %s:%s    Total: %0.2fs (%0.2f%%)    Self: %0.2fs (%0.2f%%)\n"
        % (
            filename or "___",
            function,
            total_cum_sec,
            total_cum_percent,
            total_self_sec,
            total_self_percent,
        ),
        file=fp,
    )

    children = [(child, count) for child, count in children.items()]
    children.sort(reverse=True, key=lambda x: x[1])
    for child, count in children:
        print(
            "        %6.2f%%   line %s: %s"
            % (count / relevant_samples * 100, child.lineno, child.getsource(50)),
            file=fp,
        )


def display_hotpath(data, fp, limit=0.05, **kwargs):
    class HotNode:
        def __init__(self, site):
            self.site = site
            self.count = 0
            self.children = {}

        def add(self, stack, time):
            self.count += time
            site = stack[0]
            child = self.children.get(site)
            if not child:
                child = HotNode(site)
                self.children[site] = child

            if len(stack) > 1:
                i = 1
                # Skip boiler plate parts of the stack
                while (
                    i < len(stack)
                    and "%s:%s" % (stack[i].filename(), stack[i].function) in skips
                ):
                    i += 1
                if i < len(stack):
                    child.add(stack[i:], time)

    root = HotNode(None)
    lasttime = data.samples[0].time
    for sample in data.samples:
        root.add(sample.stack[::-1], sample.time - lasttime)
        lasttime = sample.time

    if kwargs.get("color", True):
        redformat = "\033[91m%s\033[0m"
        greyformat = "\033[90m%s\033[0m"
        whiteformat = "%s"
    else:
        redformat = "* %s"
        greyformat = "  %s"
        whiteformat = "  %s"

    def _write(node, depth, multiple_siblings):
        site = node.site
        visiblechildren = [
            c for c in node.children.values() if c.count >= (limit * root.count)
        ]
        if site:
            indent = depth * 2 - 1
            filename = ""
            function = ""
            if len(node.children) > 0:
                childsite = list(node.children.values())[0].site
                filename = (childsite.filename() + ":").ljust(15)
                function = childsite.function

            # lots of string formatting
            listpattern = (
                "".ljust(indent)
                + ("\\" if multiple_siblings else "|")
                + " %4.1f%%  %s %s"
            )
            liststring = listpattern % (
                node.count / root.count * 100,
                filename,
                function,
            )
            codepattern = "%" + str(55 - len(liststring)) + "s %s:  %s"
            codestring = codepattern % ("line", site.lineno, site.getsource(30))

            finalstring = liststring + codestring
            childrensamples = sum([c.count for c in node.children.values()])
            # Make frames that performed more than 10% of the operation red
            if node.count - childrensamples > (0.1 * root.count):
                finalstring = redformat % finalstring
            # Make frames that didn't actually perform work dark grey
            elif node.count - childrensamples == 0:
                finalstring = greyformat % finalstring
            else:
                finalstring = whiteformat % finalstring
            print(finalstring, file=fp)

        newdepth = depth
        if len(visiblechildren) > 1 or multiple_siblings:
            newdepth += 1

        visiblechildren.sort(reverse=True, key=lambda x: x.count)
        for child in visiblechildren:
            _write(child, newdepth, len(visiblechildren) > 1)

    if root.count > 0:
        _write(root, 0, False)


def write_to_flame(data, fp, scriptpath=None, outputfile=None, **kwargs):
    if scriptpath is None:
        scriptpath = encoding.environ["HOME"] + "/flamegraph.pl"
    if not os.path.exists(scriptpath):
        print("error: missing %s" % scriptpath, file=fp)
        print("get it here: https://github.com/brendangregg/FlameGraph", file=fp)
        return

    fd, path = tempfile.mkstemp()

    file = open(path, "w+")

    lines = {}
    for sample in data.samples:
        sites = [s.function for s in sample.stack]
        sites.reverse()
        line = ";".join(sites)
        if line in lines:
            lines[line] = lines[line] + 1
        else:
            lines[line] = 1

    for line, count in lines.items():
        file.write("%s %s\n" % (line, count))

    file.close()

    if outputfile is None:
        outputfile = "~/flamegraph.svg"

    os.system("perl ~/flamegraph.pl %s > %s" % (path, outputfile))
    print("Written to %s" % outputfile, file=fp)


_pathcache = {}


def simplifypath(path):
    """Attempt to make the path to a Python module easier to read by
    removing whatever part of the Python search path it was found
    on."""

    if path in _pathcache:
        return _pathcache[path]
    hgpath = encoding.__file__.rsplit(os.sep, 2)[0]
    for p in [hgpath] + sys.path:
        prefix = p + os.sep
        if path.startswith(prefix):
            path = path[len(prefix) :]
            break
    _pathcache[path] = path
    return path


def write_to_json(data, fp):
    samples = []

    for sample in data.samples:
        stack = []

        for frame in sample.stack:
            stack.append((frame.path, frame.lineno, frame.function))

        samples.append((sample.time, stack))

    print(json.dumps(samples), file=fp)


def write_to_chrome(data, fp, minthreshold=0.005, maxthreshold=0.999):
    samples = []
    laststack = collections.deque()
    lastseen = collections.deque()

    # The Chrome tracing format allows us to use a compact stack
    # representation to save space. It's fiddly but worth it.
    # We maintain a bijection between stack and ID.
    stack2id = {}
    id2stack = []  # will eventually be rendered

    def stackid(stack):
        if not stack:
            return
        if stack in stack2id:
            return stack2id[stack]
        parent = stackid(stack[1:])
        myid = len(stack2id)
        stack2id[stack] = myid
        id2stack.append(dict(category=stack[0][0], name="%s %s" % stack[0]))
        if parent is not None:
            id2stack[-1].update(parent=parent)
        return myid

    def endswith(a, b):
        return list(a)[-len(b) :] == list(b)

    # The sampling profiler can sample multiple times without
    # advancing the clock, potentially causing the Chrome trace viewer
    # to render single-pixel columns that we cannot zoom in on.  We
    # work around this by pretending that zero-duration samples are a
    # millisecond in length.

    clamp = 0.001

    # We provide knobs that by default attempt to filter out stack
    # frames that are too noisy:
    #
    # * A few take almost all execution time. These are usually boring
    #   setup functions, giving a stack that is deep but uninformative.
    #
    # * Numerous samples take almost no time, but introduce lots of
    #   noisy, oft-deep "spines" into a rendered profile.

    exclude_list = set()
    totaltime = data.samples[-1].time - data.samples[0].time
    minthreshold = totaltime * minthreshold
    maxthreshold = max(totaltime * maxthreshold, clamp)

    def poplast():
        oldsid = stackid(tuple(laststack))
        oldcat, oldfunc = laststack.popleft()
        oldtime, oldidx = lastseen.popleft()
        duration = sample.time - oldtime
        if minthreshold <= duration <= maxthreshold:
            # ensure no zero-duration events
            sampletime = max(oldtime + clamp, sample.time)
            samples.append(
                dict(
                    ph="E",
                    name=oldfunc,
                    cat=oldcat,
                    sf=oldsid,
                    ts=sampletime * 1e6,
                    pid=0,
                )
            )
        else:
            exclude_list.add(oldidx)

    # Much fiddling to synthesize correctly(ish) nested begin/end
    # events given only stack snapshots.

    for sample in data.samples:
        tos = sample.stack[0]
        name = tos.function
        path = simplifypath(tos.path)
        stack = tuple(
            (
                ("%s:%d" % (simplifypath(frame.path), frame.lineno), frame.function)
                for frame in sample.stack
            )
        )
        qstack = collections.deque(stack)
        if laststack == qstack:
            continue
        while laststack and qstack and laststack[-1] == qstack[-1]:
            laststack.pop()
            qstack.pop()
        while laststack:
            poplast()
        for f in reversed(qstack):
            lastseen.appendleft((sample.time, len(samples)))
            laststack.appendleft(f)
            path, name = f
            sid = stackid(tuple(laststack))
            samples.append(
                dict(ph="B", name=name, cat=path, ts=sample.time * 1e6, sf=sid, pid=0)
            )
        laststack = collections.deque(stack)
    while laststack:
        poplast()
    events = [s[1] for s in enumerate(samples) if s[0] not in exclude_list]
    frames = collections.OrderedDict((str(k), v) for (k, v) in enumerate(id2stack))
    json.dump(dict(traceEvents=events, stackFrames=frames), fp, indent=1)
    fp.write("\n")


def printusage():
    print(
        """
The statprof command line allows you to inspect the last profile's results in
the following forms:

usage:
    hotpath [-l --limit percent]
        Shows a graph of calls with the percent of time each takes.
        Red calls take over 10%% of the total time themselves.
    lines
        Shows the actual sampled lines.
    functions
        Shows the samples grouped by function.
    function [filename:]functionname
        Shows the callers and callees of a particular function.
    flame [-s --script-path] [-o --output-file path]
        Writes out a flamegraph to output-file (defaults to ~/flamegraph.svg)
        Requires that ~/flamegraph.pl exist.
        (Specify alternate script path with --script-path.)"""
    )


def main(argv=None):
    if argv is None:
        argv = sys.argv

    if len(argv) == 1:
        printusage()
        return 0

    displayargs = {}

    optstart = 2
    displayargs["function"] = None
    if argv[1] == "hotpath":
        displayargs["format"] = DisplayFormats.Hotpath
    elif argv[1] == "lines":
        displayargs["format"] = DisplayFormats.ByLine
    elif argv[1] == "functions":
        displayargs["format"] = DisplayFormats.ByMethod
    elif argv[1] == "function":
        displayargs["format"] = DisplayFormats.AboutMethod
        displayargs["function"] = argv[2]
        optstart = 3
    elif argv[1] == "flame":
        displayargs["format"] = DisplayFormats.FlameGraph
    else:
        printusage()
        return 0

    # process options
    try:
        opts, args = getopt.getopt(
            sys.argv[optstart:],
            "hl:f:o:p:",
            ["help", "limit=", "file=", "output-file=", "script-path="],
        )
    except getopt.error as msg:
        print(msg)
        printusage()
        return 2

    displayargs["limit"] = 0.05
    path = None
    for o, value in opts:
        if o in ("-l", "--limit"):
            displayargs["limit"] = float(value)
        elif o in ("-f", "--file"):
            path = value
        elif o in ("-o", "--output-file"):
            displayargs["outputfile"] = value
        elif o in ("-p", "--script-path"):
            displayargs["scriptpath"] = value
        elif o in ("-h", "help"):
            printusage()
            return 0
        else:
            assert False, "unhandled option %s" % o

    if not path:
        print("must specify --file to load")
        return 1

    load_data(path=path)

    display(**displayargs)

    return 0


if __name__ == "__main__":
    sys.exit(main())
