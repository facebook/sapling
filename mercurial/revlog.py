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
        transaction.add(self.indexfile, n * len(entry))
        self.opener(self.indexfile, "a").write(entry)

        self.cache = (node, n, text)
        return node

    def ancestor(self, a, b):
        def expand(list, map):
            a = []
            while list:
                n = list.pop(0)
                map[n] = 1
                yield n
                for p in self.parents(n):
                    if p != nullid and p not in map:
                        list.append(p)
            yield nullid

        amap = {}
        bmap = {}
        ag = expand([a], amap)
        bg = expand([b], bmap)
        adone = bdone = 0

        while not adone or not bdone:
            if not adone:
                an = ag.next()
                if an == nullid:
                    adone = 1
                elif an in bmap:
                    return an
            if not bdone:
                bn = bg.next()
                if bn == nullid:
                    bdone = 1
                elif bn in amap:
                    return bn

        return nullid

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

    def group(self, linkmap):
        # given a list of changeset revs, return a set of deltas and
        # metadata corresponding to nodes the first delta is
        # parent(nodes[0]) -> nodes[0] the receiver is guaranteed to
        # have this parent as it has all history before these
        # changesets. parent is parent[0]

        revs = []
        needed = {}

        # find file nodes/revs that match changeset revs
        for i in xrange(0, self.count()):
            if self.index[i][3] in linkmap:
                revs.append(i)
                needed[i] = 1

        # if we don't have any revisions touched by these changesets, bail
        if not revs: return struct.pack(">l", 0)

        # add the parent of the first rev
        p = self.parents(self.node(revs[0]))[0]
        revs.insert(0, self.rev(p))

        # for each delta that isn't contiguous in the log, we need to
        # reconstruct the base, reconstruct the result, and then
        # calculate the delta. We also need to do this where we've
        # stored a full version and not a delta
        for i in xrange(0, len(revs) - 1):
            a, b = revs[i], revs[i + 1]
            if a + 1 != b or self.base(b) == b:
                for j in xrange(self.base(a), a + 1):
                    needed[j] = 1
                for j in xrange(self.base(b), b + 1):
                    needed[j] = 1

        # calculate spans to retrieve from datafile
        needed = needed.keys()
        needed.sort()
        spans = []
        for n in needed:
            if n < 0: continue
            o = self.start(n)
            l = self.length(n)
            spans.append((o, l, [(n, l)]))

        # merge spans
        merge = [spans.pop(0)]
        while spans:
            e = spans.pop(0)
            f = merge[-1]
            if e[0] == f[0] + f[1]:
                merge[-1] = (f[0], f[1] + e[1], f[2] + e[2])
            else:
                merge.append(e)

        # read spans in, divide up chunks
        chunks = {}
        for span in merge:
            # we reopen the file for each span to make http happy for now
            f = self.opener(self.datafile)
            f.seek(span[0])
            data = f.read(span[1])

            # divide up the span
            pos = 0
            for r, l in span[2]:
                chunks[r] = data[pos: pos + l]
                pos += l

        # helper to reconstruct intermediate versions
        def construct(text, base, rev):
            for r in range(base + 1, rev + 1):
                b = decompress(chunks[r])
                text = self.patch(text, b)
            return text

        # build deltas
        deltas = []
        for d in range(0, len(revs) - 1):
            a, b = revs[d], revs[d + 1]
            n = self.node(b)
            
            if a + 1 != b or self.base(b) == b:
                if a >= 0:
                    base = self.base(a)
                    ta = decompress(chunks[self.base(a)])
                    ta = construct(ta, base, a)
                else:
                    ta = ""
                    
                base = self.base(b)
                if a > base:
                    base = a
                    tb = ta
                else:
                    tb = decompress(chunks[self.base(b)])
                tb = construct(tb, base, b)
                d = self.diff(ta, tb)
            else:
                d = decompress(chunks[b])

            p = self.parents(n)
            meta = n + p[0] + p[1] + linkmap[self.linkrev(n)]
            l = struct.pack(">l", len(meta) + len(d) + 4)
            deltas.append(l + meta + d)

        l = struct.pack(">l", sum(map(len, deltas)) + 4)
        deltas.insert(0, l)
        return "".join(deltas)
        
    def addgroup(self, data, linkmapper, transaction):
        # given a set of deltas, add them to the revision log. the
        # first delta is against its parent, which should be in our
        # log, the rest are against the previous delta.

        if len(data) <= 4: return

        # retrieve the parent revision of the delta chain
        chain = data[28:48]
        text = self.revision(chain)

        # track the base of the current delta log
        r = self.count()
        t = r - 1
        
        base = prev = -1
        start = end = 0
        if r:
            start = self.start(self.base(t))
            end = self.end(t)
            measure = self.length(self.base(t))
            base = self.base(t)
            prev = self.tip()

        transaction.add(self.datafile, end)
        transaction.add(self.indexfile, r * struct.calcsize(indexformat))
        dfh = self.opener(self.datafile, "a")
        ifh = self.opener(self.indexfile, "a")

        # loop through our set of deltas
        pos = 4
        while pos < len(data):
            l, node, p1, p2, cs = struct.unpack(">l20s20s20s20s",
                                                data[pos:pos+84])
            link = linkmapper(cs)
            delta = data[pos + 84:pos + l]
            pos += l

            # full versions are inserted when the needed deltas become
            # comparable to the uncompressed text or when the previous
            # version is not the one we have a delta against. We use
            # the size of the previous full rev as a proxy for the
            # current size.

            if chain == prev:
                cdelta = compress(delta)

            if chain != prev or (end - start + len(cdelta)) > measure * 2:
                # flush our writes here so we can read it in revision
                dfh.flush()
                ifh.flush()
                text = self.revision(self.node(t))
                text = self.patch(text, delta)
                chk = self.addrevision(text, transaction, link, p1, p2)
                if chk != node:
                    raise "consistency error adding group"
                measure = len(text)
            else:
                e = (end, len(cdelta), self.base(t), link, p1, p2, node)
                self.index.append(e)
                self.nodemap[node] = r
                dfh.write(cdelta)
                ifh.write(struct.pack(indexformat, *e))

            t, r = r, r + 1
            chain = prev
            start = self.start(self.base(t))
            end = self.end(t)

        dfh.close()
        ifh.close()
        return node
