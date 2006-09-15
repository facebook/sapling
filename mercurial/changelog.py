# changelog.py - changelog class for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from revlog import *
from i18n import gettext as _
from demandload import demandload
demandload(globals(), "os time util")

class changelog(revlog):
    def __init__(self, opener, defversion=REVLOGV0):
        revlog.__init__(self, opener, "00changelog.i", "00changelog.d",
                        defversion)

    def extract(self, text):
        """
        format used:
        nodeid\n  : manifest node in ascii
        user\n    : user, no \n or \r allowed
        time tz\n : date (time is int or float, timezone is int)
        files\n\n : files modified by the cset, no \n or \r allowed
        (.*)      : comment (free text, ideally utf-8)
        """
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
            parseddate = "%d %d" % util.parsedate(date)
        else:
            parseddate = "%d %d" % util.makedate()
        list.sort()
        l = [hex(manifest), user, parseddate] + list + ["", desc]
        text = "\n".join(l)
        return self.addrevision(text, transaction, self.count(), p1, p2)
