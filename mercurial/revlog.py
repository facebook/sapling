# revlog.py - storage back-end for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Storage back-end for Mercurial.

This provides efficient delta storage with O(1) retrieve and append
and O(changes) merge between branches.
"""

# import stuff from node for others to import from revlog
import collections
from node import bin, hex, nullid, nullrev
from i18n import _
import ancestor, mdiff, parsers, error, util, templatefilters
import struct, zlib, errno

_pack = struct.pack
_unpack = struct.unpack
_compress = zlib.compress
_decompress = zlib.decompress
_sha = util.sha1

# revlog header flags
REVLOGV0 = 0
REVLOGNG = 1
REVLOGNGINLINEDATA = (1 << 16)
REVLOGGENERALDELTA = (1 << 17)
REVLOG_DEFAULT_FLAGS = REVLOGNGINLINEDATA
REVLOG_DEFAULT_FORMAT = REVLOGNG
REVLOG_DEFAULT_VERSION = REVLOG_DEFAULT_FORMAT | REVLOG_DEFAULT_FLAGS
REVLOGNG_FLAGS = REVLOGNGINLINEDATA | REVLOGGENERALDELTA

# revlog index flags
REVIDX_ISCENSORED = (1 << 15) # revision has censor metadata, must be verified
REVIDX_DEFAULT_FLAGS = 0
REVIDX_KNOWN_FLAGS = REVIDX_ISCENSORED

# max size of revlog with inline data
_maxinline = 131072
_chunksize = 1048576

RevlogError = error.RevlogError
LookupError = error.LookupError
CensoredNodeError = error.CensoredNodeError

def getoffset(q):
    return int(q >> 16)

def gettype(q):
    return int(q & 0xFFFF)

def offset_type(offset, type):
    return long(long(offset) << 16 | type)

_nullhash = _sha(nullid)

def hash(text, p1, p2):
    """generate a hash from the given text and its parent hashes

    This hash combines both the current file contents and its history
    in a manner that makes it easy to distinguish nodes with the same
    content in the revision graph.
    """
    # As of now, if one of the parent node is null, p2 is null
    if p2 == nullid:
        # deep copy of a hash is faster than creating one
        s = _nullhash.copy()
        s.update(p1)
    else:
        # none of the parent nodes are nullid
        l = [p1, p2]
        l.sort()
        s = _sha(l[0])
        s.update(l[1])
    s.update(text)
    return s.digest()

def decompress(bin):
    """ decompress the given input """
    if not bin:
        return bin
    t = bin[0]
    if t == '\0':
        return bin
    if t == 'x':
        try:
            return _decompress(bin)
        except zlib.error, e:
            raise RevlogError(_("revlog decompress error: %s") % str(e))
    if t == 'u':
        return bin[1:]
    raise RevlogError(_("unknown compression type %r") % t)

# index v0:
#  4 bytes: offset
#  4 bytes: compressed length
#  4 bytes: base rev
#  4 bytes: link rev
# 32 bytes: parent 1 nodeid
# 32 bytes: parent 2 nodeid
# 32 bytes: nodeid
indexformatv0 = ">4l20s20s20s"
v0shaoffset = 56

class revlogoldio(object):
    def __init__(self):
        self.size = struct.calcsize(indexformatv0)

    def parseindex(self, data, inline):
        s = self.size
        index = []
        nodemap =  {nullid: nullrev}
        n = off = 0
        l = len(data)
        while off + s <= l:
            cur = data[off:off + s]
            off += s
            e = _unpack(indexformatv0, cur)
            # transform to revlogv1 format
            e2 = (offset_type(e[0], 0), e[1], -1, e[2], e[3],
                  nodemap.get(e[4], nullrev), nodemap.get(e[5], nullrev), e[6])
            index.append(e2)
            nodemap[e[6]] = n
            n += 1

        # add the magic null revision at -1
        index.append((0, 0, 0, -1, -1, -1, -1, nullid))

        return index, nodemap, None

    def packentry(self, entry, node, version, rev):
        if gettype(entry[0]):
            raise RevlogError(_("index entry flags need RevlogNG"))
        e2 = (getoffset(entry[0]), entry[1], entry[3], entry[4],
              node(entry[5]), node(entry[6]), entry[7])
        return _pack(indexformatv0, *e2)

# index ng:
#  6 bytes: offset
#  2 bytes: flags
#  4 bytes: compressed length
#  4 bytes: uncompressed length
#  4 bytes: base rev
#  4 bytes: link rev
#  4 bytes: parent 1 rev
#  4 bytes: parent 2 rev
# 32 bytes: nodeid
indexformatng = ">Qiiiiii20s12x"
ngshaoffset = 32
versionformat = ">I"

# corresponds to uncompressed length of indexformatng (2 gigs, 4-byte
# signed integer)
_maxentrysize = 0x7fffffff

class revlogio(object):
    def __init__(self):
        self.size = struct.calcsize(indexformatng)

    def parseindex(self, data, inline):
        # call the C implementation to parse the index data
        index, cache = parsers.parse_index2(data, inline)
        return index, getattr(index, 'nodemap', None), cache

    def packentry(self, entry, node, version, rev):
        p = _pack(indexformatng, *entry)
        if rev == 0:
            p = _pack(versionformat, version) + p[4:]
        return p

class revlog(object):
    """
    the underlying revision storage object

    A revlog consists of two parts, an index and the revision data.

    The index is a file with a fixed record size containing
    information on each revision, including its nodeid (hash), the
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
    def __init__(self, opener, indexfile):
        """
        create a revlog object

        opener is a function that abstracts the file opening operation
        and can be used to implement COW semantics or the like.
        """
        self.indexfile = indexfile
        self.datafile = indexfile[:-2] + ".d"
        self.opener = opener
        self._cache = None
        self._basecache = None
        self._chunkcache = (0, '')
        self._chunkcachesize = 65536
        self._maxchainlen = None
        self.index = []
        self._pcache = {}
        self._nodecache = {nullid: nullrev}
        self._nodepos = None

        v = REVLOG_DEFAULT_VERSION
        opts = getattr(opener, 'options', None)
        if opts is not None:
            if 'revlogv1' in opts:
                if 'generaldelta' in opts:
                    v |= REVLOGGENERALDELTA
            else:
                v = 0
            if 'chunkcachesize' in opts:
                self._chunkcachesize = opts['chunkcachesize']
            if 'maxchainlen' in opts:
                self._maxchainlen = opts['maxchainlen']

        if self._chunkcachesize <= 0:
            raise RevlogError(_('revlog chunk cache size %r is not greater '
                                'than 0') % self._chunkcachesize)
        elif self._chunkcachesize & (self._chunkcachesize - 1):
            raise RevlogError(_('revlog chunk cache size %r is not a power '
                                'of 2') % self._chunkcachesize)

        i = ''
        self._initempty = True
        try:
            f = self.opener(self.indexfile)
            i = f.read()
            f.close()
            if len(i) > 0:
                v = struct.unpack(versionformat, i[:4])[0]
                self._initempty = False
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise

        self.version = v
        self._inline = v & REVLOGNGINLINEDATA
        self._generaldelta = v & REVLOGGENERALDELTA
        flags = v & ~0xFFFF
        fmt = v & 0xFFFF
        if fmt == REVLOGV0 and flags:
            raise RevlogError(_("index %s unknown flags %#04x for format v0")
                              % (self.indexfile, flags >> 16))
        elif fmt == REVLOGNG and flags & ~REVLOGNG_FLAGS:
            raise RevlogError(_("index %s unknown flags %#04x for revlogng")
                              % (self.indexfile, flags >> 16))
        elif fmt > REVLOGNG:
            raise RevlogError(_("index %s unknown format %d")
                              % (self.indexfile, fmt))

        self._io = revlogio()
        if self.version == REVLOGV0:
            self._io = revlogoldio()
        try:
            d = self._io.parseindex(i, self._inline)
        except (ValueError, IndexError):
            raise RevlogError(_("index %s is corrupted") % (self.indexfile))
        self.index, nodemap, self._chunkcache = d
        if nodemap is not None:
            self.nodemap = self._nodecache = nodemap
        if not self._chunkcache:
            self._chunkclear()
        # revnum -> (chain-length, sum-delta-length)
        self._chaininfocache = {}

    def tip(self):
        return self.node(len(self.index) - 2)
    def __contains__(self, rev):
        return 0 <= rev < len(self)
    def __len__(self):
        return len(self.index) - 1
    def __iter__(self):
        return iter(xrange(len(self)))
    def revs(self, start=0, stop=None):
        """iterate over all rev in this revlog (from start to stop)"""
        step = 1
        if stop is not None:
            if start > stop:
                step = -1
            stop += step
        else:
            stop = len(self)
        return xrange(start, stop, step)

    @util.propertycache
    def nodemap(self):
        self.rev(self.node(0))
        return self._nodecache

    def hasnode(self, node):
        try:
            self.rev(node)
            return True
        except KeyError:
            return False

    def clearcaches(self):
        try:
            self._nodecache.clearcaches()
        except AttributeError:
            self._nodecache = {nullid: nullrev}
            self._nodepos = None

    def rev(self, node):
        try:
            return self._nodecache[node]
        except TypeError:
            raise
        except RevlogError:
            # parsers.c radix tree lookup failed
            raise LookupError(node, self.indexfile, _('no node'))
        except KeyError:
            # pure python cache lookup failed
            n = self._nodecache
            i = self.index
            p = self._nodepos
            if p is None:
                p = len(i) - 2
            for r in xrange(p, -1, -1):
                v = i[r][7]
                n[v] = r
                if v == node:
                    self._nodepos = r - 1
                    return r
            raise LookupError(node, self.indexfile, _('no node'))

    def node(self, rev):
        return self.index[rev][7]
    def linkrev(self, rev):
        return self.index[rev][4]
    def parents(self, node):
        i = self.index
        d = i[self.rev(node)]
        return i[d[5]][7], i[d[6]][7] # map revisions to nodes inline
    def parentrevs(self, rev):
        return self.index[rev][5:7]
    def start(self, rev):
        return int(self.index[rev][0] >> 16)
    def end(self, rev):
        return self.start(rev) + self.length(rev)
    def length(self, rev):
        return self.index[rev][1]
    def chainbase(self, rev):
        index = self.index
        base = index[rev][3]
        while base != rev:
            rev = base
            base = index[rev][3]
        return base
    def chainlen(self, rev):
        return self._chaininfo(rev)[0]

    def _chaininfo(self, rev):
        chaininfocache = self._chaininfocache
        if rev in chaininfocache:
            return chaininfocache[rev]
        index = self.index
        generaldelta = self._generaldelta
        iterrev = rev
        e = index[iterrev]
        clen = 0
        compresseddeltalen = 0
        while iterrev != e[3]:
            clen += 1
            compresseddeltalen += e[1]
            if generaldelta:
                iterrev = e[3]
            else:
                iterrev -= 1
            if iterrev in chaininfocache:
                t = chaininfocache[iterrev]
                clen += t[0]
                compresseddeltalen += t[1]
                break
            e = index[iterrev]
        else:
            # Add text length of base since decompressing that also takes
            # work. For cache hits the length is already included.
            compresseddeltalen += e[1]
        r = (clen, compresseddeltalen)
        chaininfocache[rev] = r
        return r

    def flags(self, rev):
        return self.index[rev][0] & 0xFFFF
    def rawsize(self, rev):
        """return the length of the uncompressed text for a given revision"""
        l = self.index[rev][2]
        if l >= 0:
            return l

        t = self.revision(self.node(rev))
        return len(t)
    size = rawsize

    def ancestors(self, revs, stoprev=0, inclusive=False):
        """Generate the ancestors of 'revs' in reverse topological order.
        Does not generate revs lower than stoprev.

        See the documentation for ancestor.lazyancestors for more details."""

        return ancestor.lazyancestors(self.parentrevs, revs, stoprev=stoprev,
                                      inclusive=inclusive)

    def descendants(self, revs):
        """Generate the descendants of 'revs' in revision order.

        Yield a sequence of revision numbers starting with a child of
        some rev in revs, i.e., each revision is *not* considered a
        descendant of itself.  Results are ordered by revision number (a
        topological sort)."""
        first = min(revs)
        if first == nullrev:
            for i in self:
                yield i
            return

        seen = set(revs)
        for i in self.revs(start=first + 1):
            for x in self.parentrevs(i):
                if x != nullrev and x in seen:
                    seen.add(i)
                    yield i
                    break

    def findcommonmissing(self, common=None, heads=None):
        """Return a tuple of the ancestors of common and the ancestors of heads
        that are not ancestors of common. In revset terminology, we return the
        tuple:

          ::common, (::heads) - (::common)

        The list is sorted by revision number, meaning it is
        topologically sorted.

        'heads' and 'common' are both lists of node IDs.  If heads is
        not supplied, uses all of the revlog's heads.  If common is not
        supplied, uses nullid."""
        if common is None:
            common = [nullid]
        if heads is None:
            heads = self.heads()

        common = [self.rev(n) for n in common]
        heads = [self.rev(n) for n in heads]

        # we want the ancestors, but inclusive
        class lazyset(object):
            def __init__(self, lazyvalues):
                self.addedvalues = set()
                self.lazyvalues = lazyvalues

            def __contains__(self, value):
                return value in self.addedvalues or value in self.lazyvalues

            def __iter__(self):
                added = self.addedvalues
                for r in added:
                    yield r
                for r in self.lazyvalues:
                    if not r in added:
                        yield r

            def add(self, value):
                self.addedvalues.add(value)

            def update(self, values):
                self.addedvalues.update(values)

        has = lazyset(self.ancestors(common))
        has.add(nullrev)
        has.update(common)

        # take all ancestors from heads that aren't in has
        missing = set()
        visit = collections.deque(r for r in heads if r not in has)
        while visit:
            r = visit.popleft()
            if r in missing:
                continue
            else:
                missing.add(r)
                for p in self.parentrevs(r):
                    if p not in has:
                        visit.append(p)
        missing = list(missing)
        missing.sort()
        return has, [self.node(r) for r in missing]

    def incrementalmissingrevs(self, common=None):
        """Return an object that can be used to incrementally compute the
        revision numbers of the ancestors of arbitrary sets that are not
        ancestors of common. This is an ancestor.incrementalmissingancestors
        object.

        'common' is a list of revision numbers. If common is not supplied, uses
        nullrev.
        """
        if common is None:
            common = [nullrev]

        return ancestor.incrementalmissingancestors(self.parentrevs, common)

    def findmissingrevs(self, common=None, heads=None):
        """Return the revision numbers of the ancestors of heads that
        are not ancestors of common.

        More specifically, return a list of revision numbers corresponding to
        nodes N such that every N satisfies the following constraints:

          1. N is an ancestor of some node in 'heads'
          2. N is not an ancestor of any node in 'common'

        The list is sorted by revision number, meaning it is
        topologically sorted.

        'heads' and 'common' are both lists of revision numbers.  If heads is
        not supplied, uses all of the revlog's heads.  If common is not
        supplied, uses nullid."""
        if common is None:
            common = [nullrev]
        if heads is None:
            heads = self.headrevs()

        inc = self.incrementalmissingrevs(common=common)
        return inc.missingancestors(heads)

    def findmissing(self, common=None, heads=None):
        """Return the ancestors of heads that are not ancestors of common.

        More specifically, return a list of nodes N such that every N
        satisfies the following constraints:

          1. N is an ancestor of some node in 'heads'
          2. N is not an ancestor of any node in 'common'

        The list is sorted by revision number, meaning it is
        topologically sorted.

        'heads' and 'common' are both lists of node IDs.  If heads is
        not supplied, uses all of the revlog's heads.  If common is not
        supplied, uses nullid."""
        if common is None:
            common = [nullid]
        if heads is None:
            heads = self.heads()

        common = [self.rev(n) for n in common]
        heads = [self.rev(n) for n in heads]

        inc = self.incrementalmissingrevs(common=common)
        return [self.node(r) for r in inc.missingancestors(heads)]

    def nodesbetween(self, roots=None, heads=None):
        """Return a topological path from 'roots' to 'heads'.

        Return a tuple (nodes, outroots, outheads) where 'nodes' is a
        topologically sorted list of all nodes N that satisfy both of
        these constraints:

          1. N is a descendant of some node in 'roots'
          2. N is an ancestor of some node in 'heads'

        Every node is considered to be both a descendant and an ancestor
        of itself, so every reachable node in 'roots' and 'heads' will be
        included in 'nodes'.

        'outroots' is the list of reachable nodes in 'roots', i.e., the
        subset of 'roots' that is returned in 'nodes'.  Likewise,
        'outheads' is the subset of 'heads' that is also in 'nodes'.

        'roots' and 'heads' are both lists of node IDs.  If 'roots' is
        unspecified, uses nullid as the only root.  If 'heads' is
        unspecified, uses list of all of the revlog's heads."""
        nonodes = ([], [], [])
        if roots is not None:
            roots = list(roots)
            if not roots:
                return nonodes
            lowestrev = min([self.rev(n) for n in roots])
        else:
            roots = [nullid] # Everybody's a descendant of nullid
            lowestrev = nullrev
        if (lowestrev == nullrev) and (heads is None):
            # We want _all_ the nodes!
            return ([self.node(r) for r in self], [nullid], list(self.heads()))
        if heads is None:
            # All nodes are ancestors, so the latest ancestor is the last
            # node.
            highestrev = len(self) - 1
            # Set ancestors to None to signal that every node is an ancestor.
            ancestors = None
            # Set heads to an empty dictionary for later discovery of heads
            heads = {}
        else:
            heads = list(heads)
            if not heads:
                return nonodes
            ancestors = set()
            # Turn heads into a dictionary so we can remove 'fake' heads.
            # Also, later we will be using it to filter out the heads we can't
            # find from roots.
            heads = dict.fromkeys(heads, False)
            # Start at the top and keep marking parents until we're done.
            nodestotag = set(heads)
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
                        # If we are possibly a descendant of one of the roots
                        # and we haven't already been marked as an ancestor
                        ancestors.add(n) # Mark as ancestor
                        # Add non-nullid parents to list of nodes to tag.
                        nodestotag.update([p for p in self.parents(n) if
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
            if lowestrev > nullrev:
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
                lowestrev = nullrev
                roots = [nullid]
        # Transform our roots list into a set.
        descendants = set(roots)
        # Also, keep the original roots so we can filter out roots that aren't
        # 'real' roots (i.e. are descended from other roots).
        roots = descendants.copy()
        # Our topologically sorted list of output nodes.
        orderedout = []
        # Don't start at nullid since we don't want nullid in our output list,
        # and if nullid shows up in descendants, empty parents will look like
        # they're descendants.
        for r in self.revs(start=max(lowestrev, 0), stop=highestrev + 1):
            n = self.node(r)
            isdescendant = False
            if lowestrev == nullrev:  # Everybody is a descendant of nullid
                isdescendant = True
            elif n in descendants:
                # n is already a descendant
                isdescendant = True
                # This check only needs to be done here because all the roots
                # will start being marked is descendants before the loop.
                if n in roots:
                    # If n was a root, check if it's a 'real' root.
                    p = tuple(self.parents(n))
                    # If any of its parents are descendants, it's not a root.
                    if (p[0] in descendants) or (p[1] in descendants):
                        roots.remove(n)
            else:
                p = tuple(self.parents(n))
                # A node is a descendant if either of its parents are
                # descendants.  (We seeded the dependents list with the roots
                # up there, remember?)
                if (p[0] in descendants) or (p[1] in descendants):
                    descendants.add(n)
                    isdescendant = True
            if isdescendant and ((ancestors is None) or (n in ancestors)):
                # Only include nodes that are both descendants and ancestors.
                orderedout.append(n)
                if (ancestors is not None) and (n in heads):
                    # We're trying to figure out which heads are reachable
                    # from roots.
                    # Mark this head as having been reached
                    heads[n] = True
                elif ancestors is None:
                    # Otherwise, we're trying to discover the heads.
                    # Assume this is a head because if it isn't, the next step
                    # will eventually remove it.
                    heads[n] = True
                    # But, obviously its parents aren't.
                    for p in self.parents(n):
                        heads.pop(p, None)
        heads = [n for n, flag in heads.iteritems() if flag]
        roots = list(roots)
        assert orderedout
        assert roots
        assert heads
        return (orderedout, roots, heads)

    def headrevs(self):
        try:
            return self.index.headrevs()
        except AttributeError:
            return self._headrevs()

    def computephases(self, roots):
        return self.index.computephasesmapsets(roots)

    def _headrevs(self):
        count = len(self)
        if not count:
            return [nullrev]
        # we won't iter over filtered rev so nobody is a head at start
        ishead = [0] * (count + 1)
        index = self.index
        for r in self:
            ishead[r] = 1  # I may be an head
            e = index[r]
            ishead[e[5]] = ishead[e[6]] = 0  # my parent are not
        return [r for r, val in enumerate(ishead) if val]

    def heads(self, start=None, stop=None):
        """return the list of all nodes that have no children

        if start is specified, only heads that are descendants of
        start will be returned
        if stop is specified, it will consider all the revs from stop
        as if they had no children
        """
        if start is None and stop is None:
            if not len(self):
                return [nullid]
            return [self.node(r) for r in self.headrevs()]

        if start is None:
            start = nullid
        if stop is None:
            stop = []
        stoprevs = set([self.rev(n) for n in stop])
        startrev = self.rev(start)
        reachable = set((startrev,))
        heads = set((startrev,))

        parentrevs = self.parentrevs
        for r in self.revs(start=startrev + 1):
            for p in parentrevs(r):
                if p in reachable:
                    if r not in stoprevs:
                        reachable.add(r)
                    heads.add(r)
                if p in heads and p not in stoprevs:
                    heads.remove(p)

        return [self.node(r) for r in heads]

    def children(self, node):
        """find the children of a given node"""
        c = []
        p = self.rev(node)
        for r in self.revs(start=p + 1):
            prevs = [pr for pr in self.parentrevs(r) if pr != nullrev]
            if prevs:
                for pr in prevs:
                    if pr == p:
                        c.append(self.node(r))
            elif p == nullrev:
                c.append(self.node(r))
        return c

    def descendant(self, start, end):
        if start == nullrev:
            return True
        for i in self.descendants([start]):
            if i == end:
                return True
            elif i > end:
                break
        return False

    def commonancestorsheads(self, a, b):
        """calculate all the heads of the common ancestors of nodes a and b"""
        a, b = self.rev(a), self.rev(b)
        try:
            ancs = self.index.commonancestorsheads(a, b)
        except (AttributeError, OverflowError): # C implementation failed
            ancs = ancestor.commonancestorsheads(self.parentrevs, a, b)
        return map(self.node, ancs)

    def isancestor(self, a, b):
        """return True if node a is an ancestor of node b

        The implementation of this is trivial but the use of
        commonancestorsheads is not."""
        return a in self.commonancestorsheads(a, b)

    def ancestor(self, a, b):
        """calculate the "best" common ancestor of nodes a and b"""

        a, b = self.rev(a), self.rev(b)
        try:
            ancs = self.index.ancestors(a, b)
        except (AttributeError, OverflowError):
            ancs = ancestor.ancestors(self.parentrevs, a, b)
        if ancs:
            # choose a consistent winner when there's a tie
            return min(map(self.node, ancs))
        return nullid

    def _match(self, id):
        if isinstance(id, int):
            # rev
            return self.node(id)
        if len(id) == 20:
            # possibly a binary node
            # odds of a binary node being all hex in ASCII are 1 in 10**25
            try:
                node = id
                self.rev(node) # quick search the index
                return node
            except LookupError:
                pass # may be partial hex id
        try:
            # str(rev)
            rev = int(id)
            if str(rev) != id:
                raise ValueError
            if rev < 0:
                rev = len(self) + rev
            if rev < 0 or rev >= len(self):
                raise ValueError
            return self.node(rev)
        except (ValueError, OverflowError):
            pass
        if len(id) == 40:
            try:
                # a full hex nodeid?
                node = bin(id)
                self.rev(node)
                return node
            except (TypeError, LookupError):
                pass

    def _partialmatch(self, id):
        try:
            n = self.index.partialmatch(id)
            if n and self.hasnode(n):
                return n
            return None
        except RevlogError:
            # parsers.c radix tree lookup gave multiple matches
            # fall through to slow path that filters hidden revisions
            pass
        except (AttributeError, ValueError):
            # we are pure python, or key was too short to search radix tree
            pass

        if id in self._pcache:
            return self._pcache[id]

        if len(id) < 40:
            try:
                # hex(node)[:...]
                l = len(id) // 2  # grab an even number of digits
                prefix = bin(id[:l * 2])
                nl = [e[7] for e in self.index if e[7].startswith(prefix)]
                nl = [n for n in nl if hex(n).startswith(id) and
                      self.hasnode(n)]
                if len(nl) > 0:
                    if len(nl) == 1:
                        self._pcache[id] = nl[0]
                        return nl[0]
                    raise LookupError(id, self.indexfile,
                                      _('ambiguous identifier'))
                return None
            except TypeError:
                pass

    def lookup(self, id):
        """locate a node based on:
            - revision number or str(revision number)
            - nodeid or subset of hex nodeid
        """
        n = self._match(id)
        if n is not None:
            return n
        n = self._partialmatch(id)
        if n:
            return n

        raise LookupError(id, self.indexfile, _('no match found'))

    def cmp(self, node, text):
        """compare text with a given file revision

        returns True if text is different than what is stored.
        """
        p1, p2 = self.parents(node)
        return hash(text, p1, p2) != node

    def _addchunk(self, offset, data):
        o, d = self._chunkcache
        # try to add to existing cache
        if o + len(d) == offset and len(d) + len(data) < _chunksize:
            self._chunkcache = o, d + data
        else:
            self._chunkcache = offset, data

    def _loadchunk(self, offset, length):
        if self._inline:
            df = self.opener(self.indexfile)
        else:
            df = self.opener(self.datafile)

        # Cache data both forward and backward around the requested
        # data, in a fixed size window. This helps speed up operations
        # involving reading the revlog backwards.
        cachesize = self._chunkcachesize
        realoffset = offset & ~(cachesize - 1)
        reallength = (((offset + length + cachesize) & ~(cachesize - 1))
                      - realoffset)
        df.seek(realoffset)
        d = df.read(reallength)
        df.close()
        self._addchunk(realoffset, d)
        if offset != realoffset or reallength != length:
            return util.buffer(d, offset - realoffset, length)
        return d

    def _getchunk(self, offset, length):
        o, d = self._chunkcache
        l = len(d)

        # is it in the cache?
        cachestart = offset - o
        cacheend = cachestart + length
        if cachestart >= 0 and cacheend <= l:
            if cachestart == 0 and cacheend == l:
                return d # avoid a copy
            return util.buffer(d, cachestart, cacheend - cachestart)

        return self._loadchunk(offset, length)

    def _chunkraw(self, startrev, endrev):
        start = self.start(startrev)
        end = self.end(endrev)
        if self._inline:
            start += (startrev + 1) * self._io.size
            end += (endrev + 1) * self._io.size
        length = end - start
        return self._getchunk(start, length)

    def _chunk(self, rev):
        return decompress(self._chunkraw(rev, rev))

    def _chunks(self, revs):
        '''faster version of [self._chunk(rev) for rev in revs]

        Assumes that revs is in ascending order.'''
        if not revs:
            return []
        start = self.start
        length = self.length
        inline = self._inline
        iosize = self._io.size
        buffer = util.buffer

        l = []
        ladd = l.append

        # preload the cache
        try:
            while True:
                # ensure that the cache doesn't change out from under us
                _cache = self._chunkcache
                self._chunkraw(revs[0], revs[-1])
                if _cache == self._chunkcache:
                    break
            offset, data = _cache
        except OverflowError:
            # issue4215 - we can't cache a run of chunks greater than
            # 2G on Windows
            return [self._chunk(rev) for rev in revs]

        for rev in revs:
            chunkstart = start(rev)
            if inline:
                chunkstart += (rev + 1) * iosize
            chunklength = length(rev)
            ladd(decompress(buffer(data, chunkstart - offset, chunklength)))

        return l

    def _chunkclear(self):
        self._chunkcache = (0, '')

    def deltaparent(self, rev):
        """return deltaparent of the given revision"""
        base = self.index[rev][3]
        if base == rev:
            return nullrev
        elif self._generaldelta:
            return base
        else:
            return rev - 1

    def revdiff(self, rev1, rev2):
        """return or calculate a delta between two revisions"""
        if rev1 != nullrev and self.deltaparent(rev2) == rev1:
            return str(self._chunk(rev2))

        return mdiff.textdiff(self.revision(rev1),
                              self.revision(rev2))

    def revision(self, nodeorrev):
        """return an uncompressed revision of a given node or revision
        number.
        """
        if isinstance(nodeorrev, int):
            rev = nodeorrev
            node = self.node(rev)
        else:
            node = nodeorrev
            rev = None

        _cache = self._cache # grab local copy of cache to avoid thread race
        cachedrev = None
        if node == nullid:
            return ""
        if _cache:
            if _cache[0] == node:
                return _cache[2]
            cachedrev = _cache[1]

        # look up what we need to read
        text = None
        if rev is None:
            rev = self.rev(node)

        # check rev flags
        if self.flags(rev) & ~REVIDX_KNOWN_FLAGS:
            raise RevlogError(_('incompatible revision flag %x') %
                              (self.flags(rev) & ~REVIDX_KNOWN_FLAGS))

        # build delta chain
        chain = []
        index = self.index # for performance
        generaldelta = self._generaldelta
        iterrev = rev
        e = index[iterrev]
        while iterrev != e[3] and iterrev != cachedrev:
            chain.append(iterrev)
            if generaldelta:
                iterrev = e[3]
            else:
                iterrev -= 1
            e = index[iterrev]

        if iterrev == cachedrev:
            # cache hit
            text = _cache[2]
        else:
            chain.append(iterrev)
        chain.reverse()

        # drop cache to save memory
        self._cache = None

        bins = self._chunks(chain)
        if text is None:
            text = str(bins[0])
            bins = bins[1:]

        text = mdiff.patches(text, bins)

        text = self._checkhash(text, node, rev)

        self._cache = (node, rev, text)
        return text

    def hash(self, text, p1, p2):
        """Compute a node hash.

        Available as a function so that subclasses can replace the hash
        as needed.
        """
        return hash(text, p1, p2)

    def _checkhash(self, text, node, rev):
        p1, p2 = self.parents(node)
        self.checkhash(text, p1, p2, node, rev)
        return text

    def checkhash(self, text, p1, p2, node, rev=None):
        if node != self.hash(text, p1, p2):
            revornode = rev
            if revornode is None:
                revornode = templatefilters.short(hex(node))
            raise RevlogError(_("integrity check failed on %s:%s")
                % (self.indexfile, revornode))

    def checkinlinesize(self, tr, fp=None):
        if not self._inline or (self.start(-2) + self.length(-2)) < _maxinline:
            return

        trinfo = tr.find(self.indexfile)
        if trinfo is None:
            raise RevlogError(_("%s not found in the transaction")
                              % self.indexfile)

        trindex = trinfo[2]
        if trindex is not None:
            dataoff = self.start(trindex)
        else:
            # revlog was stripped at start of transaction, use all leftover data
            trindex = len(self) - 1
            dataoff = self.end(-2)

        tr.add(self.datafile, dataoff)

        if fp:
            fp.flush()
            fp.close()

        df = self.opener(self.datafile, 'w')
        try:
            for r in self:
                df.write(self._chunkraw(r, r))
        finally:
            df.close()

        fp = self.opener(self.indexfile, 'w', atomictemp=True)
        self.version &= ~(REVLOGNGINLINEDATA)
        self._inline = False
        for i in self:
            e = self._io.packentry(self.index[i], self.node, self.version, i)
            fp.write(e)

        # if we don't call close, the temp file will never replace the
        # real index
        fp.close()

        tr.replace(self.indexfile, trindex * self._io.size)
        self._chunkclear()

    def addrevision(self, text, transaction, link, p1, p2, cachedelta=None,
                    node=None):
        """add a revision to the log

        text - the revision data to add
        transaction - the transaction object used for rollback
        link - the linkrev data to add
        p1, p2 - the parent nodeids of the revision
        cachedelta - an optional precomputed delta
        node - nodeid of revision; typically node is not specified, and it is
            computed by default as hash(text, p1, p2), however subclasses might
            use different hashing method (and override checkhash() in such case)
        """
        if link == nullrev:
            raise RevlogError(_("attempted to add linkrev -1 to %s")
                              % self.indexfile)

        if len(text) > _maxentrysize:
            raise RevlogError(
                _("%s: size of %d bytes exceeds maximum revlog storage of 2GiB")
                % (self.indexfile, len(text)))

        node = node or self.hash(text, p1, p2)
        if node in self.nodemap:
            return node

        dfh = None
        if not self._inline:
            dfh = self.opener(self.datafile, "a")
        ifh = self.opener(self.indexfile, "a+")
        try:
            return self._addrevision(node, text, transaction, link, p1, p2,
                                     REVIDX_DEFAULT_FLAGS, cachedelta, ifh, dfh)
        finally:
            if dfh:
                dfh.close()
            ifh.close()

    def compress(self, text):
        """ generate a possibly-compressed representation of text """
        if not text:
            return ("", text)
        l = len(text)
        bin = None
        if l < 44:
            pass
        elif l > 1000000:
            # zlib makes an internal copy, thus doubling memory usage for
            # large files, so lets do this in pieces
            z = zlib.compressobj()
            p = []
            pos = 0
            while pos < l:
                pos2 = pos + 2**20
                p.append(z.compress(text[pos:pos2]))
                pos = pos2
            p.append(z.flush())
            if sum(map(len, p)) < l:
                bin = "".join(p)
        else:
            bin = _compress(text)
        if bin is None or len(bin) > l:
            if text[0] == '\0':
                return ("", text)
            return ('u', text)
        return ("", bin)

    def _addrevision(self, node, text, transaction, link, p1, p2, flags,
                     cachedelta, ifh, dfh):
        """internal function to add revisions to the log

        see addrevision for argument descriptions.
        invariants:
        - text is optional (can be None); if not set, cachedelta must be set.
          if both are set, they must correspond to each other.
        """
        btext = [text]
        def buildtext():
            if btext[0] is not None:
                return btext[0]
            # flush any pending writes here so we can read it in revision
            if dfh:
                dfh.flush()
            ifh.flush()
            baserev = cachedelta[0]
            delta = cachedelta[1]
            # special case deltas which replace entire base; no need to decode
            # base revision. this neatly avoids censored bases, which throw when
            # they're decoded.
            hlen = struct.calcsize(">lll")
            if delta[:hlen] == mdiff.replacediffheader(self.rawsize(baserev),
                                                       len(delta) - hlen):
                btext[0] = delta[hlen:]
            else:
                basetext = self.revision(self.node(baserev))
                btext[0] = mdiff.patch(basetext, delta)
            try:
                self.checkhash(btext[0], p1, p2, node)
                if flags & REVIDX_ISCENSORED:
                    raise RevlogError(_('node %s is not censored') % node)
            except CensoredNodeError:
                # must pass the censored index flag to add censored revisions
                if not flags & REVIDX_ISCENSORED:
                    raise
            return btext[0]

        def builddelta(rev):
            # can we use the cached delta?
            if cachedelta and cachedelta[0] == rev:
                delta = cachedelta[1]
            else:
                t = buildtext()
                if self.iscensored(rev):
                    # deltas based on a censored revision must replace the
                    # full content in one patch, so delta works everywhere
                    header = mdiff.replacediffheader(self.rawsize(rev), len(t))
                    delta = header + t
                else:
                    ptext = self.revision(self.node(rev))
                    delta = mdiff.textdiff(ptext, t)
            data = self.compress(delta)
            l = len(data[1]) + len(data[0])
            if basecache[0] == rev:
                chainbase = basecache[1]
            else:
                chainbase = self.chainbase(rev)
            dist = l + offset - self.start(chainbase)
            if self._generaldelta:
                base = rev
            else:
                base = chainbase
            chainlen, compresseddeltalen = self._chaininfo(rev)
            chainlen += 1
            compresseddeltalen += l
            return dist, l, data, base, chainbase, chainlen, compresseddeltalen

        curr = len(self)
        prev = curr - 1
        base = chainbase = curr
        chainlen = None
        offset = self.end(prev)
        d = None
        if self._basecache is None:
            self._basecache = (prev, self.chainbase(prev))
        basecache = self._basecache
        p1r, p2r = self.rev(p1), self.rev(p2)

        # should we try to build a delta?
        if prev != nullrev:
            if self._generaldelta:
                if p1r >= basecache[1]:
                    d = builddelta(p1r)
                elif p2r >= basecache[1]:
                    d = builddelta(p2r)
                else:
                    d = builddelta(prev)
            else:
                d = builddelta(prev)
            dist, l, data, base, chainbase, chainlen, compresseddeltalen = d

        # full versions are inserted when the needed deltas
        # become comparable to the uncompressed text
        if text is None:
            textlen = mdiff.patchedsize(self.rawsize(cachedelta[0]),
                                        cachedelta[1])
        else:
            textlen = len(text)

        # - 'dist' is the distance from the base revision -- bounding it limits
        #   the amount of I/O we need to do.
        # - 'compresseddeltalen' is the sum of the total size of deltas we need
        #   to apply -- bounding it limits the amount of CPU we consume.
        if (d is None or dist > textlen * 4 or l > textlen or
            compresseddeltalen > textlen * 2 or
            (self._maxchainlen and chainlen > self._maxchainlen)):
            text = buildtext()
            data = self.compress(text)
            l = len(data[1]) + len(data[0])
            base = chainbase = curr

        e = (offset_type(offset, flags), l, textlen,
             base, link, p1r, p2r, node)
        self.index.insert(-1, e)
        self.nodemap[node] = curr

        entry = self._io.packentry(e, self.node, self.version, curr)
        self._writeentry(transaction, ifh, dfh, entry, data, link, offset)

        if type(text) == str: # only accept immutable objects
            self._cache = (node, curr, text)
        self._basecache = (curr, chainbase)
        return node

    def _writeentry(self, transaction, ifh, dfh, entry, data, link, offset):
        curr = len(self) - 1
        if not self._inline:
            transaction.add(self.datafile, offset)
            transaction.add(self.indexfile, curr * len(entry))
            if data[0]:
                dfh.write(data[0])
            dfh.write(data[1])
            dfh.flush()
            ifh.write(entry)
        else:
            offset += curr * self._io.size
            transaction.add(self.indexfile, offset, curr)
            ifh.write(entry)
            ifh.write(data[0])
            ifh.write(data[1])
            self.checkinlinesize(transaction, ifh)

    def addgroup(self, bundle, linkmapper, transaction):
        """
        add a delta group

        given a set of deltas, add them to the revision log. the
        first delta is against its parent, which should be in our
        log, the rest are against the previous delta.
        """

        # track the base of the current delta log
        content = []
        node = None

        r = len(self)
        end = 0
        if r:
            end = self.end(r - 1)
        ifh = self.opener(self.indexfile, "a+")
        isize = r * self._io.size
        if self._inline:
            transaction.add(self.indexfile, end + isize, r)
            dfh = None
        else:
            transaction.add(self.indexfile, isize, r)
            transaction.add(self.datafile, end)
            dfh = self.opener(self.datafile, "a")
        def flush():
            if dfh:
                dfh.flush()
            ifh.flush()
        try:
            # loop through our set of deltas
            chain = None
            while True:
                chunkdata = bundle.deltachunk(chain)
                if not chunkdata:
                    break
                node = chunkdata['node']
                p1 = chunkdata['p1']
                p2 = chunkdata['p2']
                cs = chunkdata['cs']
                deltabase = chunkdata['deltabase']
                delta = chunkdata['delta']

                content.append(node)

                link = linkmapper(cs)
                if node in self.nodemap:
                    # this can happen if two branches make the same change
                    chain = node
                    continue

                for p in (p1, p2):
                    if p not in self.nodemap:
                        raise LookupError(p, self.indexfile,
                                          _('unknown parent'))

                if deltabase not in self.nodemap:
                    raise LookupError(deltabase, self.indexfile,
                                      _('unknown delta base'))

                baserev = self.rev(deltabase)

                if baserev != nullrev and self.iscensored(baserev):
                    # if base is censored, delta must be full replacement in a
                    # single patch operation
                    hlen = struct.calcsize(">lll")
                    oldlen = self.rawsize(baserev)
                    newlen = len(delta) - hlen
                    if delta[:hlen] != mdiff.replacediffheader(oldlen, newlen):
                        raise error.CensoredBaseError(self.indexfile,
                                                      self.node(baserev))

                flags = REVIDX_DEFAULT_FLAGS
                if self._peek_iscensored(baserev, delta, flush):
                    flags |= REVIDX_ISCENSORED

                chain = self._addrevision(node, None, transaction, link,
                                          p1, p2, flags, (baserev, delta),
                                          ifh, dfh)
                if not dfh and not self._inline:
                    # addrevision switched from inline to conventional
                    # reopen the index
                    ifh.close()
                    dfh = self.opener(self.datafile, "a")
                    ifh = self.opener(self.indexfile, "a")
        finally:
            if dfh:
                dfh.close()
            ifh.close()

        return content

    def iscensored(self, rev):
        """Check if a file revision is censored."""
        return False

    def _peek_iscensored(self, baserev, delta, flush):
        """Quickly check if a delta produces a censored revision."""
        return False

    def getstrippoint(self, minlink):
        """find the minimum rev that must be stripped to strip the linkrev

        Returns a tuple containing the minimum rev and a set of all revs that
        have linkrevs that will be broken by this strip.
        """
        brokenrevs = set()
        strippoint = len(self)

        heads = {}
        futurelargelinkrevs = set()
        for head in self.headrevs():
            headlinkrev = self.linkrev(head)
            heads[head] = headlinkrev
            if headlinkrev >= minlink:
                futurelargelinkrevs.add(headlinkrev)

        # This algorithm involves walking down the rev graph, starting at the
        # heads. Since the revs are topologically sorted according to linkrev,
        # once all head linkrevs are below the minlink, we know there are
        # no more revs that could have a linkrev greater than minlink.
        # So we can stop walking.
        while futurelargelinkrevs:
            strippoint -= 1
            linkrev = heads.pop(strippoint)

            if linkrev < minlink:
                brokenrevs.add(strippoint)
            else:
                futurelargelinkrevs.remove(linkrev)

            for p in self.parentrevs(strippoint):
                if p != nullrev:
                    plinkrev = self.linkrev(p)
                    heads[p] = plinkrev
                    if plinkrev >= minlink:
                        futurelargelinkrevs.add(plinkrev)

        return strippoint, brokenrevs

    def strip(self, minlink, transaction):
        """truncate the revlog on the first revision with a linkrev >= minlink

        This function is called when we're stripping revision minlink and
        its descendants from the repository.

        We have to remove all revisions with linkrev >= minlink, because
        the equivalent changelog revisions will be renumbered after the
        strip.

        So we truncate the revlog on the first of these revisions, and
        trust that the caller has saved the revisions that shouldn't be
        removed and that it'll re-add them after this truncation.
        """
        if len(self) == 0:
            return

        rev, _ = self.getstrippoint(minlink)
        if rev == len(self):
            return

        # first truncate the files on disk
        end = self.start(rev)
        if not self._inline:
            transaction.add(self.datafile, end)
            end = rev * self._io.size
        else:
            end += rev * self._io.size

        transaction.add(self.indexfile, end)

        # then reset internal state in memory to forget those revisions
        self._cache = None
        self._chaininfocache = {}
        self._chunkclear()
        for x in xrange(rev, len(self)):
            del self.nodemap[self.node(x)]

        del self.index[rev:-1]

    def checksize(self):
        expected = 0
        if len(self):
            expected = max(0, self.end(len(self) - 1))

        try:
            f = self.opener(self.datafile)
            f.seek(0, 2)
            actual = f.tell()
            f.close()
            dd = actual - expected
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
            dd = 0

        try:
            f = self.opener(self.indexfile)
            f.seek(0, 2)
            actual = f.tell()
            f.close()
            s = self._io.size
            i = max(0, actual // s)
            di = actual - (i * s)
            if self._inline:
                databytes = 0
                for r in self:
                    databytes += max(0, self.length(r))
                dd = 0
                di = actual - len(self) * s - databytes
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
            di = 0

        return (dd, di)

    def files(self):
        res = [self.indexfile]
        if not self._inline:
            res.append(self.datafile)
        return res
