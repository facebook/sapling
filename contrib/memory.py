# memory.py - track memory usage
#
# Copyright 2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''helper extension to measure memory usage

Reads current and peak memory usage from ``/proc/self/status`` and
prints it to ``stderr`` on exit.
'''

import atexit

def memusage(ui):
    """Report memory usage of the current process."""
    status = None
    result = {'peak': 0, 'rss': 0}
    try:
        # This will only work on systems with a /proc file system
        # (like Linux).
        status = open('/proc/self/status', 'r')
        for line in status:
            parts = line.split()
            key = parts[0][2:-1].lower()
            if key in result:
                result[key] = int(parts[1])
    finally:
        if status is not None:
            status.close()
    ui.write_err(", ".join(["%s: %.1f MiB" % (key, value / 1024.0)
                            for key, value in result.iteritems()]) + "\n")

def extsetup(ui):
    atexit.register(memusage, ui)
