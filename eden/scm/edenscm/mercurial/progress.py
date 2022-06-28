# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# progress.py progress bars related code
#
# Copyright (C) 2010 Augie Fackler <durin42@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import os
import threading
import time
import traceback

import bindings
from bindings import threading as rustthreading

from . import pycompat, util
from .i18n import _, _x


_tracer = util.tracer


def fmtremaining(seconds):
    """format a number of remaining seconds in human readable way

    This will properly display seconds, minutes, hours, days if needed"""
    if seconds is None:
        return ""

    if seconds < 60:
        # i18n: format XX seconds as "XXs"
        return _("%02ds") % seconds
    minutes = seconds // 60
    if minutes < 60:
        seconds -= minutes * 60
        # i18n: format X minutes and YY seconds as "XmYYs"
        return _("%dm%02ds") % (minutes, seconds)
    # we're going to ignore seconds in this case
    minutes += 1
    hours = minutes // 60
    minutes -= hours * 60
    if hours < 30:
        # i18n: format X hours and YY minutes as "XhYYm"
        return _("%dh%02dm") % (hours, minutes)
    # we're going to ignore minutes in this case
    hours += 1
    days = hours // 24
    hours -= days * 24
    if days < 15:
        # i18n: format X days and YY hours as "XdYYh"
        return _("%dd%02dh") % (days, hours)
    # we're going to ignore hours in this case
    days += 1
    weeks = days // 7
    days -= weeks * 7
    if weeks < 55:
        # i18n: format X weeks and YY days as "XwYYd"
        return _("%dw%02dd") % (weeks, days)
    # we're going to ignore days and treat a year as 52 weeks
    weeks += 1
    years = weeks // 52
    weeks -= years * 52
    # i18n: format X years and YY weeks as "XyYYw"
    return _("%dy%02dw") % (years, weeks)


def estimateremaining(bar):
    if not bar._total:
        return None
    bounds = bar._getestimatebounds()
    if bounds is None:
        return None
    startpos, starttime = bounds[0]
    endpos, endtime = bounds[1]
    if startpos is None or endpos is None:
        return None
    if startpos == endpos:
        return None
    target = bar._total - startpos
    delta = endpos - startpos
    if target >= delta and delta > 0.1:
        elapsed = endtime - starttime
        seconds = (elapsed * (target - delta)) // delta + 1
        return seconds
    return None


def fmtspeed(speed, bar):
    if speed is None:
        return ""
    elif bar._formatfunc:
        return _("%s/sec") % bar._formatfunc(speed)
    elif bar._unit:
        return _("%d %s/sec") % (speed, bar._unit)
    else:
        return _("%d per sec") % speed


def estimatespeed(bar):
    bounds = bar._getestimatebounds()
    if bounds is None:
        return None
    startpos, starttime = bounds[0]
    endpos, endtime = bounds[1]
    if startpos is None or endpos is None:
        return None
    delta = endpos - startpos
    elapsed = endtime - starttime
    if elapsed > 0:
        return delta // elapsed
    return None


