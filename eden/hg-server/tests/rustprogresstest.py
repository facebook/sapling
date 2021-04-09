# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import time

from bindings import progress as rustprogress
from edenscm.mercurial import progress, registrar


cmdtable = {}
command = registrar.command(cmdtable)


class faketime(object):
    def __init__(self):
        self.now = 0.0

    def time(self):
        return self.now

    def increment(self):
        now = self.now
        self.now += 1.0
        return now


_faketime = faketime()
time.time = _faketime.time


@command("rustspinnertest", [], "hg rustspinnertest loops", norepo=True)
def rustspinnertest(ui, loops):
    loops = int(loops)

    with rustprogress.spinner(ui, "progress spinner test") as prog:
        for i in range(loops + 1):
            progress.getengine().pump(_faketime.increment())
            prog.set_message("loop %d" % (i + 1))


@command("rustprogresstest", [], "hg progresstest loops total", norepo=True)
def rustprogresstest(ui, loops, total):
    loops = int(loops)
    total = int(total)

    if total == -1:
        total = None
        halfway = None
    else:
        # We will update the total halfway through.
        halfway = total / 2

    with rustprogress.bar(ui, "progress bar test", halfway, "cycles") as prog:
        for i in range(loops + 1):
            # Test updating the total.
            if prog.total() == i - 1:
                prog.set_total(total)

            progress.getengine().pump(_faketime.increment())

            prog.increment(1)
            pos = prog.position()
            prog.set_message("loop %d" % pos)


@command("rustbytesprogresstest", norepo=True)
def rustbytesprogresstest(ui):
    values = [
        0,
        10,
        250,
        999,
        1000,
        1024,
        22000,
        1048576,
        1474560,
        123456789,
        555555555,
        1000000000,
        1111111111,
    ]
    with rustprogress.bar(ui, "bytes test", max(values), "bytes") as prog:
        for i, value in enumerate(values):
            prog.set(value)
            prog.set_message("loop %d" % (i + 1))
            progress.getengine().pump(_faketime.increment())


def uisetup(ui):
    class syncengine(progress.getengine().__class__):
        def _activate(self, ui):
            pass

        def _deactivate(self):
            pass

        def pump(self, now):
            self._recalculatedisplay(now)
            self._updateestimation(now)
            self._show(now)

    progress.getengine().__class__ = syncengine
