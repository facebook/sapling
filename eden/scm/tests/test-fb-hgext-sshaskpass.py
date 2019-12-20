from __future__ import absolute_import

import os
import sys

# Make sure we use sshaskpass.py in this repo, unaffected by PYTHONPATH
from edenscm.hgext import sshaskpass


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
