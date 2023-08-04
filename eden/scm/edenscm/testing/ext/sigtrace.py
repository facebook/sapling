# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""sigtrace - dump stack and memory traces on SIGUSR1"""

import os
import signal
import sys
import time
import traceback


pathformat = "/tmp/trace-%(pid)s-%(time)s.log"


def printstacks(sig, currentframe) -> None:
    path = pathformat % {"time": time.time(), "pid": os.getpid()}
    writesigtrace(path, writestderr=True)


def writesigtrace(path, writestderr: bool = False) -> None:
    content = ""
    for tid, frame in sys._current_frames().items():
        tb = "".join(traceback.format_stack(frame))
        content += "Thread %s:\n%s\n" % (
            tid,
            tb,
        )

    with open(path, "w") as f:
        f.write(content)

    # Also print to stderr
    sys.stderr.write(content)
    sys.stderr.write("\nStacktrace written to %s\n" % path)
    sys.stderr.flush()


def testsetup(t):
    sig = getattr(signal, "SIGUSR1")
    if sig is not None:
        signal.signal(sig, printstacks)
        sys.stderr.write(
            "sigtrace: use 'kill -USR1 %d' to dump stacktrace\n" % os.getpid()
        )
        sys.stderr.flush()
