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
demandload(globals(), "sha struct util zlib")

# revlog version strings
REVLOGV0 = 0
REVLOGNG = 1

# revlog flags
REVLOGNGINLINEDATA = (1 << 16)

def flagstr(flag):
    if flag == "inline":
        return REVLOGNGINLINEDATA
    raise RevlogError(_("unknown revlog flag %s" % flag))

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

indexformatv0 = ">4l20s20s20s"
v0shaoffset = 56
# index ng:
# 6 bytes offset
# 2 bytes flags
# 4 bytes compressed length
# 4 bytes uncompressed length
# 4 bytes: base rev
# 4 bytes link rev
# 4 bytes parent 1 rev
# 4 bytes parent 2 rev
# 32 bytes: nodeid
indexformatng = ">Qiiiiii20s12x"
ngshaoffset = 32
versionformat = ">i"

class lazyparser(object):
    """
    this class avoids the need to parse the entirety of large indices
    """
    def __init__(self, dataf, size, indexformat, shaoffset):
        self.dataf = dataf
        self.format = indexformat
        self.s = struct.calcsize(indexformat)
        self.indexformat = indexformat
        self.datasize = size
        self.l = size/self.s
        self.index = [None] * self.l
        self.map = {nullid: -1}
        self.allmap = 0
        self.all = 0
        self.mapfind_count = 0
        self.shaoffset = shaoffset

    def loadmap(self):
        """
        during a commit, we need to make sure the rev being added is
        not a duplicate.  This requires loading the entire index,
        which is fairly slow.  loadmap can load up just the node map,
        which takes much less time.
        """
        if self.allmap: return
        start = 0
        end = self.datasize
        self.allmap = 1
        cur = 0
        count = 0
        blocksize = self.s * 256
        self.dataf.seek(0)
        while cur < end:
            data = self.dataf.read(blocksize)
            off = 0
            for x in xrange(256):
                n = data[off + self.shaoffset:off + self.shaoffset + 20]
                self.map[n] = count
                count += 1
                if count >= self.l:
                    break
                off += self.s
            cur += blocksize

    def loadblock(self, blockstart, blocksize, data=None):
        if self.all: return
        if data is None:
            self.dataf.seek(blockstart)
            data = self.dataf.read(blocksize)
        lend = len(data) / self.s
        i = blockstart / self.s
        off = 0
        for x in xrange(lend):
            if self.index[i + x] == None:
                b = data[off : off + self.s]
                self.index[i + x] = b
                n = b[self.shaoffset:self.shaoffset + 20]
                self.map[n] = i + x
            off += self.s

    def findnode(self, node):
        """search backwards through the index file for a specific node"""
        if self.allmap: return None

        # hg log will cause many many searches for the manifest
        # nodes.  After we get called a few times, just load the whole
        # thing.
        if self.mapfind_count > 8:
            self.loadmap()
            if node in self.map:
                return node
            return None
        self.mapfind_count += 1
        last = self.l - 1
        while self.index[last] != None:
            if last == 0:
                self.all = 1
                self.allmap = 1
                return None
            last -= 1
        end = (last + 1) * self.s
        blocksize = self.s * 256
        while end >= 0:
            start = max(end - blocksize, 0)
            self.dataf.seek(start)
            data = self.dataf.read(end - start)
            findend = end - start
            while True:
                # we're searching backwards, so weh have to make sure
                # we don't find a changeset where this node is a parent
                off = data.rfind(node, 0, findend)
                findend = off
                if off >= 0:
                    i = off / self.s
                    off = i * self.s
                    n = data[off + self.shaoffset:off + self.shaoffset + 20]
                    if n == node:
                        self.map[n] = i + start / self.s
                        return node
                else:
                    break
            end -= blocksize
        return None

    def loadindex(self, i=None, end=None):
        if self.all: return
        all = False
        if i == None:
            blockstart = 0
            blocksize = (512 / self.s) * self.s
            end = self.datasize
            all = True
        else:
            if end:
                blockstart = i * self.s
                end = end * self.s
                blocksize = end - blockstart
            else:
                blockstart = (i & ~(32)) * self.s
                blocksize = self.s * 64
                end = blockstart + blocksize
        while blockstart < end:
            self.loadblock(blockstart, blocksize)
            blockstart += blocksize
        if all: self.all = True

class lazyindex(object):
    """a lazy version of the index array"""
    def __init__(self, parser):
        self.p = parser
    def __len__(self):
        return len(self.p.index)
    def load(self, pos):
        if pos < 0:
            pos += len(self.p.index)
        self.p.loadindex(pos)
        return self.p.index[pos]
    def __getitem__(self, pos):
        ret = self.p.index[pos] or self.load(pos)
        if isinstance(ret, str):
            ret = struct.unpack(self.p.indexformat, ret)
        return ret
    def __setitem__(self, pos, item):
        self.p.index[pos] = item
    def __delitem__(self, pos):
        del self.p.index[pos]
    def append(self, e):
        self.p.index.append(e)

