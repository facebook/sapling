# sigtrace.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""sigtrace - dump stack and memory traces on signal

By default, SIGUSR1 will make hg dump stacks of all threads and SIGUSR2 will
dump memory traces. All traces are dumped to /tmp by default.

Config::

    [sigtrace]
    signal = USR1
    memsignal = USR2
    pathformat = /tmp/trace-%(pid)s-%(time)s.log
    mempathformat = /tmp/memtrace-%(pid)s-%(time)s.log
"""

import os
import signal
import sys
import time
import traceback

from edenscm.mercurial import registrar, util


pathformat = "/tmp/trace-%(pid)s-%(time)s.log"
mempathformat = "/tmp/memtrace-%(pid)s-%(time)s.log"

configtable = {}
configitem = registrar.configitem(configtable)

configitem("sigtrace", "pathformat", default=pathformat)
configitem("sigtrace", "signal", default="USR1")
configitem("sigtrace", "mempathformat", default=mempathformat)
configitem("sigtrace", "memsignal", default="USR2")


def printstacks(sig, currentframe):
    content = ""
    for tid, frame in sys._current_frames().iteritems():
        content += "Thread %s:\n%s\n" % (tid, util.smarttraceback(frame))

    path = pathformat % {"time": time.time(), "pid": os.getpid()}
    with open(path, "w") as f:
        f.write(content)

    # Also print to stderr
    sys.stderr.write(content)
    sys.stderr.write("\nStacktrace written to %s\n" % path)
    sys.stderr.flush()


memorytracker = []


def printmemory(sig, currentframe):
    try:
        from pympler import muppy, summary

        muppy.get_objects
    except ImportError:
        return

    all_objects = muppy.get_objects()
    sum1 = summary.summarize(all_objects)
    path = mempathformat % {"time": time.time(), "pid": os.getpid()}
    with open(path, "w") as f:
        f.write("\n".join(summary.format_(sum1, limit=50, sort="#")))


def uisetup(ui):
    global pathformat, mempathformat
    pathformat = ui.config("sigtrace", "pathformat")
    mempathformat = ui.config("sigtrace", "mempathformat")
    signame = ui.config("sigtrace", "signal")
    sig = getattr(signal, "SIG" + signame, None)
    if sig is not None:
        signal.signal(sig, printstacks)

    sig2name = ui.config("sigtrace", "memsignal")
    sig2 = getattr(signal, "SIG" + sig2name, None)
    if sig2:
        signal.signal(sig2, printmemory)