# NB: the engine's only purpose is to maintain the progressfile extension's hook point.
class engine(object):
    def __init__(self):
        self._cond = rustthreading.Condition()
        self._active = False
        self._refresh = None
        self._delay = None
        self._bars = []
        self._currentbarindex = None

    @contextlib.contextmanager
    def lock(self):
        # Ugly hack for buggy Python (https://bugs.python.org/issue29988)
        #
        # Python can skip executing "__exit__" if a signal arrives at the
        # "right" time. Workaround it by using N "__exit__"s. Skipping all of
        # them would require N signals to be all sent at "right" time. Unlikely
        # in practise.
        b = rustthreading.bug29988wrapper(self._cond)
        with b, b, b, b, b, b:
            yield

    @contextlib.contextmanager
    def _lockclear(self):
        with self.lock(), _suspendrustprogressbar():
            yield

    def resetstate(self):
        with self.lock():
            self._bars = []
            self._currentbarindex = None
            self._refresh = None
            self._cond.notify_all()

    def register(self, bar):
        with self.lock():
            now = time.time()
            bar._enginestarttime = now
            self._bars.append(bar)
            self._recalculatedisplay(now)
            global suspend
            suspend = self._lockclear
            # Do not redraw when registering a nested bar
            if len(self._bars) <= 1:
                self._cond.notify_all()

    def unregister(self, bar):
        with self.lock():
            try:
                index = self._bars.index(bar)
            except ValueError:
                pass
            else:
                if index == self._currentbarindex:
                    if index == 0:
                        self._complete()
                del self._bars[index:]
                self._recalculatedisplay(time.time())
                if not self._bars:
                    global suspend
                    suspend = _suspendrustprogressbar
                # Do not redraw when unregistering a nested bar
                if len(self._bars) < 1:
                    self._cond.notify_all()

    def _activate(self, ui):
        with self.lock():
            if not self._active:
                self._active = True
                self._thread = threading.Thread(target=self._run, name="progress")
                self._thread.daemon = True
                self._thread.start()
                ui.atexit(self._deactivate)

    def _deactivate(self):
        if self._active:
            with self.lock():
                self._active = False
                self._cond.notify_all()
            self._thread.join()

    def _run(self):
        with self.lock():
            while self._active:
                self._cond.wait(self._refresh)
                if self._active:
                    now = time.time()
                    self._recalculatedisplay(now)
                    self._updateestimation(now)
                    self._show(now)

    def _show(self, now):
        pass

    def _complete(self):
        pass

    def _currentbar(self):
        if self._currentbarindex is not None:
            return self._bars[self._currentbarindex]
        return None

    def _recalculatedisplay(self, now):
        """determine which bar should be displayed, if any"""
        with self.lock():
            if not self._bars:
                self._currentbarindex = None
                self._refresh = None
                return

            # Look to see if there is a new bar to show, or how long until
            # another bar should be shown.
            if self._currentbarindex is None:
                nextbarindex = 0
                newbarindex = None
            else:
                newbarindex = min(self._currentbarindex, len(self._bars) - 1)
                nextbarindex = self._currentbarindex + 1
            changetimes = []
            for b in reversed(range(nextbarindex, len(self._bars))):
                bar = self._bars[b]
                if self._currentbarindex is None:
                    startdelay = bar._delay
                else:
                    startdelay = bar._changedelay
                if bar._enginestarttime + startdelay < now:
                    newbarindex = b
                else:
                    changetimes.append(bar._enginestarttime + startdelay - now)
            self._currentbarindex = newbarindex

            # Update the refresh time.
            bar = self._currentbar()
            if bar is not None:
                changetimes.append(bar._refresh)
            if changetimes:
                self._refresh = min(changetimes)
            else:
                self._refresh = None

    def _updateestimation(self, now):
        with self.lock():
            for bar in self._bars:
                bar._updateestimation(now)


_engine_pid = None
_engine = None


def getengine():
    global _engine
    global _engine_pid
    pid = os.getpid()
    if pid != _engine_pid:
        _engine = engine()
        _engine_pid = pid
    return _engine


@contextlib.contextmanager
def _suspendrustprogressbar():
    util.mainio.disable_progress(True)
    try:
        yield
    finally:
        util.mainio.disable_progress(False)


suspend = _suspendrustprogressbar


def _progvalue(value):
    """split a progress bar value into a position and item"""
    if isinstance(value, tuple):
        return value
    else:
        return value, ""


class basebar(object):
    """bar base class that traces events and updates rust model"""

    def __init__(self):
        self._rust_model = None

    def __enter__(self):
        spanid = _tracer.span(
            [("name", "Progress Bar: %s" % self._topic), ("cat", "progressbar")]
        )
        _tracer.enter(spanid)
        self._spanid = spanid

        # Tell rust about progress bars so it has access to progress
        # metadata, even if rust isn't rendering progress information.
        self._rust_model = bindings.progress.model.ProgressBar(
            topic=self._topic,
            total=self._total,
            unit=self._unit,
        )

        return self.enter()

    def __exit__(self, exctype, excvalue, traceback):
        spanid = self._spanid
        total = getattr(self, "_total", None)
        if total is not None:
            _tracer.edit(spanid, [("total", str(total))])
        _tracer.exit(spanid)

        self._rust_model = None

        return self.exit(exctype, excvalue, traceback)

    def __setattr__(self, name, value):
        super(basebar, self).__setattr__(name, value)

        if not self._rust_model:
            return

        # Proxy value and total updates to rust.
        if name == "value":
            pos, message = _progvalue(value)
            if self._detail:
                if message:
                    message = "%s %s" % (message, self._detail)
                else:
                    message = self._detail

            if message:
                self._rust_model.set_message(str(message))
            self._rust_model.set_position(pos or 0)
        elif name == "_total":
            self._rust_model.set_total(value or 0)