class lazymap(object):
    """a lazy version of the node map"""
    def __init__(self, parser):
        self.p = parser
    def load(self, key):
        n = self.p.findnode(key)
        if n == None:
            raise KeyError(key)
    def __contains__(self, key):
        if key in self.p.map:
            return True
        self.p.loadmap()
        return key in self.p.map
    def __iter__(self):
        yield nullid
        for i in xrange(self.p.l):
            ret = self.p.index[i]
            if not ret:
                self.p.loadindex(i)
                ret = self.p.index[i]
            if isinstance(ret, str):
                ret = struct.unpack(self.p.indexformat, ret)
            yield ret[-1]
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
    def __init__(self, opener, indexfile, datafile, defversion=REVLOGV0):
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
        self.defversion = defversion
        self.load()

    def load(self):
        v = self.defversion
        try:
            f = self.opener(self.indexfile)
            i = f.read(4)
            f.seek(0)
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
            i = ""
        else:
            try:
                st = util.fstat(f)
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
            if len(i) > 0:
                v = struct.unpack(versionformat, i)[0]
        flags = v & ~0xFFFF
        fmt = v & 0xFFFF
        if fmt == REVLOGV0:
            if flags:
                raise RevlogError(_("index %s invalid flags %x for format v0" %
                                   (self.indexfile, flags)))
        elif fmt == REVLOGNG:
            if flags & ~REVLOGNGINLINEDATA:
                raise RevlogError(_("index %s invalid flags %x for revlogng" %
                                   (self.indexfile, flags)))
        else:
            raise RevlogError(_("index %s invalid format %d" %
                               (self.indexfile, fmt)))
        self.version = v
        if v == REVLOGV0:
            self.indexformat = indexformatv0
            shaoffset = v0shaoffset
        else:
            self.indexformat = indexformatng
            shaoffset = ngshaoffset

        if i:
            if not self.inlinedata() and st and st.st_size > 10000:
                # big index, let's parse it on demand
                parser = lazyparser(f, st.st_size, self.indexformat, shaoffset)
                self.index = lazyindex(parser)
                self.nodemap = lazymap(parser)
            else:
                i = f.read()
                self.parseindex(i)
            if self.inlinedata():
                # we've already got the entire data file read in, save it
                # in the chunk data
                self.chunkcache = (0, i)
            if self.version != REVLOGV0:
                e = list(self.index[0])
                type = self.ngtype(e[0])
                e[0] = self.offset_type(0, type)
                self.index[0] = e
        else:
            self.nodemap = { nullid: -1}
            self.index = []


    def parseindex(self, data):
        s = struct.calcsize(self.indexformat)
        l = len(data)
        self.index = []
        self.nodemap =  {nullid: -1}
        inline = self.inlinedata()
        off = 0
        n = 0
        while off < l:
            e = struct.unpack(self.indexformat, data[off:off + s])
            self.index.append(e)
            self.nodemap[e[-1]] = n
            n += 1
            off += s
            if inline:
                off += e[1]

    def ngoffset(self, q):
        if q & 0xFFFF:
            raise RevlogError(_('%s: incompatible revision flag %x') %
                              (self.indexfile, q))
        return long(q >> 16)

    def ngtype(self, q):
        return int(q & 0xFFFF)

    def offset_type(self, offset, type):
        return long(long(offset) << 16 | type)

    def loadindex(self, start, end):
        """load a block of indexes all at once from the lazy parser"""
        if isinstance(self.index, lazyindex):
            self.index.p.loadindex(start, end)

    def loadindexmap(self):
        """loads both the map and the index from the lazy parser"""
        if isinstance(self.index, lazyindex):
            p = self.index.p
            p.loadindex()
            self.nodemap = p.map

    def loadmap(self):
        """loads the map from the lazy parser"""
        if isinstance(self.nodemap, lazymap):
            self.nodemap.p.loadmap()
            self.nodemap = self.nodemap.p.map

    def inlinedata(self): return self.version & REVLOGNGINLINEDATA
    def tip(self): return self.node(len(self.index) - 1)
    def count(self): return len(self.index)
    def node(self, rev):
        return (rev < 0) and nullid or self.index[rev][-1]
    def rev(self, node):
        try:
            return self.nodemap[node]
        except KeyError:
            raise RevlogError(_('%s: no node %s') % (self.indexfile, hex(node)))
    def linkrev(self, node): return self.index[self.rev(node)][-4]
    def parents(self, node):
        if node == nullid: return (nullid, nullid)
        r = self.rev(node)
        d = self.index[r][-3:-1]
        if self.version == REVLOGV0:
            return d
        return [ self.node(x) for x in d ]
    def start(self, rev):
        if rev < 0:
            return -1
        if self.version != REVLOGV0:
            return self.ngoffset(self.index[rev][0])
        return self.index[rev][0]

    def end(self, rev): return self.start(rev) + self.length(rev)

    def size(self, rev):
        """return the length of the uncompressed text for a given revision"""
        l = -1
        if self.version != REVLOGV0:
            l = self.index[rev][2]
        if l >= 0:
            return l

        t = self.revision(self.node(rev))
        return len(t)

        # alternate implementation, The advantage to this code is it
        # will be faster for a single revision.  But, the results are not
        # cached, so finding the size of every revision will be slower.
        """
        if self.cache and self.cache[1] == rev:
            return len(self.cache[2])

        base = self.base(rev)
        if self.cache and self.cache[1] >= base and self.cache[1] < rev:
            base = self.cache[1]
            text = self.cache[2]
        else:
            text = self.revision(self.node(base))

        l = len(text)
        for x in xrange(base + 1, rev + 1):
            l = mdiff.patchedsize(l, self.chunk(x))
        return l
        """

    def length(self, rev):
        if rev < 0:
            return 0
        else:
            return self.index[rev][1]
    def base(self, rev): return (rev < 0) and rev or self.index[rev][-5]

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

    def chunk(self, rev, df=None, cachelen=4096):
        start, length = self.start(rev), self.length(rev)
        inline = self.inlinedata()
        if inline:
            start += (rev + 1) * struct.calcsize(self.indexformat)
        end = start + length
        def loadcache(df):
            cache_length = max(cachelen, length) # 4k
            if not df:
                if inline:
                    df = self.opener(self.indexfile)
                else:
                    df = self.opener(self.datafile)
            df.seek(start)
            self.chunkcache = (start, df.read(cache_length))

        if not self.chunkcache:
            loadcache(df)

        cache_start = self.chunkcache[0]
        cache_end = cache_start + len(self.chunkcache[1])
        if start >= cache_start and end <= cache_end:
            # it is cached
            offset = start - cache_start
        else:
            loadcache(df)
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

        if self.inlinedata():
            # we probably have the whole chunk cached
            df = None
        else:
            df = self.opener(self.datafile)

        # do we have useful data cached?
        if self.cache and self.cache[1] >= base and self.cache[1] < rev:
            base = self.cache[1]
            text = self.cache[2]
            self.loadindex(base, rev + 1)
        else:
            self.loadindex(base, rev + 1)
            text = self.chunk(base, df=df)

        bins = []
        for r in xrange(base + 1, rev + 1):
            bins.append(self.chunk(r, df=df))

        text = self.patches(text, bins)

        p1, p2 = self.parents(node)
        if node != hash(text, p1, p2):
            raise RevlogError(_("integrity check failed on %s:%d")
                          % (self.datafile, rev))

        self.cache = (node, rev, text)
        return text

    def checkinlinesize(self, tr, fp=None):
        if not self.inlinedata():
            return
        if not fp:
            fp = self.opener(self.indexfile, 'r')
            fp.seek(0, 2)
        size = fp.tell()
        if size < 131072:
            return
        trinfo = tr.find(self.indexfile)
        if trinfo == None:
            raise RevlogError(_("%s not found in the transaction"  %
                              self.indexfile))

        trindex = trinfo[2]
        dataoff = self.start(trindex)

        tr.add(self.datafile, dataoff)
        df = self.opener(self.datafile, 'w')
        calc = struct.calcsize(self.indexformat)
        for r in xrange(self.count()):
            start = self.start(r) + (r + 1) * calc
            length = self.length(r)
            fp.seek(start)
            d = fp.read(length)
            df.write(d)
        fp.close()
        df.close()
        fp = self.opener(self.indexfile, 'w', atomictemp=True)
        self.version &= ~(REVLOGNGINLINEDATA)
        if self.count():
            x = self.index[0]
            e = struct.pack(self.indexformat, *x)[4:]
            l = struct.pack(versionformat, self.version)
            fp.write(l)
            fp.write(e)

        for i in xrange(1, self.count()):
            x = self.index[i]
            e = struct.pack(self.indexformat, *x)
            fp.write(e)

        # if we don't call rename, the temp file will never replace the
        # real index
        fp.rename()

        tr.replace(self.indexfile, trindex * calc)
        self.chunkcache = None

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

        if self.version == REVLOGV0:
            e = (offset, l, base, link, p1, p2, node)
        else:
            e = (self.offset_type(offset, 0), l, len(text),
                 base, link, self.rev(p1), self.rev(p2), node)

        self.index.append(e)
        self.nodemap[node] = n
        entry = struct.pack(self.indexformat, *e)

        if not self.inlinedata():
            transaction.add(self.datafile, offset)
            transaction.add(self.indexfile, n * len(entry))
            f = self.opener(self.datafile, "a")
            if data[0]:
                f.write(data[0])
            f.write(data[1])
            f.close()
            f = self.opener(self.indexfile, "a")
        else:
            f = self.opener(self.indexfile, "a+")
            f.seek(0, 2)
            transaction.add(self.indexfile, f.tell(), self.count() - 1)

        if len(self.index) == 1 and self.version != REVLOGV0:
            l = struct.pack(versionformat, self.version)
            f.write(l)
            entry = entry[4:]

        f.write(entry)

        if self.inlinedata():
            f.write(data[0])
            f.write(data[1])
            self.checkinlinesize(transaction, f)

        self.cache = (node, n, text)
        return node

    def ancestor(self, a, b):
        """calculate the least common ancestor of nodes a and b"""

        # start with some short cuts for the linear cases
        if a == b:
            return a
        ra = self.rev(a)
        rb = self.rev(b)
        if ra < rb:
            last = b
            first = a
        else:
            last = a
            first = b

        # reachable won't include stop in the list, so we have to use a parent
        reachable = self.reachable(last, stop=self.parents(first)[0])
        if first in reachable:
            return first

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
        start = end = textlen = 0
        if r:
            end = self.end(t)

        ifh = self.opener(self.indexfile, "a+")
        ifh.seek(0, 2)
        transaction.add(self.indexfile, ifh.tell(), self.count())
        if self.inlinedata():
            dfh = None
        else:
            transaction.add(self.datafile, end)
            dfh = self.opener(self.datafile, "a")

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
                textlen = mdiff.patchedsize(textlen, delta)

            if chain != prev or (end - start + len(cdelta)) > textlen * 2:
                # flush our writes here so we can read it in revision
                if dfh:
                    dfh.flush()
                ifh.flush()
                text = self.revision(chain)
                text = self.patches(text, [delta])
                chk = self.addrevision(text, transaction, link, p1, p2)
                if chk != node:
                    raise RevlogError(_("consistency error adding group"))
                textlen = len(text)
            else:
                if self.version == REVLOGV0:
                    e = (end, len(cdelta), base, link, p1, p2, node)
                else:
                    e = (self.offset_type(end, 0), len(cdelta), textlen, base,
                         link, self.rev(p1), self.rev(p2), node)
                self.index.append(e)
                self.nodemap[node] = r
                if self.inlinedata():
                    ifh.write(struct.pack(self.indexformat, *e))
                    ifh.write(cdelta)
                    self.checkinlinesize(transaction, ifh)
                    if not self.inlinedata():
                        dfh = self.opener(self.datafile, "a")
                        ifh = self.opener(self.indexfile, "a")
                else:
                    if not dfh:
                        # addrevision switched from inline to conventional
                        # reopen the index
                        dfh = self.opener(self.datafile, "a")
                        ifh = self.opener(self.indexfile, "a")
                    dfh.write(cdelta)
                    ifh.write(struct.pack(self.indexformat, *e))

            t, r, chain, prev = r, r + 1, node, node
            base = self.base(t)
            start = self.start(base)
            end = self.end(t)

        if node is None:
            raise RevlogError(_("group to be added is empty"))
        return node

    def strip(self, rev, minlink):
        if self.count() == 0 or rev >= self.count():
            return

        if isinstance(self.index, lazyindex):
            self.loadindexmap()

        # When stripping away a revision, we need to make sure it
        # does not actually belong to an older changeset.
        # The minlink parameter defines the oldest revision
        # we're allowed to strip away.
        while minlink > self.index[rev][-4]:
            rev += 1
            if rev >= self.count():
                return

        # first truncate the files on disk
        end = self.start(rev)
        if not self.inlinedata():
            df = self.opener(self.datafile, "a")
            df.truncate(end)
            end = rev * struct.calcsize(self.indexformat)
        else:
            end += rev * struct.calcsize(self.indexformat)

        indexf = self.opener(self.indexfile, "a")
        indexf.truncate(end)

        # then reset internal state in memory to forget those revisions
        self.cache = None
        self.chunkcache = None
        for x in xrange(rev, self.count()):
            del self.nodemap[self.node(x)]

        del self.index[rev:]

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
            s = struct.calcsize(self.indexformat)
            i = actual / s
            di = actual - (i * s)
            if self.inlinedata():
                databytes = 0
                for r in xrange(self.count()):
                    databytes += self.length(r)
                dd = 0
                di = actual - self.count() * s - databytes
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
            di = 0

        return (dd, di)


