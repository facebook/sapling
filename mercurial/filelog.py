# filelog.py - file history class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import bin, nullid
from revlog import revlog

class filelog(revlog):
    def __init__(self, opener, path):
        revlog.__init__(self, opener,
                        "/".join(("data", self.encodedir(path + ".i"))))

    # This avoids a collision between a file named foo and a dir named
    # foo.i or foo.d
    def encodedir(self, path):
        return (path
                .replace(".hg/", ".hg.hg/")
                .replace(".i/", ".i.hg/")
                .replace(".d/", ".d.hg/"))

    def decodedir(self, path):
        return (path
                .replace(".d.hg/", ".d/")
                .replace(".i.hg/", ".i/")
                .replace(".hg.hg/", ".hg/"))

    def read(self, node):
        t = self.revision(node)
        if not t.startswith('\1\n'):
            return t
        s = t.index('\1\n', 2)
        return t[s+2:]

    def _readmeta(self, node):
        t = self.revision(node)
        if not t.startswith('\1\n'):
            return {}
        s = t.index('\1\n', 2)
        mt = t[2:s]
        m = {}
        for l in mt.splitlines():
            k, v = l.split(": ", 1)
            m[k] = v
        return m

    def add(self, text, meta, transaction, link, p1=None, p2=None):
        if meta or text.startswith('\1\n'):
            mt = ""
            if meta:
                mt = [ "%s: %s\n" % (k, v) for k,v in meta.items() ]
            text = "\1\n%s\1\n%s" % ("".join(mt), text)
        return self.addrevision(text, transaction, link, p1, p2)

    def renamed(self, node):
        if self.parents(node)[0] != nullid:
            return False
        m = self._readmeta(node)
        if m and "copy" in m:
            return (m["copy"], bin(m["copyrev"]))
        return False

    def size(self, rev):
        """return the size of a given revision"""

        # for revisions with renames, we have to go the slow way
        node = self.node(rev)
        if self.renamed(node):
            return len(self.read(node))

        return revlog.size(self, rev)

    def cmp(self, node, text):
        """compare text with a given file revision"""

        # for renames, we have to go the slow way
        if self.renamed(node):
            t2 = self.read(node)
            return t2 != text

        return revlog.cmp(self, node, text)
