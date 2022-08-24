from __future__ import absolute_import

import time

import bindings
from edenscm import progress, pycompat, registrar, util

cmdtable = {}
command = registrar.command(cmdtable)


class faketime(object):
    def __init__(self):
        self.now = 0

    def time(self):
        return self.now

    def increment(self):
        now = self.now
        self.now += 1
        return now


_faketime = faketime()
time.time = _faketime.time

unicodeloopitems = [
    pycompat.ensurestr("\u3042\u3044"),  # 2 x 2 = 4 columns
    pycompat.ensurestr("\u3042\u3044\u3046"),  # 2 x 3 = 6 columns
    pycompat.ensurestr("\u3042\u3044\u3046\u3048"),  # 2 x 4 = 8 columns
]


@command(
    "progresstest",
    [
        ("", "nested", False, "show nested results"),
        ("", "unicode", False, "use unicode topics and items"),
        ("", "output", False, "output text on each iteration"),
    ],
    "hg progresstest loops total",
    norepo=True,
)
def progresstest(ui, loops, total, **opts):
    loops = int(loops)
    total = int(total)
    if total == -1:
        total = None
    nested = opts.get("nested", None)
    useunicode = opts.get("unicode", False)
    if useunicode:
        topic = pycompat.ensurestr("\u3042\u3044\u3046\u3048")
    else:
        topic = "progress test"
    with progress.bar(ui, topic, "cycles", total) as prog:
        for i in range(loops + 1):
            if useunicode:
                prog.value = (i, unicodeloopitems[i % len(unicodeloopitems)])
            else:
                prog.value = (i, "loop %s" % i)
            syncrender()
            if nested:
                nestedtotal = 5 if i % 6 == 5 else 2
                with progress.bar(
                    ui, "nested progress", total=nestedtotal
                ) as nestedprog:
                    for j in range(nestedtotal + 1):
                        nestedprog.value = (j, "nest %s" % j)
                        syncrender()


@command("bytesprogresstest", norepo=True)
def bytesprogresstest(ui):
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
    with progress.bar(
        ui, "bytes progress test", "bytes", max(values), formatfunc=util.bytecount
    ) as prog:
        for value in values:
            prog.value = (value, "%s bytes" % value)
            syncrender()


@command("iterprogresstest", norepo=True)
def iterprogresstest(ui):
    for _ in progress.each(ui, [1, 2, 3, 4], "numbers"):
        syncrender()


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


def syncrender():
    progress.getengine().pump(_faketime.increment())
    bindings.progress.render.step()
