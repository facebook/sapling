# progress.py progress bars related code
#
# Copyright (C) 2010 Augie Fackler <durin42@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import errno
import threading
import time

from edenscmnative import threading as rustthreading

from . import encoding, util
from .i18n import _


def spacejoin(*args):
    return " ".join(s for s in args if s)


def shouldprint(ui):
    return not (ui.quiet or ui.plain("progress")) and (
        ui._isatty(ui.ferr) or ui.configbool("progress", "assume-tty")
    )


def fmtremaining(seconds):
    """format a number of remaining seconds in human readable way

    This will properly display seconds, minutes, hours, days if needed"""
    if seconds is None:
        return ""

    if seconds < 60:
        # i18n: format XX seconds as "XXs"
        return _("%02ds") % (seconds)
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
        return delta / elapsed
    return None


# file_write() and file_flush() of Python 2 do not restart on EINTR if
# the file is attached to a "slow" device (e.g. a terminal) and raise
# IOError. We cannot know how many bytes would be written by file_write(),
# but a progress text is known to be short enough to be written by a
# single write() syscall, so we can just retry file_write() with the whole
# text. (issue5532)
#
# This should be a short-term workaround. We'll need to fix every occurrence
# of write() to a terminal or pipe.
def _eintrretry(func, *args):
    while True:
        try:
            return func(*args)
        except IOError as err:
            if err.errno == errno.EINTR:
                continue
            raise


class baserenderer(object):
    """progress bar renderer for classic-style progress bars"""

    def __init__(self, bar):
        self._bar = bar
        self.printed = False
        self.configwidth = bar._ui.config("progress", "width", default=None)

    def _flusherr(self):
        _eintrretry(self._bar._ui.ferr.flush)

    def _writeerr(self, msg):
        _eintrretry(self._bar._ui.ferr.write, msg)

    def width(self):
        ui = self._bar._ui
        tw = ui.termwidth()
        if self.configwidth is not None:
            return min(int(self.configwidth), tw)
        else:
            return tw

    def show(self):
        raise NotImplementedError()

    def clear(self):
        if not self.printed:
            return
        self._writeerr("\r%s\r" % (" " * self.width()))

    def complete(self):
        if not self.printed:
            return
        self.show(time.time())
        self._writeerr("\n")


class classicrenderer(baserenderer):
    def __init__(self, bar):
        super(classicrenderer, self).__init__(bar)
        self.order = bar._ui.configlist("progress", "format")

    def show(self, now):
        pos, item = _progvalue(self._bar.value)
        if pos is None:
            pos = round(now - self._bar._enginestarttime, 1)
        formatfunc = self._bar._formatfunc
        if formatfunc is None:
            formatfunc = str
        topic = self._bar._topic
        unit = self._bar._unit
        total = self._bar._total
        termwidth = self.width()
        self.printed = True
        head = ""
        needprogress = False
        tail = ""
        for indicator in self.order:
            add = ""
            if indicator == "topic":
                add = topic
            elif indicator == "number":
                fpos = formatfunc(pos)
                if total:
                    ftotal = formatfunc(total)
                    maxlen = max(len(fpos), len(ftotal))
                    add = ("% " + str(maxlen) + "s/%s") % (fpos, ftotal)
                else:
                    add = fpos
            elif indicator.startswith("item") and item:
                slice = "end"
                if "-" in indicator:
                    wid = int(indicator.split("-")[1])
                elif "+" in indicator:
                    slice = "beginning"
                    wid = int(indicator.split("+")[1])
                else:
                    wid = 20
                if slice == "end":
                    add = encoding.trim(item, wid, leftside=True)
                else:
                    add = encoding.trim(item, wid)
                add += (wid - encoding.colwidth(add)) * " "
            elif indicator == "bar":
                add = ""
                needprogress = True
            elif indicator == "unit" and unit:
                add = unit
            elif indicator == "estimate":
                add = fmtremaining(estimateremaining(self._bar))
            elif indicator == "speed":
                add = fmtspeed(estimatespeed(self._bar), self._bar)
            if not needprogress:
                head = spacejoin(head, add)
            else:
                tail = spacejoin(tail, add)
        if needprogress:
            used = 0
            if head:
                used += encoding.colwidth(head) + 1
            if tail:
                used += encoding.colwidth(tail) + 1
            progwidth = termwidth - used - 3
            if pos is not None and total and pos <= total:
                amt = pos * progwidth // total
                bar = "=" * (amt - 1)
                if amt > 0:
                    bar += ">"
                bar += " " * (progwidth - amt)
            else:
                elapsed = now - self._bar._enginestarttime
                indetpos = int(elapsed / self._bar._refresh)
                progwidth -= 3
                # mod the count by twice the width so we can make the
                # cursor bounce between the right and left sides
                amt = indetpos % (2 * progwidth)
                amt -= progwidth
                bar = " " * int(progwidth - abs(amt)) + "<=>" + " " * int(abs(amt))
            prog = "".join(("[", bar, "]"))
            out = spacejoin(head, prog, tail)
        else:
            out = spacejoin(head, tail)
        self._writeerr("\r" + encoding.trim(out, termwidth))
        self._flusherr()


