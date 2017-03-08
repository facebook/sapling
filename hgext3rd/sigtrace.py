# sigtrace.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""sigtrace - dump stack traces on signal

By default, SIGUSR1 will make hg dump stacks of all threads to /tmp.

Config::

    [sigtrace]
    signal = USR1
    pathformat = /tmp/trace-%(pid)s-%(time)s.log
"""

import os
import signal
import sys
import time
import traceback

pathformat = '/tmp/trace-%(pid)s-%(time)s.log'

def printstacks(sig, currentframe):
    content = ''
    for tid, frame in sys._current_frames().iteritems():
        content += ('Thread %s:\n%s\n'
                    % (tid, ''.join(traceback.format_stack(frame))))

    with open(pathformat % {'time': time.time(), 'pid': os.getpid()}, 'w') as f:
        f.write(content)

def uisetup(ui):
    global pathformat
    pathformat = ui.config('sigtrace', 'pathformat', pathformat)
    signame = ui.config('sigtrace', 'signal', 'USR1')
    sig = getattr(signal, 'SIG' + signame, None)
    if sig is not None:
        signal.signal(sig, printstacks)
