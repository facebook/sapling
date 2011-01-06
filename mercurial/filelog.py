# filelog.py - file history class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import revlog

def _parsemeta(text):
    if not text.startswith('\1\n'):
        return {}
    s = text.index('\1\n', 2)
    mt = text[2:s]
    m = {}
    for l in mt.splitlines():
        k, v = l.split(": ", 1)
        m[k] = v
    return m

class filelog(revlog.revlog):
    def __init__(self, opener, path):
        revlog.revlog.__init__(self, opener,
                        "/".join(("data", path + ".i")))

    def read(self, node):
        t = self.revision(node)
        if not t.startswith('\1\n'):
            return t
        s = t.index('\1\n', 2)
        return t[s + 2:]

    def add(self, text, meta, transaction, link, p1=None, p2=None):
        if meta or text.startswith('\1\n'):
            mt = ["%s: %s\n" % (k, v) for k, v in sorted(meta.iteritems())]
            text = "\1\n%s\1\n%s" % ("".join(mt), text)
        return self.addrevision(text, transaction, link, p1, p2)

    def renamed(self, node):
        if self.parents(node)[0] != revlog.nullid:
            return False
        t = self.revision(node)
        m = _parsemeta(t)
        if m and "copy" in m:
            return (m["copy"], revlog.bin(m["copyrev"]))
        return False

    def size(self, rev):
        """return the size of a given revision"""

        # for revisions with renames, we have to go the slow way
        node = self.node(rev)
        if self.renamed(node):
            return len(self.read(node))

        # XXX if self.read(node).startswith("\1\n"), this returns (size+4)
        return revlog.revlog.size(self, rev)

    def cmp(self, node, text):
        """compare text with a given file revision

        returns True if text is different than what is stored.
        """

        t = text
        if text.startswith('\1\n'):
            t = '\1\n\1\n' + text

        samehashes = not revlog.revlog.cmp(self, node, t)
        if samehashes:
            return False

        # renaming a file produces a different hash, even if the data
        # remains unchanged. Check if it's the case (slow):
        if self.renamed(node):
            t2 = self.read(node)
            return t2 != text

        return True
