# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os
import signal
import sys

from edenscm import error

# Make sure we use sshaskpass.py in this repo, unaffected by PYTHONPATH
from edenscm.ext import sshaskpass
from hghave import require


require(["py2"])


if not sys.platform.startswith("linux"):
    sys.stderr.write("this test only supports linux\n")
    sys.exit(80)


# stdin, stderr have to be tty to run test
pid, master = os.forkpty()
if pid:
    # parent, test some I/O
    os.write(master, "(input)\n")
    with os.fdopen(master, "r") as f:
        sys.stdout.write("pty receives: %r" % f.read())
    os.waitpid(pid, 0)
    sys.exit(0)

sigterm = getattr(signal, "SIGTERM", None)
if sigterm:

    def catchterm(*args):
        raise error.SignalInterrupt

    signal.signal(sigterm, catchterm)

# child, start a ttyserver and do some I/O
ttysrvpid, sockpath = sshaskpass._startttyserver()

try:
    r, w = sshaskpass._receivefds(sockpath)
    with os.fdopen(r) as f:
        line = f.readline()
        os.write(w, "client receives: " + line)
finally:
    sshaskpass._killprocess(ttysrvpid)
    os.unlink(sockpath)