class fancyrenderer(baserenderer):
    def __init__(self, bar):
        super(fancyrenderer, self).__init__(bar)

    def _mergespans(self, leftspans, rightspans):
        spans = []
        leftspans.reverse()
        rightspans.reverse()
        while leftspans and rightspans:
            leftwidth, leftlabel = leftspans.pop()
            rightwidth, rightlabel = rightspans.pop()
            if leftwidth < rightwidth:
                spans.append((leftwidth, spacejoin(leftlabel, rightlabel)))
                rightspans.append((rightwidth - leftwidth, rightlabel))
            elif leftwidth == rightwidth:
                spans.append((leftwidth, spacejoin(leftlabel, rightlabel)))
            elif leftwidth > rightwidth:
                spans.append((rightwidth, spacejoin(leftlabel, rightlabel)))
                leftspans.append((leftwidth - rightwidth, leftlabel))
        spans.extend(reversed(leftspans))
        spans.extend(reversed(rightspans))
        return spans

    def _applyspans(self, ui, line, spans):
        out = []
        outpos = 0
        outdebt = 0
        linebyte = 0
        linewidth = encoding.colwidth(line)
        spans.reverse()
        while outpos < linewidth:
            if not spans:
                out.append(line[linebyte:])
                break
            spanwidth, spanlabel = spans.pop()
            spantext = encoding.trim(line[linebyte:], spanwidth + outdebt)
            outdebt += spanwidth - encoding.colwidth(spantext)
            linebyte += len(spantext)
            out.append(ui.label(spantext, spanlabel))
            outpos += spanwidth
        return "".join(out)

    def show(self, now):
        topic = self._bar._topic
        total = self._bar._total
        pos, item = _progvalue(self._bar.value)
        if total:
            style = "normal"
        else:
            if pos is None:
                style = "spinner"
                pos = round(now - self._bar._enginestarttime, 1)
            else:
                style = "indet"
            spinpos = int((now - self._bar._enginestarttime) * 20)
        termwidth = self.width()
        self.printed = True
        formatfunc = self._bar._formatfunc or str
        fpos = formatfunc(pos)
        if total:
            ftotal = formatfunc(total)
            number = ("% " + str(len(ftotal)) + "s/%s") % (fpos, ftotal)
            remaining = " " + fmtremaining(estimateremaining(self._bar))
        else:
            number = fpos
            remaining = ""

        start = " %s" % topic
        if item:
            start += ": "
        end = "  %s%s " % (number, remaining)
        startwidth = encoding.colwidth(start)
        endwidth = encoding.colwidth(end)
        midwidth = termwidth - startwidth - endwidth
        mid = encoding.trim(item + " " * midwidth, midwidth)
        line = encoding.trim(start + mid + end, termwidth)
        if style == "normal":
            progpos = termwidth * pos // total
            spans = [
                (progpos, "progress.fancy.bar.normal"),
                (termwidth - progpos, "progress.fancy.bar.background"),
            ]
        elif style == "indet":
            spinnerwidth = min(6, termwidth / 2)
            progpos = spinpos % ((termwidth - spinnerwidth) * 2)
            if progpos >= (termwidth - spinnerwidth):
                progpos = 2 * (termwidth - spinnerwidth) - progpos
            spans = [
                (progpos, "progress.fancy.bar.background"),
                (spinnerwidth, "progress.fancy.bar.indeterminate"),
                (termwidth - spinnerwidth - progpos, "progress.fancy.bar.background"),
            ]
        elif style == "spinner":
            spinnerwidth = min(6, termwidth / 2)
            progpos = spinpos % termwidth
            spans = []
            on = "progress.fancy.bar.spinner"
            off = "progress.fancy.bar.background"
            if progpos > termwidth - spinnerwidth:
                spans = [
                    (spinnerwidth - (termwidth - progpos), on),
                    (termwidth - spinnerwidth, off),
                    (termwidth - progpos, on),
                ]
            else:
                spans = [
                    (progpos, off),
                    (spinnerwidth, on),
                    (termwidth - spinnerwidth - progpos, off),
                ]

        spans = self._mergespans(
            spans,
            [
                (startwidth, "progress.fancy.topic"),
                (midwidth, "progress.fancy.item"),
                (endwidth, "progress.fancy.count"),
            ],
        )
        line = self._applyspans(self._bar._ui, line, spans)
        self._writeerr("\r" + line + "\r")
        self._flusherr()


class nullrenderer(baserenderer):
    def __init__(self, bar):
        super(nullrenderer, self).__init__(bar)

    def show(self, now):
        pass


