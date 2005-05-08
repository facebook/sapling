# revlog.py - storage back-end for mercurial
#
# This provides efficient delta storage with O(1) retrieve and append
# and O(changes) merge between branches
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import zlib, struct, sha, os, tempfile, binascii
from mercurial import mdiff

def hex(node): return binascii.hexlify(node)
def bin(node): return binascii.unhexlify(node)

def compress(text):
    return zlib.compress(text)

def decompress(bin):
    return zlib.decompress(bin)

def hash(text, p1, p2):
    l = [p1, p2]
    l.sort()
    return sha.sha(l[0] + l[1] + text).digest()

nullid = "\0" * 20
indexformat = ">4l20s20s20s"

class revlog:
    def __init__(self, opener, indexfile, datafile):
        self.indexfile = indexfile
        self.datafile = datafile
        self.index = []
        self.opener = opener
        self.cache = None
        self.nodemap = {nullid: -1}
        # read the whole index for now, handle on-demand later
        try:
            n = 0
            i = self.opener(self.indexfile).read()
            s = struct.calcsize(indexformat)
            for f in range(0, len(i), s):
                # offset, size, base, linkrev, p1, p2, nodeid
                e = struct.unpack(indexformat, i[f:f + s])
                self.nodemap[e[6]] = n
                self.index.append(e)
                n += 1
        except IOError: pass

    def tip(self): return self.node(len(self.index) - 1)
    def count(self): return len(self.index)
    def node(self, rev): return (rev < 0) and nullid or self.index[rev][6]
    def rev(self, node): return self.nodemap[node]
    def linkrev(self, node): return self.index[self.nodemap[node]][3]
    def parents(self, node):
        if node == nullid: return (nullid, nullid)
        return self.index[self.nodemap[node]][4:6]

    def start(self, rev): return self.index[rev][0]
    def length(self, rev): return self.index[rev][1]
    def end(self, rev): return self.start(rev) + self.length(rev)
    def base(self, rev): return self.index[rev][2]

    def lookup(self, id):
        try:
            rev = int(id)
            return self.node(rev)
        except ValueError:
            c = []
            for n in self.nodemap:
                if id in hex(n):
                    c.append(n)
            if len(c) > 1: raise KeyError("Ambiguous identifier")
            if len(c) < 1: raise KeyError
            return c[0]
                
        return None

    def revisions(self, list):
        # this can be optimized to do spans, etc
        # be stupid for now
        for node in list:
            yield self.revision(node)

    def diff(self, a, b):
        return mdiff.textdiff(a, b)

    def patch(self, text, patch):
        return mdiff.patch(text, patch)

    def revision(self, node):
        if node == nullid: return ""
        if self.cache and self.cache[0] == node: return self.cache[2]

        text = None
        rev = self.rev(node)
        base = self.base(rev)
        start = self.start(base)
        end = self.end(rev)

        if self.cache and self.cache[1] >= base and self.cache[1] < rev:
            base = self.cache[1]
            start = self.start(base + 1)
            text = self.cache[2]
            last = 0

        f = self.opener(self.datafile)
        f.seek(start)
        data = f.read(end - start)

        if not text:
            last = self.length(base)
            text = decompress(data[:last])

        for r in range(base + 1, rev + 1):
            s = self.length(r)
            b = decompress(data[last:last + s])
            text = self.patch(text, b)
            last = last + s

        (p1, p2) = self.parents(node)
        if node != hash(text, p1, p2):
            raise "integrity check failed on %s:%d" % (self.datafile, rev)

        self.cache = (node, rev, text)
        return text  

    def addrevision(self, text, transaction, link, p1=None, p2=None):
        if text is None: text = ""
        if p1 is None: p1 = self.tip()
        if p2 is None: p2 = nullid

        node = hash(text, p1, p2)

        n = self.count()
        t = n - 1

        if n:
            start = self.start(self.base(t))
            end = self.end(t)
            prev = self.revision(self.tip())
            data = compress(self.diff(prev, text))

        # full versions are inserted when the needed deltas
        # become comparable to the uncompressed text
        if not n or (end + len(data) - start) > len(text) * 2:
            data = compress(text)
            base = n
        else:
            base = self.base(t)

        offset = 0
        if t >= 0:
            offset = self.end(t)

        e = (offset, len(data), base, link, p1, p2, node)
        
        self.index.append(e)
        self.nodemap[node] = n
        entry = struct.pack(indexformat, *e)

        transaction.add(self.datafile, e[0])
        self.opener(self.datafile, "a").write(data)
        transaction.add(self.indexfile, (n + 1) * len(entry))
        self.opener(self.indexfile, "a").write(entry)

        self.cache = (node, n, text)
        return node

    def ancestor(self, a, b):
        def expand(e1, e2, a1, a2):
            ne = []
            for n in e1:
                (p1, p2) = self.parents(n)
                if p1 in a2: return p1
                if p2 in a2: return p2
                if p1 != nullid and p1 not in a1:
                    a1[p1] = 1
                    ne.append(p1)
                if p2 != nullid and p2 not in a1:
                    a1[p2] = 1
                    ne.append(p2)
            return expand(e2, ne, a2, a1)
        return expand([a], [b], {a:1}, {b:1})

    def mergedag(self, other, transaction, linkseq, accumulate = None):
        """combine the nodes from other's DAG into ours"""
        old = self.tip()
        i = self.count()
        l = []

        # merge the other revision log into our DAG
        for r in range(other.count()):
            id = other.node(r)
            if id not in self.nodemap:
                (xn, yn) = other.parents(id)
                l.append((id, xn, yn))
                self.nodemap[id] = i
                i += 1

        # merge node date for new nodes
        r = other.revisions([e[0] for e in l])
        for e in l:
            t = r.next()
            if accumulate: accumulate(t)
            self.addrevision(t, transaction, linkseq.next(), e[1], e[2])

        # return the unmerged heads for later resolving
        return (old, self.tip())