class normalbar(basebar):
    """context manager that adds a progress bar to slow operations

    To use this, wrap a section of code that takes a long time like this:

    with progress.bar(ui, "topic") as prog:
        # processing code
        prog.value = pos
        # alternatively: prog.value = (pos, item)
    """

    def __init__(
        self, ui, topic, unit="", total=None, start=0, formatfunc=None, detail=None
    ):
        super(normalbar, self).__init__()

        self._ui = ui
        self._topic = topic
        self._unit = unit
        self._total = total
        self._start = start
        self._formatfunc = formatfunc
        self._delay = ui.configwith(float, "progress", "delay")
        self._refresh = ui.configwith(float, "progress", "refresh")
        self._changedelay = ui.configwith(float, "progress", "changedelay")
        self._estimateinterval = ui.configwith(float, "progress", "estimateinterval")
        self._estimatecount = max(20, int(self._estimateinterval))
        self._estimatetick = self._estimateinterval / self._estimatecount
        self._estimatering = util.ring(self._estimatecount)
        self._detail = detail

    def reset(self, topic, unit="", total=None):
        with getengine().lock():
            self._topic = topic
            self._unit = unit
            self._total = total
            self.value = self._start
            self._estimatering = util.ring(self._estimatecount)

    def _getestimatebounds(self):
        if len(self._estimatering) < 2:
            return None
        else:
            return self._estimatering[0], self._estimatering[-1]

    def _updateestimation(self, now):
        ring = self._estimatering
        if len(ring) == 0 or ring[-1][1] + self._estimatetick <= now:
            pos, _item = _progvalue(self.value)
            ring.push((pos, now))

    def enter(self):
        self.value = self._start
        getengine().register(self)
        return self

    def exit(self, type, value, traceback):
        getengine().unregister(self)


class debugbar(basebar):
    def __init__(
        self, ui, topic, unit="", total=None, start=0, formatfunc=None, detail=None
    ):
        super(debugbar, self).__init__()

        self._ui = ui
        self._topic = topic
        self._unit = unit
        self._total = total
        self._start = start
        self._formatfunc = formatfunc
        self._started = False
        self._detail = detail

    def reset(self, topic, unit="", total=None):
        if self._started:
            self._ui.write(_x("progress: %s (reset)\n") % self._topic)
        self._topic = topic
        self._unit = unit
        self._total = total
        self.value = self._start
        self._started = False

    def enter(self):
        super(debugbar, self).__setattr__("value", self._start)
        return self

    def exit(self, type, value, traceback):
        if self._started:
            self._ui.write(_x("progress: %s (end)\n") % self._topic)

    def __setattr__(self, name, value):
        if name == "value":
            self._started = True
            pos, item = _progvalue(value)
            unit = (" %s" % self._unit) if self._unit else ""
            item = (" %s" % item) if item else ""
            if self._total:
                pct = 100.0 * pos / self._total
                self._ui.write(
                    _x("progress: %s:%s %d/%d%s (%4.2f%%)\n")
                    % (self._topic, item, pos, self._total, unit, pct)
                )
            else:
                self._ui.write(
                    _x("progress: %s:%s %d%s\n") % (self._topic, item, pos, unit)
                )
        super(debugbar, self).__setattr__(name, value)


def bar(ui, topic, unit="", total=None, start=0, formatfunc=None):
    detail = None
    if ui.configbool("progress", "verbose"):
        frames = list(
            filter(lambda frame: frame.filename != __file__, traceback.extract_stack())
        )
        if frames:
            caller = frames[-1]
            path = util.splitpath(caller.filename)
            if len(path) > 2:
                path = path[-2:]
            detail = "(%s:%d)" % (pycompat.ossep.join(path), caller.lineno)

    if ui.configbool("progress", "debug"):
        return debugbar(ui, topic, unit, total, start, formatfunc)
    else:
        return normalbar(ui, topic, unit, total, start, formatfunc, detail=detail)


def spinner(ui, topic):
    return bar(ui, topic, start=None)


class iterwrapper(object):
    def __init__(self, itr, bar):
        self.itr = itr
        self.bar = bar

    def __iter__(self):
        self.bar.__enter__()
        return self

    def __next__(self):
        try:
            n = next(self.itr)
            self.bar.value += 1
            return n
        except Exception:
            # We ignore exception info, so don't bother populating.
            self.bar.__exit__(None, None, None)
            raise


def each(ui, iterable, topic, unit=""):
    b = bar(ui, topic, unit, total=len(iterable))
    return iterwrapper(iter(iterable), b)


def resetstate():
    getengine().resetstate()
