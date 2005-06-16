# revlog.py - storage back-end for mercurial
#
# This provides efficient delta storage with O(1) retrieve and append
# and O(changes) merge between branches
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import zlib, struct, sha, binascii, heapq
from mercurial import mdiff

def hex(node): return binascii.hexlify(node)
def bin(node): return binascii.unhexlify(node)
def short(node): return hex(node[:6])

def compress(text):
    if not text: return text
    if len(text) < 44:
        if text[0] == '\0': return text
        return 'u' + text
    bin = zlib.compress(text)
    if len(bin) > len(text):
        if text[0] == '\0': return text
        return 'u' + text
    return bin

def decompress(bin):
    if not bin: return bin
    t = bin[0]
    if t == '\0': return bin
    if t == 'x': return zlib.decompress(bin)
    if t == 'u': return bin[1:]
    raise "unknown compression type %s" % t

def hash(text, p1, p2):
    l = [p1, p2]
    l.sort()
    return sha.sha(l[0] + l[1] + text).digest()

nullid = "\0" * 20
indexformat = ">4l20s20s20s"

class lazyparser:
    def __init__(self, data, revlog):
        self.data = data
        self.s = struct.calcsize(indexformat)
        self.l = len(data)/self.s
        self.index = [None] * self.l
        self.map = {nullid: -1}
        self.all = 0
        self.revlog = revlog

    def load(self, pos=None):
        if self.all: return
        if pos is not None:
            block = pos / 1000
            i = block * 1000
            end = min(self.l, i + 1000)
        else:
            self.all = 1
            i = 0
            end = self.l
            self.revlog.index = self.index
            self.revlog.nodemap = self.map
            
        while i < end:
            d = self.data[i * self.s: (i + 1) * self.s]
            e = struct.unpack(indexformat, d)
            self.index[i] = e
            self.map[e[6]] = i
            i += 1
        
class lazyindex:
    def __init__(self, parser):
        self.p = parser
    def __len__(self):
        return len(self.p.index)
    def load(self, pos):
        self.p.load(pos)
        return self.p.index[pos]
    def __getitem__(self, pos):
        return self.p.index[pos] or self.load(pos)
    def append(self, e):
        self.p.index.append(e)
        
class lazymap:
    def __init__(self, parser):
        self.p = parser
    def load(self, key):
        if self.p.all: return
        n = self.p.data.find(key)
        if n < 0: raise KeyError("node " + hex(key))
        pos = n / self.p.s
        self.p.load(pos)
    def __contains__(self, key):
        self.p.load()
        return key in self.p.map
    def __iter__(self):
        for i in xrange(self.p.l):
            try:
                yield self.p.index[i][6]
            except:
                self.p.load(i)
                yield self.p.index[i][6]
    def __getitem__(self, key):
        try:
            return self.p.map[key]
        except KeyError:
            try:
                self.load(key)
                return self.p.map[key]
            except KeyError:
                raise KeyError("node " + hex(key))
    def __setitem__(self, key, val):
        self.p.map[key] = val

