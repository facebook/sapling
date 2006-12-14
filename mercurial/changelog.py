# changelog.py - changelog class for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from revlog import *
from i18n import gettext as _
import os, time, util

def _string_escape(text):
    """
    >>> d = {'nl': chr(10), 'bs': chr(92), 'cr': chr(13), 'nul': chr(0)}
    >>> s = "ab%(nl)scd%(bs)s%(bs)sn%(nul)sab%(cr)scd%(bs)s%(nl)s" % d
    >>> s
    'ab\\ncd\\\\\\\\n\\x00ab\\rcd\\\\\\n'
    >>> res = _string_escape(s)
    >>> s == _string_unescape(res)
    True
    """
    # subset of the string_escape codec
    text = text.replace('\\', '\\\\').replace('\n', '\\n').replace('\r', '\\r')
    return text.replace('\0', '\\0')

def _string_unescape(text):
    return text.decode('string_escape')

class changelog(revlog):
    def __init__(self, opener, defversion=REVLOGV0):
        revlog.__init__(self, opener, "00changelog.i", "00changelog.d",
                        defversion)

    def decode_extra(self, text):
        extra = {}
        for l in text.split('\0'):
            if not l:
                continue
            k, v = _string_unescape(l).split(':', 1)
            extra[k] = v
        return extra

    def encode_extra(self, d):
        items = [_string_escape(":".join(t)) for t in d.iteritems()]
        return "\0".join(items)

    def extract(self, text):
        """
        format used:
        nodeid\n        : manifest node in ascii
        user\n          : user, no \n or \r allowed
        time tz extra\n : date (time is int or float, timezone is int)
                        : extra is metadatas, encoded and separated by '\0'
                        : older versions ignore it
        files\n\n       : files modified by the cset, no \n or \r allowed
        (.*)            : comment (free text, ideally utf-8)

        changelog v0 doesn't use extra
        """
        if not text:
            return (nullid, "", (0, 0), [], "", {})
        last = text.index("\n\n")
        desc = util.tolocal(text[last + 2:])
        l = text[:last].split('\n')
        manifest = bin(l[0])
        user = util.tolocal(l[1])

        extra_data = l[2].split(' ', 2)
        if len(extra_data) != 3:
            time = float(extra_data.pop(0))
            try:
                # various tools did silly things with the time zone field.
                timezone = int(extra_data[0])
            except:
                timezone = 0
            extra = {}
        else:
            time, timezone, extra = extra_data
            time, timezone = float(time), int(timezone)
            extra = self.decode_extra(extra)
        files = l[3:]
        return (manifest, user, (time, timezone), files, desc, extra)

    def read(self, node):
        return self.extract(self.revision(node))

    def add(self, manifest, list, desc, transaction, p1=None, p2=None,
                  user=None, date=None, extra={}):

        user, desc = util.fromlocal(user), util.fromlocal(desc)

        if date:
            parseddate = "%d %d" % util.parsedate(date)
        else:
            parseddate = "%d %d" % util.makedate()
        if extra:
            extra = self.encode_extra(extra)
            parseddate = "%s %s" % (parseddate, extra)
        list.sort()
        l = [hex(manifest), user, parseddate] + list + ["", desc]
        text = "\n".join(l)
        return self.addrevision(text, transaction, self.count(), p1, p2)
