# progress.py progress bars related code
#
# Copyright (C) 2010 Augie Fackler <durin42@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import sys
import time
import threading
from mercurial import encoding

from mercurial.i18n import _


def spacejoin(*args):
    return ' '.join(s for s in args if s)

def shouldprint(ui):
    return not (ui.quiet or ui.plain()) and (
        ui._isatty(sys.stderr) or ui.configbool('progress', 'assume-tty'))

def fmtremaining(seconds):
    """format a number of remaining seconds in humain readable way

    This will properly display seconds, minutes, hours, days if needed"""
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

class progbar(object):
    def __init__(self, ui):
        self.ui = ui
        self._refreshlock = threading.Lock()
        self.resetstate()

    def resetstate(self):
        self.topics = []
        self.topicstates = {}
        self.starttimes = {}
        self.startvals = {}
        self.printed = False
        self.lastprint = time.time() + float(self.ui.config(
            'progress', 'delay', default=3))
        self.curtopic = None
        self.lasttopic = None
        self.indetcount = 0
        self.refresh = float(self.ui.config(
            'progress', 'refresh', default=0.1))
        self.changedelay = max(3 * self.refresh,
                               float(self.ui.config(
                                   'progress', 'changedelay', default=1)))
        self.order = self.ui.configlist(
            'progress', 'format',
            default=['topic', 'bar', 'number', 'estimate'])

    def show(self, now, topic, pos, item, unit, total):
        if not shouldprint(self.ui):
            return
        termwidth = self.width()
        self.printed = True
        head = ''
        needprogress = False
        tail = ''
        for indicator in self.order:
            add = ''
            if indicator == 'topic':
                add = topic
            elif indicator == 'number':
                if total:
                    add = ('% ' + str(len(str(total))) +
                           's/%s') % (pos, total)
                else:
                    add = str(pos)
            elif indicator.startswith('item') and item:
                slice = 'end'
                if '-' in indicator:
                    wid = int(indicator.split('-')[1])
                elif '+' in indicator:
                    slice = 'beginning'
                    wid = int(indicator.split('+')[1])
                else:
                    wid = 20
                if slice == 'end':
                    add = encoding.trim(item, wid, leftside=True)
                else:
                    add = encoding.trim(item, wid)
                add += (wid - encoding.colwidth(add)) * ' '
            elif indicator == 'bar':
                add = ''
                needprogress = True
            elif indicator == 'unit' and unit:
                add = unit
            elif indicator == 'estimate':
                add = self.estimate(topic, pos, total, now)
            elif indicator == 'speed':
                add = self.speed(topic, pos, unit, now)
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
            if total and pos <= total:
                amt = pos * progwidth // total
                bar = '=' * (amt - 1)
                if amt > 0:
                    bar += '>'
                bar += ' ' * (progwidth - amt)
            else:
                progwidth -= 3
                self.indetcount += 1
                # mod the count by twice the width so we can make the
                # cursor bounce between the right and left sides
                amt = self.indetcount % (2 * progwidth)
                amt -= progwidth
                bar = (' ' * int(progwidth - abs(amt)) + '<=>' +
                       ' ' * int(abs(amt)))
            prog = ''.join(('[', bar , ']'))
            out = spacejoin(head, prog, tail)
        else:
            out = spacejoin(head, tail)
        sys.stderr.write('\r' + encoding.trim(out, termwidth))
        self.lasttopic = topic
        sys.stderr.flush()

    def clear(self):
        if not shouldprint(self.ui):
            return
        sys.stderr.write('\r%s\r' % (' ' * self.width()))

    def complete(self):
        if not shouldprint(self.ui):
            return
        if self.ui.configbool('progress', 'clear-complete', default=True):
            self.clear()
        else:
            sys.stderr.write('\n')
        sys.stderr.flush()

    def width(self):
        tw = self.ui.termwidth()
        return min(int(self.ui.config('progress', 'width', default=tw)), tw)

    def estimate(self, topic, pos, total, now):
        if total is None:
            return ''
        initialpos = self.startvals[topic]
        target = total - initialpos
        delta = pos - initialpos
        if delta > 0:
            elapsed = now - self.starttimes[topic]
            if elapsed > float(
                self.ui.config('progress', 'estimate', default=2)):
                seconds = (elapsed * (target - delta)) // delta + 1
                return fmtremaining(seconds)
        return ''

    def speed(self, topic, pos, unit, now):
        initialpos = self.startvals[topic]
        delta = pos - initialpos
        elapsed = now - self.starttimes[topic]
        if elapsed > float(
            self.ui.config('progress', 'estimate', default=2)):
            return _('%d %s/sec') % (delta / elapsed, unit)
        return ''

    def _oktoprint(self, now):
        '''Check if conditions are met to print - e.g. changedelay elapsed'''
        if (self.lasttopic is None # first time we printed
            # not a topic change
            or self.curtopic == self.lasttopic
            # it's been long enough we should print anyway
            or now - self.lastprint >= self.changedelay):
            return True
        else:
            return False

    def progress(self, topic, pos, item='', unit='', total=None):
        now = time.time()
        self._refreshlock.acquire()
        try:
            if pos is None:
                self.starttimes.pop(topic, None)
                self.startvals.pop(topic, None)
                self.topicstates.pop(topic, None)
                # reset the progress bar if this is the outermost topic
                if self.topics and self.topics[0] == topic and self.printed:
                    self.complete()
                    self.resetstate()
                # truncate the list of topics assuming all topics within
                # this one are also closed
                if topic in self.topics:
                    self.topics = self.topics[:self.topics.index(topic)]
                    # reset the last topic to the one we just unwound to,
                    # so that higher-level topics will be stickier than
                    # lower-level topics
                    if self.topics:
                        self.lasttopic = self.topics[-1]
                    else:
                        self.lasttopic = None
            else:
                if topic not in self.topics:
                    self.starttimes[topic] = now
                    self.startvals[topic] = pos
                    self.topics.append(topic)
                self.topicstates[topic] = pos, item, unit, total
                self.curtopic = topic
                if now - self.lastprint >= self.refresh and self.topics:
                    if self._oktoprint(now):
                        self.lastprint = now
                        self.show(now, topic, *self.topicstates[topic])
        finally:
            self._refreshlock.release()