class revlog:
    def __init__(self, opener, indexfile, datafile):
        self.indexfile = indexfile
        self.datafile = datafile
        self.opener = opener
        self.cache = None

        try:
            i = self.opener(self.indexfile).read()
        except IOError:
            i = ""

        if len(i) > 10000:
            # big index, let's parse it on demand
            parser = lazyparser(i, self)
            self.index = lazyindex(parser)
            self.nodemap = lazymap(parser)
        else:
            s = struct.calcsize(indexformat)
            l = len(i) / s
            self.index = [None] * l
            m = [None] * l

            n = 0
            for f in xrange(0, len(i), s):
                # offset, size, base, linkrev, p1, p2, nodeid
                e = struct.unpack(indexformat, i[f:f + s])
                m[n] = (e[6], n)
                self.index[n] = e
                n += 1

            self.nodemap = dict(m)
            self.nodemap[nullid] = -1
            

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

    def heads(self):
        p = {}
        h = []
        for r in range(self.count() - 1, -1, -1):
            n = self.node(r)
            if n not in p:
                h.append(n)
            for pn in self.parents(n):
                p[pn] = 1
        return h

    def children(self, node):
        c = []
        p = self.rev(node)
        for r in range(p + 1, self.count()):
            n = self.node(r)
            for pn in self.parents(n):
                if pn == p:
                    c.append(p)
                    continue
                elif pn == nullid:
                    continue
        return c
    
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
            if len(c) < 1: raise KeyError("No match found")
            return c[0]
                
        return None

    def diff(self, a, b):
        return mdiff.textdiff(a, b)

    def patches(self, t, pl):
        return mdiff.patches(t, pl)

    def delta(self, node):
        r = self.rev(node)
        b = self.base(r)
        if r == b:
            return self.diff(self.revision(self.node(r - 1)),
                             self.revision(node))
        else:
            f = self.opener(self.datafile)
            f.seek(self.start(r))
            data = f.read(self.length(r))
        return decompress(data)

    def revision(self, node):
        if node == nullid: return ""
        if self.cache and self.cache[0] == node: return self.cache[2]

        text = None
        rev = self.rev(node)
        start, length, base, link, p1, p2, node = self.index[rev]
        end = start + length
        if base != rev: start = self.start(base)

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

        bins = []
        for r in xrange(base + 1, rev + 1):
            s = self.length(r)
            bins.append(decompress(data[last:last + s]))
            last = last + s

        text = mdiff.patches(text, bins)

        if node != hash(text, p1, p2):
            raise IOError("integrity check failed on %s:%d"
                          % (self.datafile, rev))

        self.cache = (node, rev, text)
        return text  

    def addrevision(self, text, transaction, link, p1=None, p2=None):
        if text is None: text = ""
        if p1 is None: p1 = self.tip()
        if p2 is None: p2 = nullid

        node = hash(text, p1, p2)

        if node in self.nodemap:
            return node

        n = self.count()
        t = n - 1

        if n:
            base = self.base(t)
            start = self.start(base)
            end = self.end(t)
            prev = self.revision(self.tip())
            d = self.diff(prev, text)
            data = compress(d)
            dist = end - start + len(data)

        # full versions are inserted when the needed deltas
        # become comparable to the uncompressed text
        if not n or dist > len(text) * 2:
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
        # calculate the distance of every node from root
        dist = {nullid: 0}
        for i in xrange(self.count()):
            n = self.node(i)
            p1, p2 = self.parents(n)
            dist[n] = max(dist[p1], dist[p2]) + 1
        
        # traverse ancestors in order of decreasing distance from root
        def ancestors(node):
            # we store negative distances because heap returns smallest member
            h = [(-dist[node], node)]
            seen = {}
            earliest = self.count()
            while h:
                d, n = heapq.heappop(h)
                if n not in seen:
                    seen[n] = 1
                    r = self.rev(n)
                    yield (-d, r, n)
                    for p in self.parents(n):
                        heapq.heappush(h, (-dist[p], p))

        x = ancestors(a)
        y = ancestors(b)
        lx = x.next()
        ly = y.next()

        # increment each ancestor list until it is closer to root than
        # the other, or they match
        while 1:
            if lx == ly:
                return lx[2]
            elif lx < ly:
                ly = y.next()
            elif lx > ly:
                lx = x.next()

    def group(self, linkmap):
        # given a list of changeset revs, return a set of deltas and
        # metadata corresponding to nodes. the first delta is
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
        if not revs:
            yield struct.pack(">l", 0)
            return

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
        oo = -1
        ol = 0
        for n in needed:
            if n < 0: continue
            o = self.start(n)
            l = self.length(n)
            if oo + ol == o: # can we merge with the previous?
                nl = spans[-1][2]
                nl.append((n, l))
                ol += l
                spans[-1] = (oo, ol, nl)
            else:
                oo = o
                ol = l
                spans.append((oo, ol, [(n, l)]))

        # read spans in, divide up chunks
        chunks = {}
        for span in spans:
            # we reopen the file for each span to make http happy for now
            f = self.opener(self.datafile)
            f.seek(span[0])
            data = f.read(span[1])

            # divide up the span
            pos = 0
            for r, l in span[2]:
                chunks[r] = decompress(data[pos: pos + l])
                pos += l

        # helper to reconstruct intermediate versions
        def construct(text, base, rev):
            bins = [chunks[r] for r in xrange(base + 1, rev + 1)]
            return mdiff.patches(text, bins)

        # build deltas
        deltas = []
        for d in xrange(0, len(revs) - 1):
            a, b = revs[d], revs[d + 1]
            n = self.node(b)

            # do we need to construct a new delta?
            if a + 1 != b or self.base(b) == b:
                if a >= 0:
                    base = self.base(a)
                    ta = chunks[self.base(a)]
                    ta = construct(ta, base, a)
                else:
                    ta = ""
                    
                base = self.base(b)
                if a > base:
                    base = a
                    tb = ta
                else:
                    tb = chunks[self.base(b)]
                tb = construct(tb, base, b)
                d = self.diff(ta, tb)
            else:
                d = chunks[b]

            p = self.parents(n)
            meta = n + p[0] + p[1] + linkmap[self.linkrev(n)]
            l = struct.pack(">l", len(meta) + len(d) + 4)
            yield l
            yield meta
            yield d

        yield struct.pack(">l", 0)

    def addgroup(self, revs, linkmapper, transaction, unique = 0):
        # given a set of deltas, add them to the revision log. the
        # first delta is against its parent, which should be in our
        # log, the rest are against the previous delta.

        # track the base of the current delta log
        r = self.count()
        t = r - 1
        node = nullid
        
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
        chain = None
        for chunk in revs:
            node, p1, p2, cs = struct.unpack("20s20s20s20s", chunk[:80])
            link = linkmapper(cs)
            if node in self.nodemap:
                # this can happen if two branches make the same change
                if unique:
                    raise "already have %s" % hex(node[:4])
                continue
            delta = chunk[80:]

            if not chain:
                # retrieve the parent revision of the delta chain
                chain = p1
                if not chain in self.nodemap:
                    raise "unknown base %s" % short(chain[:4])

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
                text = self.revision(chain)
                text = self.patches(text, [delta])
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

            t, r, chain, prev = r, r + 1, node, node
            start = self.start(self.base(t))
            end = self.end(t)

        dfh.close()
        ifh.close()
        return node
