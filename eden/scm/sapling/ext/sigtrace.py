# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""sigtrace - dump stack and memory traces on signal

By default, SIGUSR1 will make hg dump stacks of all threads and SIGUSR2 will
dump memory traces. All traces are dumped to /tmp by default.

Config::

    [sigtrace]
    signal = USR1
    memsignal = USR2
    pathformat = /tmp/trace-%(pid)s-%(time)s.log
    mempathformat = /tmp/memtrace-%(pid)s-%(time)s.log

    # start a background thread that writes traces to .hg/traces every 120
    # seconds.
    interval = 120
"""

import os
import signal
import sys
import threading
import time

from sapling import tracing, util


def printstacks(sig, currentframe) -> None:
    path = pathformat % {"time": time.time(), "pid": os.getpid()}
    writesigtrace(path, writestderr=True)


def writesigtrace(path, writestderr: bool = False) -> None:
    content = ""
    for tid, frame in sys._current_frames().items():
        content += "Thread %s:\n%s\n" % (
            tid,
            util.smarttraceback(frame, skipboring=False),
        )

    with open(path, "w") as f:
        f.write(content)

    # Also print to stderr
    if writestderr:
        sys.stderr.write(content)
        sys.stderr.write("\nStacktrace written to %s\n" % path)
        sys.stderr.flush()

    # Calculate the tracing data (can take a while) and write it.
    content = "Tracing Data:\n%s\n" % util.tracer.ascii()
    with open(path, "a") as f:
        f.write("\n")
        f.write(content)

    if writestderr:
        sys.stderr.write(content)
        sys.stderr.write("\nTracing data written to %s\n" % path)
        sys.stderr.flush()


memorytracker = []


def printmemory(sig, currentframe) -> None:
    try:
        # pyre-fixme[21]: Could not find `pympler`.
        from pympler import muppy, summary

        muppy.get_objects
    except ImportError:
        return

    all_objects = muppy.get_objects()
    sum1 = summary.summarize(all_objects)
    path = mempathformat % {"time": time.time(), "pid": os.getpid()}
    with open(path, "w") as f:
        f.write("\n".join(summary.format_(sum1, limit=50, sort="#")))


def uisetup(ui) -> None:
    global pathformat, mempathformat
    pathformat = ui.config("sigtrace", "pathformat")
    mempathformat = ui.config("sigtrace", "mempathformat")
    signame = ui.config("sigtrace", "signal")
    sig = getattr(signal, "SIG" + signame, None)
    if sig is not None:
        util.signal(sig, printstacks)

    sig2name = ui.config("sigtrace", "memsignal")
    sig2 = getattr(signal, "SIG" + sig2name, None)
    if sig2:
        util.signal(sig2, printmemory)


_runningpid = None


def reposetup(ui, repo) -> None:
    interval = ui.configint("sigtrace", "interval")
    if not interval or interval <= 0:
        return

    # Only start one sigtrace thread per-process.
    mypid = os.getpid()
    global _runningpid
    if _runningpid == mypid:
        return
    _runningpid = mypid

    knownlongruning = ui.cmdname in {"web"} and not util.istest()

    def writesigtracethread(path, interval):
        try:
            dir = os.path.dirname(path)
            util.makedirs(dir)
            while True:
                time.sleep(interval)
                # Keep 10 minutes of sigtraces.
                util.gcdir(dir, 60 * 10)

                # Only track known long-running commands when the "force"
                # file exists.
                if knownlongruning:
                    forcefile = "force_sigtrace_%s" % (mypid,)
                    if repo.localvfs.exists(forcefile):
                        repo.localvfs.tryunlink(forcefile)
                    else:
                        continue

                writesigtrace(path)
        except Exception:
            pass

    tracing.debug("starting sigtrace thread", target="ext::sigtrace")

    path = repo.localvfs.join("sigtrace", "pid-%s-%s" % (mypid, ui.cmdname))
    thread = threading.Thread(
        target=writesigtracethread, args=(path, interval), name="sigtracethread"
    )
    thread.daemon = True
    thread.start()
