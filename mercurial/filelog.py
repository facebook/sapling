# filelog.py - file history class for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os
from revlog import *
from demandload import *
demandload(globals(), "bdiff")

class filelog(revlog):
    def __init__(self, opener, path):
        revlog.__init__(self, opener,
                        os.path.join("data", self.encodedir(path + ".i")),
                        os.path.join("data", self.encodedir(path + ".d")))

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
        s = t.find('\1\n', 2)
        return t[s+2:]

    def readmeta(self, node):
        t = self.revision(node)
        if not t.startswith('\1\n'):
            return {}
        s = t.find('\1\n', 2)
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
            text = "\1\n" + "".join(mt) + "\1\n" + text
        return self.addrevision(text, transaction, link, p1, p2)

    def renamed(self, node):
        if 0 and self.parents(node)[0] != nullid:
            return False
        m = self.readmeta(node)
        if m and m.has_key("copy"):
            return (m["copy"], bin(m["copyrev"]))
        return False

    def annotate(self, node):

        def decorate(text, rev):
            return ([rev] * len(text.splitlines()), text)

        def pair(parent, child):
            for a1, a2, b1, b2 in bdiff.blocks(parent[1], child[1]):
                child[0][b1:b2] = parent[0][a1:a2]
            return child

        # find all ancestors
        needed = {node:1}
        visit = [node]
        while visit:
            n = visit.pop(0)
            for p in self.parents(n):
                if p not in needed:
                    needed[p] = 1
                    visit.append(p)
                else:
                    # count how many times we'll use this
                    needed[p] += 1

        # sort by revision which is a topological order
        visit = [ (self.rev(n), n) for n in needed.keys() ]
        visit.sort()
        hist = {}

        for r,n in visit:
            curr = decorate(self.read(n), self.linkrev(n))
            for p in self.parents(n):
                if p != nullid:
                    curr = pair(hist[p], curr)
                    # trim the history of unneeded revs
                    needed[p] -= 1
                    if not needed[p]:
                        del hist[p]
            hist[n] = curr

        return zip(hist[n][0], hist[n][1].splitlines(1))