renderers = {"classic": classicrenderer, "fancy": fancyrenderer, "none": nullrenderer}


def getrenderer(bar):
    renderername = bar._ui.config("progress", "renderer")
    return renderers.get(renderername, classicrenderer)(bar)


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
        with self.lock():
            bar = self._currentbar()
            if bar is not None:
                bar._enginerenderer.clear()
            yield

    def resetstate(self):
        with self.lock():
            self._clear()
            self._bars = []
            self._currentbarindex = None
            self._refresh = None
            self._cond.notify_all()

    def register(self, bar):
        with self.lock():
            self._activate(bar._ui)
            now = time.time()
            bar._enginestarttime = now
            bar._enginerenderer = getrenderer(bar)
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
                    else:
                        self._clear()
                del self._bars[index:]
                self._recalculatedisplay(time.time())
                if not self._bars:
                    global suspend
                    suspend = util.nullcontextmanager
                # Do not redraw when unregistering a nested bar
                if len(self._bars) < 1:
                    self._cond.notify_all()
            bar._enginerenderer = None

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
        with self.lock():
            bar = self._currentbar()
            if bar is not None:
                bar._enginerenderer.show(now)

    def _clear(self):
        with self.lock():
            bar = self._currentbar()
            if bar is not None:
                bar._enginerenderer.clear()

    def _complete(self):
        with self.lock():
            bar = self._currentbar()
            if bar is not None:
                if bar._ui.configbool("progress", "clear-complete"):
                    bar._enginerenderer.clear()
                else:
                    bar._enginerenderer.complete()

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


_engine = engine()


suspend = util.nullcontextmanager


def _progvalue(value):
    """split a progress bar value into a position and item"""
    if isinstance(value, tuple):
        return value
    else:
        return value, ""


class normalbar(object):
    """context manager that adds a progress bar to slow operations

    To use this, wrap a section of code that takes a long time like this:

    with progress.bar(ui, "topic") as prog:
        # processing code
        prog.value = pos
        # alternatively: prog.value = (pos, item)
    """

    def __init__(self, ui, topic, unit="", total=None, start=0, formatfunc=None):
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

    def reset(self, topic, unit="", total=None):
        with _engine.lock():
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

    def __enter__(self):
        self.value = self._start
        _engine.register(self)
        return self

    def __exit__(self, type, value, traceback):
        _engine.unregister(self)


class debugbar(object):
    def __init__(self, ui, topic, unit="", total=None, start=0, formatfunc=None):
        self._ui = ui
        self._topic = topic
        self._unit = unit
        self._total = total
        self._start = start
        self._formatfunc = formatfunc
        self._started = False

    def reset(self, topic, unit="", total=None):
        if self._started:
            self._ui.write(("progress: %s (reset)\n") % self._topic)
        self._topic = topic
        self._unit = unit
        self._total = total
        self.value = self._start
        self._started = False

    def __enter__(self):
        super(debugbar, self).__setattr__("value", self._start)
        return self

    def __exit__(self, type, value, traceback):
        if self._started:
            self._ui.write(("progress: %s (end)\n") % self._topic)

    def __setattr__(self, name, value):
        if name == "value":
            self._started = True
            pos, item = _progvalue(value)
            unit = (" %s" % self._unit) if self._unit else ""
            item = (" %s" % item) if item else ""
            if self._total:
                pct = 100.0 * pos / self._total
                self._ui.write(
                    ("progress: %s:%s %d/%d%s (%4.2f%%)\n")
                    % (self._topic, item, pos, self._total, unit, pct)
                )
            else:
                self._ui.write(
                    ("progress: %s:%s %d%s\n") % (self._topic, item, pos, unit)
                )
        super(debugbar, self).__setattr__(name, value)


class nullbar(object):
    """A progress bar context manager that just keeps track of state."""

    def __init__(self, ui, topic, unit="", total=None, start=0, formatfunc=None):
        self._topic = topic
        self._unit = unit
        self._total = total
        self._start = start
        self._formatfunc = formatfunc

    def reset(self, topic, unit="", total=None):
        self._topic = topic
        self._unit = unit
        self._total = total
        self.value = self._start

    def __enter__(self):
        self.value = self._start
        return self

    def __exit__(self, type, value, traceback):
        pass


def bar(ui, topic, unit="", total=None, start=0, formatfunc=None):
    if ui.configbool("progress", "debug"):
        return debugbar(ui, topic, unit, total, start, formatfunc)
    elif (
        ui.quiet
        or ui.debugflag
        or ui.configbool("progress", "disable")
        or not shouldprint(ui)
    ):
        return nullbar(ui, topic, unit, total, start, formatfunc)
    else:
        return normalbar(ui, topic, unit, total, start, formatfunc)


def spinner(ui, topic):
    return bar(ui, topic, start=None)


def resetstate():
    _engine.resetstate()
