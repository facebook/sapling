# changelog.py - changelog class for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from revlog import *
from i18n import gettext as _
from demandload import demandload
demandload(globals(), "os time util")

class changelog(revlog):
    def __init__(self, opener, defversion=0):
        revlog.__init__(self, opener, "00changelog.i", "00changelog.d",
                        defversion)

    def extract(self, text):
        if not text:
            return (nullid, "", (0, 0), [], "")
        last = text.index("\n\n")
        desc = text[last + 2:]
        l = text[:last].splitlines()
        manifest = bin(l[0])
        user = l[1]
        date = l[2].split(' ')
        time = float(date.pop(0))
        try:
            # various tools did silly things with the time zone field.
            timezone = int(date[0])
        except:
            timezone = 0
        files = l[3:]
        return (manifest, user, (time, timezone), files, desc)

    def read(self, node):
        return self.extract(self.revision(node))

    def add(self, manifest, list, desc, transaction, p1=None, p2=None,
                  user=None, date=None):
        if date:
            # validate explicit (probably user-specified) date and
            # time zone offset. values must fit in signed 32 bits for
            # current 32-bit linux runtimes.
            try:
                when, offset = map(int, date.split(' '))
            except ValueError:
                raise ValueError(_('invalid date: %r') % date)
            if abs(when) > 0x7fffffff:
                raise ValueError(_('date exceeds 32 bits: %d') % when)
            if abs(offset) >= 43200:
                raise ValueError(_('impossible time zone offset: %d') % offset)
        else:
            date = "%d %d" % util.makedate()
        list.sort()
        l = [hex(manifest), user, date] + list + ["", desc]
        text = "\n".join(l)
        return self.addrevision(text, transaction, self.count(), p1, p2)
