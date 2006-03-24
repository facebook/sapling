"""
revlog.py - storage back-end for mercurial

This provides efficient delta storage with O(1) retrieve and append
and O(changes) merge between branches

Copyright 2005 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

from node import *
from i18n import gettext as _
from demandload import demandload
demandload(globals(), "binascii changegroup errno heapq mdiff os")
demandload(globals(), "sha struct zlib")

def hash(text, p1, p2):
    """generate a hash from the given text and its parent hashes

    This hash combines both the current file contents and its history
    in a manner that makes it easy to distinguish nodes with the same
    content in the revision graph.
    """
    l = [p1, p2]
    l.sort()
    s = sha.new(l[0])
    s.update(l[1])
    s.update(text)
    return s.digest()

def compress(text):
    """ generate a possibly-compressed representation of text """
    if not text: return ("", text)
    if len(text) < 44:
        if text[0] == '\0': return ("", text)
        return ('u', text)
    bin = zlib.compress(text)
    if len(bin) > len(text):
        if text[0] == '\0': return ("", text)
        return ('u', text)
    return ("", bin)

def decompress(bin):
    """ decompress the given input """
    if not bin: return bin
    t = bin[0]
    if t == '\0': return bin
    if t == 'x': return zlib.decompress(bin)
    if t == 'u': return bin[1:]
    raise RevlogError(_("unknown compression type %r") % t)

indexformat = ">4l20s20s20s"

class lazyparser(object):
    """
    this class avoids the need to parse the entirety of large indices

    By default we parse and load 1000 entries at a time.

    If no position is specified, we load the whole index, and replace
    the lazy objects in revlog with the underlying objects for
    efficiency in cases where we look at most of the nodes.
    """
    def __init__(self, data, revlog):
        self.data = data
        self.s = struct.calcsize(indexformat)
        self.l = len(data)/self.s
        self.index = [None] * self.l
        self.map = {nullid: -1}
        self.all = 0
        self.revlog = revlog

    def trunc(self, pos):
        self.l = pos/self.s

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

class lazyindex(object):
    """a lazy version of the index array"""
    def __init__(self, parser):
        self.p = parser
    def __len__(self):
        return len(self.p.index)
    def load(self, pos):
        if pos < 0:
            pos += len(self.p.index)
        self.p.load(pos)
        return self.p.index[pos]
    def __getitem__(self, pos):
        return self.p.index[pos] or self.load(pos)
    def __delitem__(self, pos):
        del self.p.index[pos]
    def append(self, e):
        self.p.index.append(e)
    def trunc(self, pos):
        self.p.trunc(pos)

class lazymap(object):
    """a lazy version of the node map"""
    def __init__(self, parser):
        self.p = parser
    def load(self, key):
        if self.p.all: return
        n = self.p.data.find(key)
        if n < 0:
            raise KeyError(key)
        pos = n / self.p.s
        self.p.load(pos)
    def __contains__(self, key):
        self.p.load()
        return key in self.p.map
    def __iter__(self):
        yield nullid
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
    def __delitem__(self, key):
        del self.p.map[key]

class RevlogError(Exception): pass

class revlog(object):
    """
    the underlying revision storage object

    A revlog consists of two parts, an index and the revision data.

    The index is a file with a fixed record size containing
    information on each revision, includings its nodeid (hash), the
    nodeids of its parents, the position and offset of its data within
    the data file, and the revision it's based on. Finally, each entry
    contains a linkrev entry that can serve as a pointer to external
    data.

    The revision data itself is a linear collection of data chunks.
    Each chunk represents a revision and is usually represented as a
    delta against the previous chunk. To bound lookup time, runs of
    deltas are limited to about 2 times the length of the original
    version data. This makes retrieval of a version proportional to
    its size, or O(1) relative to the number of revisions.

    Both pieces of the revlog are written to in an append-only
    fashion, which means we never need to rewrite a file to insert or
    remove data, and can use some simple techniques to avoid the need
    for locking while reading.
    """
    def __init__(self, opener, indexfile, datafile):
        """
        create a revlog object

        opener is a function that abstracts the file opening operation
        and can be used to implement COW semantics or the like.
        """
        self.indexfile = indexfile
        self.datafile = datafile
        self.opener = opener

        self.indexstat = None
        self.cache = None
        self.chunkcache = None
        self.load()

    def load(self):
        try:
            f = self.opener(self.indexfile)
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
            i = ""
        else:
            try:
                st = os.fstat(f.fileno())
            except AttributeError, inst:
                st = None
            else:
                oldst = self.indexstat
                if (oldst and st.st_dev == oldst.st_dev
                    and st.st_ino == oldst.st_ino
                    and st.st_mtime == oldst.st_mtime
                    and st.st_ctime == oldst.st_ctime):
                    return
            self.indexstat = st
            i = f.read()

        if i and i[:4] != "\0\0\0\0":
            raise RevlogError(_("incompatible revlog signature on %s") %
                              self.indexfile)

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
            for f in xrange(0, l * s, s):
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
    def rev(self, node):
        try:
            return self.nodemap[node]
        except KeyError:
            raise RevlogError(_('%s: no node %s') % (self.indexfile, hex(node)))
    def linkrev(self, node): return self.index[self.rev(node)][3]
    def parents(self, node):
        if node == nullid: return (nullid, nullid)
        return self.index[self.rev(node)][4:6]

    def start(self, rev): return (rev < 0) and -1 or self.index[rev][0]
    def length(self, rev):
        if rev < 0:
            return 0
        else:
            return self.index[rev][1]
    def end(self, rev): return self.start(rev) + self.length(rev)
    def base(self, rev): return (rev < 0) and rev or self.index[rev][2]

    def reachable(self, rev, stop=None):
        reachable = {}
        visit = [rev]
        reachable[rev] = 1
        if stop:
            stopn = self.rev(stop)
        else:
            stopn = 0
        while visit:
            n = visit.pop(0)
            if n == stop:
                continue
            if n == nullid:
                continue
            for p in self.parents(n):
                if self.rev(p) < stopn:
                    continue
                if p not in reachable:
                    reachable[p] = 1
                    visit.append(p)
        return reachable

    def nodesbetween(self, roots=None, heads=None):
        """Return a tuple containing three elements. Elements 1 and 2 contain
        a final list bases and heads after all the unreachable ones have been
        pruned.  Element 0 contains a topologically sorted list of all

        nodes that satisfy these constraints:
        1. All nodes must be descended from a node in roots (the nodes on
           roots are considered descended from themselves).
        2. All nodes must also be ancestors of a node in heads (the nodes in
           heads are considered to be their own ancestors).

        If roots is unspecified, nullid is assumed as the only root.
        If heads is unspecified, it is taken to be the output of the
        heads method (i.e. a list of all nodes in the repository that
        have no children)."""
        nonodes = ([], [], [])
        if roots is not None:
            roots = list(roots)
            if not roots:
                return nonodes
            lowestrev = min([self.rev(n) for n in roots])
        else:
            roots = [nullid] # Everybody's a descendent of nullid
            lowestrev = -1
        if (lowestrev == -1) and (heads is None):
            # We want _all_ the nodes!
            return ([self.node(r) for r in xrange(0, self.count())],
                    [nullid], list(self.heads()))
        if heads is None:
            # All nodes are ancestors, so the latest ancestor is the last
            # node.
            highestrev = self.count() - 1
            # Set ancestors to None to signal that every node is an ancestor.
            ancestors = None
            # Set heads to an empty dictionary for later discovery of heads
            heads = {}
        else:
            heads = list(heads)
            if not heads:
                return nonodes
            ancestors = {}
            # Start at the top and keep marking parents until we're done.
            nodestotag = heads[:]
            # Turn heads into a dictionary so we can remove 'fake' heads.
            # Also, later we will be using it to filter out the heads we can't
            # find from roots.
            heads = dict.fromkeys(heads, 0)
            # Remember where the top was so we can use it as a limit later.
            highestrev = max([self.rev(n) for n in nodestotag])
            while nodestotag:
                # grab a node to tag
                n = nodestotag.pop()
                # Never tag nullid
                if n == nullid:
                    continue
                # A node's revision number represents its place in a
                # topologically sorted list of nodes.
                r = self.rev(n)
                if r >= lowestrev:
                    if n not in ancestors:
                        # If we are possibly a descendent of one of the roots
                        # and we haven't already been marked as an ancestor
                        ancestors[n] = 1 # Mark as ancestor
                        # Add non-nullid parents to list of nodes to tag.
                        nodestotag.extend([p for p in self.parents(n) if
                                           p != nullid])
                    elif n in heads: # We've seen it before, is it a fake head?
                        # So it is, real heads should not be the ancestors of
                        # any other heads.
                        heads.pop(n)
            if not ancestors:
                return nonodes
            # Now that we have our set of ancestors, we want to remove any
            # roots that are not ancestors.

            # If one of the roots was nullid, everything is included anyway.
            if lowestrev > -1:
                # But, since we weren't, let's recompute the lowest rev to not
                # include roots that aren't ancestors.

                # Filter out roots that aren't ancestors of heads
                roots = [n for n in roots if n in ancestors]
                # Recompute the lowest revision
                if roots:
                    lowestrev = min([self.rev(n) for n in roots])
                else:
                    # No more roots?  Return empty list
                    return nonodes
            else:
                # We are descending from nullid, and don't need to care about
                # any other roots.
                lowestrev = -1
                roots = [nullid]
        # Transform our roots list into a 'set' (i.e. a dictionary where the
        # values don't matter.
        descendents = dict.fromkeys(roots, 1)
        # Also, keep the original roots so we can filter out roots that aren't
        # 'real' roots (i.e. are descended from other roots).
        roots = descendents.copy()
        # Our topologically sorted list of output nodes.
        orderedout = []
        # Don't start at nullid since we don't want nullid in our output list,
        # and if nullid shows up in descedents, empty parents will look like
        # they're descendents.
        for r in xrange(max(lowestrev, 0), highestrev + 1):
            n = self.node(r)
            isdescendent = False
            if lowestrev == -1:  # Everybody is a descendent of nullid
                isdescendent = True
            elif n in descendents:
                # n is already a descendent
                isdescendent = True
                # This check only needs to be done here because all the roots
                # will start being marked is descendents before the loop.
                if n in roots:
                    # If n was a root, check if it's a 'real' root.
                    p = tuple(self.parents(n))
                    # If any of its parents are descendents, it's not a root.
                    if (p[0] in descendents) or (p[1] in descendents):
                        roots.pop(n)
            else:
                p = tuple(self.parents(n))
                # A node is a descendent if either of its parents are
                # descendents.  (We seeded the dependents list with the roots
                # up there, remember?)
                if (p[0] in descendents) or (p[1] in descendents):
                    descendents[n] = 1
                    isdescendent = True
            if isdescendent and ((ancestors is None) or (n in ancestors)):
                # Only include nodes that are both descendents and ancestors.
                orderedout.append(n)
                if (ancestors is not None) and (n in heads):
                    # We're trying to figure out which heads are reachable
                    # from roots.
                    # Mark this head as having been reached
                    heads[n] = 1
                elif ancestors is None:
                    # Otherwise, we're trying to discover the heads.
                    # Assume this is a head because if it isn't, the next step
                    # will eventually remove it.
                    heads[n] = 1
                    # But, obviously its parents aren't.
                    for p in self.parents(n):
                        heads.pop(p, None)
        heads = [n for n in heads.iterkeys() if heads[n] != 0]
        roots = roots.keys()
        assert orderedout
        assert roots
        assert heads
        return (orderedout, roots, heads)

    def heads(self, start=None):
        """return the list of all nodes that have no children

        if start is specified, only heads that are descendants of
        start will be returned

        """
        if start is None:
            start = nullid
        reachable = {start: 1}
        heads = {start: 1}
        startrev = self.rev(start)

        for r in xrange(startrev + 1, self.count()):
            n = self.node(r)
            for pn in self.parents(n):
                if pn in reachable:
                    reachable[n] = 1
                    heads[n] = 1
                if pn in heads:
                    del heads[pn]
        return heads.keys()

    def children(self, node):
        """find the children of a given node"""
        c = []
        p = self.rev(node)
        for r in range(p + 1, self.count()):
            n = self.node(r)
            for pn in self.parents(n):
                if pn == node:
                    c.append(n)
                    continue
                elif pn == nullid:
                    continue
        return c

    def lookup(self, id):
        """locate a node based on revision number or subset of hex nodeid"""
        try:
            rev = int(id)
            if str(rev) != id: raise ValueError
            if rev < 0: rev = self.count() + rev
            if rev < 0 or rev >= self.count(): raise ValueError
            return self.node(rev)
        except (ValueError, OverflowError):
            c = []
            for n in self.nodemap:
                if hex(n).startswith(id):
                    c.append(n)
            if len(c) > 1: raise RevlogError(_("Ambiguous identifier"))
            if len(c) < 1: raise RevlogError(_("No match found"))
            return c[0]

        return None

    def diff(self, a, b):
        """return a delta between two revisions"""
        return mdiff.textdiff(a, b)

    def patches(self, t, pl):
        """apply a list of patches to a string"""
        return mdiff.patches(t, pl)

    def chunk(self, rev):
        start, length = self.start(rev), self.length(rev)
        end = start + length

        def loadcache():
            cache_length = max(4096 * 1024, length) # 4Mo
            df = self.opener(self.datafile)
            df.seek(start)
            self.chunkcache = (start, df.read(cache_length))

        if not self.chunkcache:
            loadcache()

        cache_start = self.chunkcache[0]
        cache_end = cache_start + len(self.chunkcache[1])
        if start >= cache_start and end <= cache_end:
            # it is cached
            offset = start - cache_start
        else:
            loadcache()
            offset = 0

        #def checkchunk():
        #    df = self.opener(self.datafile)
        #    df.seek(start)
        #    return df.read(length)
        #assert s == checkchunk()
        return decompress(self.chunkcache[1][offset:offset + length])

    def delta(self, node):
        """return or calculate a delta between a node and its predecessor"""
        r = self.rev(node)
        return self.revdiff(r - 1, r)

    def revdiff(self, rev1, rev2):
        """return or calculate a delta between two revisions"""
        b1 = self.base(rev1)
        b2 = self.base(rev2)
        if b1 == b2 and rev1 + 1 == rev2:
            return self.chunk(rev2)
        else:
            return self.diff(self.revision(self.node(rev1)),
                             self.revision(self.node(rev2)))

    def revision(self, node):
        """return an uncompressed revision of a given"""
        if node == nullid: return ""
        if self.cache and self.cache[0] == node: return self.cache[2]

        # look up what we need to read
        text = None
        rev = self.rev(node)
        base = self.base(rev)

        # do we have useful data cached?
        if self.cache and self.cache[1] >= base and self.cache[1] < rev:
            base = self.cache[1]
            text = self.cache[2]
        else:
            text = self.chunk(base)

        bins = []
        for r in xrange(base + 1, rev + 1):
            bins.append(self.chunk(r))

        text = self.patches(text, bins)

        p1, p2 = self.parents(node)
        if node != hash(text, p1, p2):
            raise RevlogError(_("integrity check failed on %s:%d")
                          % (self.datafile, rev))

        self.cache = (node, rev, text)
        return text

    def addrevision(self, text, transaction, link, p1=None, p2=None, d=None):
        """add a revision to the log

        text - the revision data to add
        transaction - the transaction object used for rollback
        link - the linkrev data to add
        p1, p2 - the parent nodeids of the revision
        d - an optional precomputed delta
        """
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
            if not d:
                prev = self.revision(self.tip())
                d = self.diff(prev, str(text))
            data = compress(d)
            l = len(data[1]) + len(data[0])
            dist = end - start + l

        # full versions are inserted when the needed deltas
        # become comparable to the uncompressed text
        if not n or dist > len(text) * 2:
            data = compress(text)
            l = len(data[1]) + len(data[0])
            base = n
        else:
            base = self.base(t)

        offset = 0
        if t >= 0:
            offset = self.end(t)

        e = (offset, l, base, link, p1, p2, node)

        self.index.append(e)
        self.nodemap[node] = n
        entry = struct.pack(indexformat, *e)

        transaction.add(self.datafile, e[0])
        f = self.opener(self.datafile, "a")
        if data[0]:
            f.write(data[0])
        f.write(data[1])
        transaction.add(self.indexfile, n * len(entry))
        self.opener(self.indexfile, "a").write(entry)

        self.cache = (node, n, text)
        return node

    def ancestor(self, a, b):
        """calculate the least common ancestor of nodes a and b"""
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
            while h:
                d, n = heapq.heappop(h)
                if n not in seen:
                    seen[n] = 1
                    yield (-d, n)
                    for p in self.parents(n):
                        heapq.heappush(h, (-dist[p], p))

        def generations(node):
            sg, s = None, {}
            for g,n in ancestors(node):
                if g != sg:
                    if sg:
                        yield sg, s
                    sg, s = g, {n:1}
                else:
                    s[n] = 1
            yield sg, s

        x = generations(a)
        y = generations(b)
        gx = x.next()
        gy = y.next()

        # increment each ancestor list until it is closer to root than
        # the other, or they match
        while 1:
            #print "ancestor gen %s %s" % (gx[0], gy[0])
            if gx[0] == gy[0]:
                # find the intersection
                i = [ n for n in gx[1] if n in gy[1] ]
                if i:
                    return i[0]
                else:
                    #print "next"
                    gy = y.next()
                    gx = x.next()
            elif gx[0] < gy[0]:
                #print "next y"
                gy = y.next()
            else:
                #print "next x"
                gx = x.next()

    def group(self, nodelist, lookup, infocollect=None):
        """calculate a delta group

        Given a list of changeset revs, return a set of deltas and
        metadata corresponding to nodes. the first delta is
        parent(nodes[0]) -> nodes[0] the receiver is guaranteed to
        have this parent as it has all history before these
        changesets. parent is parent[0]
        """
        revs = [self.rev(n) for n in nodelist]

        # if we don't have any revisions touched by these changesets, bail
        if not revs:
            yield changegroup.closechunk()
            return

        # add the parent of the first rev
        p = self.parents(self.node(revs[0]))[0]
        revs.insert(0, self.rev(p))

        # build deltas
        for d in xrange(0, len(revs) - 1):
            a, b = revs[d], revs[d + 1]
            nb = self.node(b)

            if infocollect is not None:
                infocollect(nb)

            d = self.revdiff(a, b)
            p = self.parents(nb)
            meta = nb + p[0] + p[1] + lookup(nb)
            yield changegroup.genchunk("%s%s" % (meta, d))

        yield changegroup.closechunk()

    def addgroup(self, revs, linkmapper, transaction, unique=0):
        """
        add a delta group

        given a set of deltas, add them to the revision log. the
        first delta is against its parent, which should be in our
        log, the rest are against the previous delta.
        """

        #track the base of the current delta log
        r = self.count()
        t = r - 1
        node = None

        base = prev = -1
        start = end = measure = 0
        if r:
            base = self.base(t)
            start = self.start(base)
            end = self.end(t)
            measure = self.length(base)
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
                # if unique:
                #    raise RevlogError(_("already have %s") % hex(node[:4]))
                chain = node
                continue
            delta = chunk[80:]

            for p in (p1, p2):
                if not p in self.nodemap:
                    raise RevlogError(_("unknown parent %s") % short(p1))

            if not chain:
                # retrieve the parent revision of the delta chain
                chain = p1
                if not chain in self.nodemap:
                    raise RevlogError(_("unknown base %s") % short(chain[:4]))

            # full versions are inserted when the needed deltas become
            # comparable to the uncompressed text or when the previous
            # version is not the one we have a delta against. We use
            # the size of the previous full rev as a proxy for the
            # current size.

            if chain == prev:
                tempd = compress(delta)
                cdelta = tempd[0] + tempd[1]

            if chain != prev or (end - start + len(cdelta)) > measure * 2:
                # flush our writes here so we can read it in revision
                dfh.flush()
                ifh.flush()
                text = self.revision(chain)
                text = self.patches(text, [delta])
                chk = self.addrevision(text, transaction, link, p1, p2)
                if chk != node:
                    raise RevlogError(_("consistency error adding group"))
                measure = len(text)
            else:
                e = (end, len(cdelta), base, link, p1, p2, node)
                self.index.append(e)
                self.nodemap[node] = r
                dfh.write(cdelta)
                ifh.write(struct.pack(indexformat, *e))

            t, r, chain, prev = r, r + 1, node, node
            base = self.base(t)
            start = self.start(base)
            end = self.end(t)

        dfh.close()
        ifh.close()
        if node is None:
            raise RevlogError(_("group to be added is empty"))
        return node

    def strip(self, rev, minlink):
        if self.count() == 0 or rev >= self.count():
            return

        # When stripping away a revision, we need to make sure it
        # does not actually belong to an older changeset.
        # The minlink parameter defines the oldest revision
        # we're allowed to strip away.
        while minlink > self.index[rev][3]:
            rev += 1
            if rev >= self.count():
                return

        # first truncate the files on disk
        end = self.start(rev)
        self.opener(self.datafile, "a").truncate(end)
        end = rev * struct.calcsize(indexformat)
        self.opener(self.indexfile, "a").truncate(end)

        # then reset internal state in memory to forget those revisions
        self.cache = None
        self.chunkcache = None
        for p in self.index[rev:]:
            del self.nodemap[p[6]]
        del self.index[rev:]

        # truncating the lazyindex also truncates the lazymap.
        if isinstance(self.index, lazyindex):
            self.index.trunc(end)


    def checksize(self):
        expected = 0
        if self.count():
            expected = self.end(self.count() - 1)

        try:
            f = self.opener(self.datafile)
            f.seek(0, 2)
            actual = f.tell()
            dd = actual - expected
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
            dd = 0

        try:
            f = self.opener(self.indexfile)
            f.seek(0, 2)
            actual = f.tell()
            s = struct.calcsize(indexformat)
            i = actual / s
            di = actual - (i * s)
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
            di = 0

        return (dd, di)


