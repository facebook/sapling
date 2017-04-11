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

from __future__ import absolute_import

def memusage(ui):
    """Report memory usage of the current process."""
    result = {'peak': 0, 'rss': 0}
    with open('/proc/self/status', 'r') as status:
        # This will only work on systems with a /proc file system
        # (like Linux).
        for line in status:
            parts = line.split()
            key = parts[0][2:-1].lower()
            if key in result:
                result[key] = int(parts[1])
    ui.write_err(", ".join(["%s: %.1f MiB" % (k, v / 1024.0)
                            for k, v in result.iteritems()]) + "\n")

def extsetup(ui):
    ui.atexit(memusage, ui)
