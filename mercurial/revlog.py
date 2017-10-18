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

from __future__ import absolute_import

import binascii
import collections
import errno
import hashlib
import heapq
import os
import struct
import zlib

# import stuff from node for others to import from revlog
from .node import (
    bin,
    hex,
    nullid,
    nullrev,
    wdirhex,
    wdirid,
    wdirrev,
)
from .i18n import _
from . import (
    ancestor,
    error,
    mdiff,
    policy,
    pycompat,
    templatefilters,
    util,
)

parsers = policy.importmod(r'parsers')

# Aliased for performance.
_zlibdecompress = zlib.decompress

# revlog header flags
REVLOGV0 = 0
REVLOGV1 = 1
# Dummy value until file format is finalized.
# Reminder: change the bounds check in revlog.__init__ when this is changed.
REVLOGV2 = 0xDEAD
FLAG_INLINE_DATA = (1 << 16)
FLAG_GENERALDELTA = (1 << 17)
REVLOG_DEFAULT_FLAGS = FLAG_INLINE_DATA
REVLOG_DEFAULT_FORMAT = REVLOGV1
REVLOG_DEFAULT_VERSION = REVLOG_DEFAULT_FORMAT | REVLOG_DEFAULT_FLAGS
REVLOGV1_FLAGS = FLAG_INLINE_DATA | FLAG_GENERALDELTA
REVLOGV2_FLAGS = REVLOGV1_FLAGS

# revlog index flags
REVIDX_ISCENSORED = (1 << 15) # revision has censor metadata, must be verified
REVIDX_ELLIPSIS = (1 << 14) # revision hash does not match data (narrowhg)
REVIDX_EXTSTORED = (1 << 13) # revision data is stored externally
REVIDX_DEFAULT_FLAGS = 0
# stable order in which flags need to be processed and their processors applied
REVIDX_FLAGS_ORDER = [
    REVIDX_ISCENSORED,
    REVIDX_ELLIPSIS,
    REVIDX_EXTSTORED,
]
REVIDX_KNOWN_FLAGS = util.bitsfrom(REVIDX_FLAGS_ORDER)

# max size of revlog with inline data
_maxinline = 131072
_chunksize = 1048576

RevlogError = error.RevlogError
LookupError = error.LookupError
CensoredNodeError = error.CensoredNodeError
ProgrammingError = error.ProgrammingError

# Store flag processors (cf. 'addflagprocessor()' to register)
_flagprocessors = {
    REVIDX_ISCENSORED: None,
}

def addflagprocessor(flag, processor):
    """Register a flag processor on a revision data flag.

    Invariant:
    - Flags need to be defined in REVIDX_KNOWN_FLAGS and REVIDX_FLAGS_ORDER.
    - Only one flag processor can be registered on a specific flag.
    - flagprocessors must be 3-tuples of functions (read, write, raw) with the
      following signatures:
          - (read)  f(self, rawtext) -> text, bool
          - (write) f(self, text) -> rawtext, bool
          - (raw)   f(self, rawtext) -> bool
      "text" is presented to the user. "rawtext" is stored in revlog data, not
      directly visible to the user.
      The boolean returned by these transforms is used to determine whether
      the returned text can be used for hash integrity checking. For example,
      if "write" returns False, then "text" is used to generate hash. If
      "write" returns True, that basically means "rawtext" returned by "write"
      should be used to generate hash. Usually, "write" and "read" return
      different booleans. And "raw" returns a same boolean as "write".

      Note: The 'raw' transform is used for changegroup generation and in some
      debug commands. In this case the transform only indicates whether the
      contents can be used for hash integrity checks.
    """
    if not flag & REVIDX_KNOWN_FLAGS:
        msg = _("cannot register processor on unknown flag '%#x'.") % (flag)
        raise ProgrammingError(msg)
    if flag not in REVIDX_FLAGS_ORDER:
        msg = _("flag '%#x' undefined in REVIDX_FLAGS_ORDER.") % (flag)
        raise ProgrammingError(msg)
    if flag in _flagprocessors:
        msg = _("cannot register multiple processors on flag '%#x'.") % (flag)
        raise error.Abort(msg)
    _flagprocessors[flag] = processor

def getoffset(q):
    return int(q >> 16)

def gettype(q):
    return int(q & 0xFFFF)

def offset_type(offset, type):
    if (type & ~REVIDX_KNOWN_FLAGS) != 0:
        raise ValueError('unknown revlog index flags')
    return int(int(offset) << 16 | type)

_nullhash = hashlib.sha1(nullid)

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
        if p1 < p2:
            a = p1
            b = p2
        else:
            a = p2
            b = p1
        s = hashlib.sha1(a)
        s.update(b)
    s.update(text)
    return s.digest()

def _slicechunk(revlog, revs):
    """slice revs to reduce the amount of unrelated data to be read from disk.

    ``revs`` is sliced into groups that should be read in one time.
    Assume that revs are sorted.
    """
    start = revlog.start
    length = revlog.length

    if len(revs) <= 1:
        yield revs
        return

    startbyte = start(revs[0])
    endbyte = start(revs[-1]) + length(revs[-1])
    readdata = deltachainspan = endbyte - startbyte

    chainpayload = sum(length(r) for r in revs)

    if deltachainspan:
        density = chainpayload / float(deltachainspan)
    else:
        density = 1.0

    # Store the gaps in a heap to have them sorted by decreasing size
    gapsheap = []
    heapq.heapify(gapsheap)
    prevend = None
    for i, rev in enumerate(revs):
        revstart = start(rev)
        revlen = length(rev)

        if prevend is not None:
            gapsize = revstart - prevend
            if gapsize:
                heapq.heappush(gapsheap, (-gapsize, i))

        prevend = revstart + revlen

    # Collect the indices of the largest holes until the density is acceptable
    indicesheap = []
    heapq.heapify(indicesheap)
    while gapsheap and density < revlog._srdensitythreshold:
        oppgapsize, gapidx = heapq.heappop(gapsheap)

        heapq.heappush(indicesheap, gapidx)

        # the gap sizes are stored as negatives to be sorted decreasingly
        # by the heap
        readdata -= (-oppgapsize)
        if readdata > 0:
            density = chainpayload / float(readdata)
        else:
            density = 1.0

    # Cut the revs at collected indices
    previdx = 0
    while indicesheap:
        idx = heapq.heappop(indicesheap)
        yield revs[previdx:idx]
        previdx = idx
    yield revs[previdx:]

# index v0:
#  4 bytes: offset
#  4 bytes: compressed length
#  4 bytes: base rev
#  4 bytes: link rev
# 20 bytes: parent 1 nodeid
# 20 bytes: parent 2 nodeid
# 20 bytes: nodeid
indexformatv0 = struct.Struct(">4l20s20s20s")
indexformatv0_pack = indexformatv0.pack
indexformatv0_unpack = indexformatv0.unpack

class revlogoldio(object):
    def __init__(self):
        self.size = indexformatv0.size

    def parseindex(self, data, inline):
        s = self.size
        index = []
        nodemap = {nullid: nullrev}
        n = off = 0
        l = len(data)
        while off + s <= l:
            cur = data[off:off + s]
            off += s
            e = indexformatv0_unpack(cur)
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
            raise RevlogError(_('index entry flags need revlog version 1'))
        e2 = (getoffset(entry[0]), entry[1], entry[3], entry[4],
              node(entry[5]), node(entry[6]), entry[7])
        return indexformatv0_pack(*e2)

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
indexformatng = struct.Struct(">Qiiiiii20s12x")
indexformatng_pack = indexformatng.pack
versionformat = struct.Struct(">I")
versionformat_pack = versionformat.pack
versionformat_unpack = versionformat.unpack

# corresponds to uncompressed length of indexformatng (2 gigs, 4-byte
# signed integer)
_maxentrysize = 0x7fffffff

class revlogio(object):
    def __init__(self):
        self.size = indexformatng.size

    def parseindex(self, data, inline):
        # call the C implementation to parse the index data
        index, cache = parsers.parse_index2(data, inline)
        return index, getattr(index, 'nodemap', None), cache

    def packentry(self, entry, node, version, rev):
        p = indexformatng_pack(*entry)
        if rev == 0:
            p = versionformat_pack(version) + p[4:]
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

    If checkambig, indexfile is opened with checkambig=True at
    writing, to avoid file stat ambiguity.

    If mmaplargeindex is True, and an mmapindexthreshold is set, the
    index will be mmapped rather than read if it is larger than the
    configured threshold.
    """
    def __init__(self, opener, indexfile, datafile=None, checkambig=False,
                 mmaplargeindex=False):
        """
        create a revlog object

        opener is a function that abstracts the file opening operation
        and can be used to implement COW semantics or the like.
        """
        self.indexfile = indexfile
        self.datafile = datafile or (indexfile[:-2] + ".d")
        self.opener = opener
        #  When True, indexfile is opened with checkambig=True at writing, to
        #  avoid file stat ambiguity.
        self._checkambig = checkambig
        # 3-tuple of (node, rev, text) for a raw revision.
        self._cache = None
        # Maps rev to chain base rev.
        self._chainbasecache = util.lrucachedict(100)
        # 2-tuple of (offset, data) of raw data from the revlog at an offset.
        self._chunkcache = (0, '')
        # How much data to read and cache into the raw revlog data cache.
        self._chunkcachesize = 65536
        self._maxchainlen = None
        self._aggressivemergedeltas = False
        self.index = []
        # Mapping of partial identifiers to full nodes.
        self._pcache = {}
        # Mapping of revision integer to full node.
        self._nodecache = {nullid: nullrev}
        self._nodepos = None
        self._compengine = 'zlib'
        self._maxdeltachainspan = -1
        self._withsparseread = False
        self._srdensitythreshold = 0.25
        self._srminblocksize = 262144

        mmapindexthreshold = None
        v = REVLOG_DEFAULT_VERSION
        opts = getattr(opener, 'options', None)
        if opts is not None:
            if 'revlogv2' in opts:
                # version 2 revlogs always use generaldelta.
                v = REVLOGV2 | FLAG_GENERALDELTA | FLAG_INLINE_DATA
            elif 'revlogv1' in opts:
                if 'generaldelta' in opts:
                    v |= FLAG_GENERALDELTA
            else:
                v = 0
            if 'chunkcachesize' in opts:
                self._chunkcachesize = opts['chunkcachesize']
            if 'maxchainlen' in opts:
                self._maxchainlen = opts['maxchainlen']
            if 'aggressivemergedeltas' in opts:
                self._aggressivemergedeltas = opts['aggressivemergedeltas']
            self._lazydeltabase = bool(opts.get('lazydeltabase', False))
            if 'compengine' in opts:
                self._compengine = opts['compengine']
            if 'maxdeltachainspan' in opts:
                self._maxdeltachainspan = opts['maxdeltachainspan']
            if mmaplargeindex and 'mmapindexthreshold' in opts:
                mmapindexthreshold = opts['mmapindexthreshold']
            self._withsparseread = bool(opts.get('with-sparse-read', False))
            if 'sparse-read-density-threshold' in opts:
                self._srdensitythreshold = opts['sparse-read-density-threshold']
            if 'sparse-read-min-block-size' in opts:
                self._srminblocksize = opts['sparse-read-min-block-size']

        if self._chunkcachesize <= 0:
            raise RevlogError(_('revlog chunk cache size %r is not greater '
                                'than 0') % self._chunkcachesize)
        elif self._chunkcachesize & (self._chunkcachesize - 1):
            raise RevlogError(_('revlog chunk cache size %r is not a power '
                                'of 2') % self._chunkcachesize)

        indexdata = ''
        self._initempty = True
        try:
            f = self.opener(self.indexfile)
            if (mmapindexthreshold is not None and
                    self.opener.fstat(f).st_size >= mmapindexthreshold):
                indexdata = util.buffer(util.mmapread(f))
            else:
                indexdata = f.read()
            f.close()
            if len(indexdata) > 0:
                v = versionformat_unpack(indexdata[:4])[0]
                self._initempty = False
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise

        self.version = v
        self._inline = v & FLAG_INLINE_DATA
        self._generaldelta = v & FLAG_GENERALDELTA
        flags = v & ~0xFFFF
        fmt = v & 0xFFFF
        if fmt == REVLOGV0:
            if flags:
                raise RevlogError(_('unknown flags (%#04x) in version %d '
                                    'revlog %s') %
                                  (flags >> 16, fmt, self.indexfile))
        elif fmt == REVLOGV1:
            if flags & ~REVLOGV1_FLAGS:
                raise RevlogError(_('unknown flags (%#04x) in version %d '
                                    'revlog %s') %
                                  (flags >> 16, fmt, self.indexfile))
        elif fmt == REVLOGV2:
            if flags & ~REVLOGV2_FLAGS:
                raise RevlogError(_('unknown flags (%#04x) in version %d '
                                    'revlog %s') %
                                  (flags >> 16, fmt, self.indexfile))
        else:
            raise RevlogError(_('unknown version (%d) in revlog %s') %
                              (fmt, self.indexfile))

        self.storedeltachains = True

        self._io = revlogio()
        if self.version == REVLOGV0:
            self._io = revlogoldio()
        try:
            d = self._io.parseindex(indexdata, self._inline)
        except (ValueError, IndexError):
            raise RevlogError(_("index %s is corrupted") % (self.indexfile))
        self.index, nodemap, self._chunkcache = d
        if nodemap is not None:
            self.nodemap = self._nodecache = nodemap
        if not self._chunkcache:
            self._chunkclear()
        # revnum -> (chain-length, sum-delta-length)
        self._chaininfocache = {}
        # revlog header -> revlog compressor
        self._decompressors = {}

    @util.propertycache
    def _compressor(self):
        return util.compengines[self._compengine].revlogcompressor()

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
        self._cache = None
        self._chainbasecache.clear()
        self._chunkcache = (0, '')
        self._pcache = {}

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
            if node == wdirid:
                raise error.WdirUnsupported
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
            if node == wdirid:
                raise error.WdirUnsupported
            raise LookupError(node, self.indexfile, _('no node'))

    # Accessors for index entries.

    # First tuple entry is 8 bytes. First 6 bytes are offset. Last 2 bytes
    # are flags.
    def start(self, rev):
        return int(self.index[rev][0] >> 16)

    def flags(self, rev):
        return self.index[rev][0] & 0xFFFF

    def length(self, rev):
        return self.index[rev][1]

    def rawsize(self, rev):
        """return the length of the uncompressed text for a given revision"""
        l = self.index[rev][2]
        if l >= 0:
            return l

        t = self.revision(rev, raw=True)
        return len(t)

    def size(self, rev):
        """length of non-raw text (processed by a "read" flag processor)"""
        # fast path: if no "read" flag processor could change the content,
        # size is rawsize. note: ELLIPSIS is known to not change the content.
        flags = self.flags(rev)
        if flags & (REVIDX_KNOWN_FLAGS ^ REVIDX_ELLIPSIS) == 0:
            return self.rawsize(rev)

        return len(self.revision(rev, raw=False))

    def chainbase(self, rev):
        base = self._chainbasecache.get(rev)
        if base is not None:
            return base

        index = self.index
        base = index[rev][3]
        while base != rev:
            rev = base
            base = index[rev][3]

        self._chainbasecache[rev] = base
        return base

    def linkrev(self, rev):
        return self.index[rev][4]

    def parentrevs(self, rev):
        try:
            return self.index[rev][5:7]
        except IndexError:
            if rev == wdirrev:
                raise error.WdirUnsupported
            raise

    def node(self, rev):
        try:
            return self.index[rev][7]
        except IndexError:
            if rev == wdirrev:
                raise error.WdirUnsupported
            raise

    # Derived from index values.

    def end(self, rev):
        return self.start(rev) + self.length(rev)

    def parents(self, node):
        i = self.index
        d = i[self.rev(node)]
        return i[d[5]][7], i[d[6]][7] # map revisions to nodes inline

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

    def _deltachain(self, rev, stoprev=None):
        """Obtain the delta chain for a revision.

        ``stoprev`` specifies a revision to stop at. If not specified, we
        stop at the base of the chain.

        Returns a 2-tuple of (chain, stopped) where ``chain`` is a list of
        revs in ascending order and ``stopped`` is a bool indicating whether
        ``stoprev`` was hit.
        """
        # Try C implementation.
        try:
            return self.index.deltachain(rev, stoprev, self._generaldelta)
        except AttributeError:
            pass

        chain = []

        # Alias to prevent attribute lookup in tight loop.
        index = self.index
        generaldelta = self._generaldelta

        iterrev = rev
        e = index[iterrev]
        while iterrev != e[3] and iterrev != stoprev:
            chain.append(iterrev)
            if generaldelta:
                iterrev = e[3]
            else:
                iterrev -= 1
            e = index[iterrev]

        if iterrev == stoprev:
            stopped = True
        else:
            chain.append(iterrev)
            stopped = False

        chain.reverse()
        return chain, stopped

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
        return has, [self.node(miss) for miss in missing]

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
                roots = [root for root in roots if root in ancestors]
                # Recompute the lowest revision
                if roots:
                    lowestrev = min([self.rev(root) for root in roots])
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
        heads = [head for head, flag in heads.iteritems() if flag]
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
        reachable = {startrev}
        heads = {startrev}

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
        return pycompat.maplist(self.node, ancs)

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
        maybewdir = wdirhex.startswith(id)
        try:
            partial = self.index.partialmatch(id)
            if partial and self.hasnode(partial):
                if maybewdir:
                    # single 'ff...' match in radix tree, ambiguous with wdir
                    raise RevlogError
                return partial
            if maybewdir:
                # no 'ff...' match in radix tree, wdir identified
                raise error.WdirUnsupported
            return None
        except RevlogError:
            # parsers.c radix tree lookup gave multiple matches
            # fast path: for unfiltered changelog, radix tree is accurate
            if not getattr(self, 'filteredrevs', None):
                raise LookupError(id, self.indexfile,
                                  _('ambiguous identifier'))
            # fall through to slow path that filters hidden revisions
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
                    if len(nl) == 1 and not maybewdir:
                        self._pcache[id] = nl[0]
                        return nl[0]
                    raise LookupError(id, self.indexfile,
                                      _('ambiguous identifier'))
                if maybewdir:
                    raise error.WdirUnsupported
                return None
            except (TypeError, binascii.Error):
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

    def shortest(self, hexnode, minlength=1):
        """Find the shortest unambiguous prefix that matches hexnode."""
        def isvalid(test):
            try:
                if self._partialmatch(test) is None:
                    return False

                try:
                    i = int(test)
                    # if we are a pure int, then starting with zero will not be
                    # confused as a rev; or, obviously, if the int is larger
                    # than the value of the tip rev
                    if test[0] == '0' or i > len(self):
                        return True
                    return False
                except ValueError:
                    return True
            except error.RevlogError:
                return False
            except error.WdirUnsupported:
                # single 'ff...' match
                return True

        shortest = hexnode
        startlength = max(6, minlength)
        length = startlength
        while True:
            test = hexnode[:length]
            if isvalid(test):
                shortest = test
                if length == minlength or length > startlength:
                    return shortest
                length -= 1
            else:
                length += 1
                if len(shortest) <= length:
                    return shortest

    def cmp(self, node, text):
        """compare text with a given file revision

        returns True if text is different than what is stored.
        """
        p1, p2 = self.parents(node)
        return hash(text, p1, p2) != node

    def _cachesegment(self, offset, data):
        """Add a segment to the revlog cache.

        Accepts an absolute offset and the data that is at that location.
        """
        o, d = self._chunkcache
        # try to add to existing cache
        if o + len(d) == offset and len(d) + len(data) < _chunksize:
            self._chunkcache = o, d + data
        else:
            self._chunkcache = offset, data

    def _readsegment(self, offset, length, df=None):
        """Load a segment of raw data from the revlog.

        Accepts an absolute offset, length to read, and an optional existing
        file handle to read from.

        If an existing file handle is passed, it will be seeked and the
        original seek position will NOT be restored.

        Returns a str or buffer of raw byte data.
        """
        if df is not None:
            closehandle = False
        else:
            if self._inline:
                df = self.opener(self.indexfile)
            else:
                df = self.opener(self.datafile)
            closehandle = True

        # Cache data both forward and backward around the requested
        # data, in a fixed size window. This helps speed up operations
        # involving reading the revlog backwards.
        cachesize = self._chunkcachesize
        realoffset = offset & ~(cachesize - 1)
        reallength = (((offset + length + cachesize) & ~(cachesize - 1))
                      - realoffset)
        df.seek(realoffset)
        d = df.read(reallength)
        if closehandle:
            df.close()
        self._cachesegment(realoffset, d)
        if offset != realoffset or reallength != length:
            return util.buffer(d, offset - realoffset, length)
        return d

    def _getsegment(self, offset, length, df=None):
        """Obtain a segment of raw data from the revlog.

        Accepts an absolute offset, length of bytes to obtain, and an
        optional file handle to the already-opened revlog. If the file
        handle is used, it's original seek position will not be preserved.

        Requests for data may be returned from a cache.

        Returns a str or a buffer instance of raw byte data.
        """
        o, d = self._chunkcache
        l = len(d)

        # is it in the cache?
        cachestart = offset - o
        cacheend = cachestart + length
        if cachestart >= 0 and cacheend <= l:
            if cachestart == 0 and cacheend == l:
                return d # avoid a copy
            return util.buffer(d, cachestart, cacheend - cachestart)

        return self._readsegment(offset, length, df=df)

    def _getsegmentforrevs(self, startrev, endrev, df=None):
        """Obtain a segment of raw data corresponding to a range of revisions.

        Accepts the start and end revisions and an optional already-open
        file handle to be used for reading. If the file handle is read, its
        seek position will not be preserved.

        Requests for data may be satisfied by a cache.

        Returns a 2-tuple of (offset, data) for the requested range of
        revisions. Offset is the integer offset from the beginning of the
        revlog and data is a str or buffer of the raw byte data.

        Callers will need to call ``self.start(rev)`` and ``self.length(rev)``
        to determine where each revision's data begins and ends.
        """
        # Inlined self.start(startrev) & self.end(endrev) for perf reasons
        # (functions are expensive).
        index = self.index
        istart = index[startrev]
        start = int(istart[0] >> 16)
        if startrev == endrev:
            end = start + istart[1]
        else:
            iend = index[endrev]
            end = int(iend[0] >> 16) + iend[1]

        if self._inline:
            start += (startrev + 1) * self._io.size
            end += (endrev + 1) * self._io.size
        length = end - start

        return start, self._getsegment(start, length, df=df)

    def _chunk(self, rev, df=None):
        """Obtain a single decompressed chunk for a revision.

        Accepts an integer revision and an optional already-open file handle
        to be used for reading. If used, the seek position of the file will not
        be preserved.

        Returns a str holding uncompressed data for the requested revision.
        """
        return self.decompress(self._getsegmentforrevs(rev, rev, df=df)[1])

    def _chunks(self, revs, df=None):
        """Obtain decompressed chunks for the specified revisions.

        Accepts an iterable of numeric revisions that are assumed to be in
        ascending order. Also accepts an optional already-open file handle
        to be used for reading. If used, the seek position of the file will
        not be preserved.

        This function is similar to calling ``self._chunk()`` multiple times,
        but is faster.

        Returns a list with decompressed data for each requested revision.
        """
        if not revs:
            return []
        start = self.start
        length = self.length
        inline = self._inline
        iosize = self._io.size
        buffer = util.buffer

        l = []
        ladd = l.append

        if not self._withsparseread:
            slicedchunks = (revs,)
        else:
            slicedchunks = _slicechunk(self, revs)

        for revschunk in slicedchunks:
            firstrev = revschunk[0]
            # Skip trailing revisions with empty diff
            for lastrev in revschunk[::-1]:
                if length(lastrev) != 0:
                    break

            try:
                offset, data = self._getsegmentforrevs(firstrev, lastrev, df=df)
            except OverflowError:
                # issue4215 - we can't cache a run of chunks greater than
                # 2G on Windows
                return [self._chunk(rev, df=df) for rev in revschunk]

            decomp = self.decompress
            for rev in revschunk:
                chunkstart = start(rev)
                if inline:
                    chunkstart += (rev + 1) * iosize
                chunklength = length(rev)
                ladd(decomp(buffer(data, chunkstart - offset, chunklength)))

        return l

    def _chunkclear(self):
        """Clear the raw chunk cache."""
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
        """return or calculate a delta between two revisions

        The delta calculated is in binary form and is intended to be written to
        revlog data directly. So this function needs raw revision data.
        """
        if rev1 != nullrev and self.deltaparent(rev2) == rev1:
            return bytes(self._chunk(rev2))

        return mdiff.textdiff(self.revision(rev1, raw=True),
                              self.revision(rev2, raw=True))

    def revision(self, nodeorrev, _df=None, raw=False):
        """return an uncompressed revision of a given node or revision
        number.

        _df - an existing file handle to read from. (internal-only)
        raw - an optional argument specifying if the revision data is to be
        treated as raw data when applying flag transforms. 'raw' should be set
        to True when generating changegroups or in debug commands.
        """
        if isinstance(nodeorrev, int):
            rev = nodeorrev
            node = self.node(rev)
        else:
            node = nodeorrev
            rev = None

        cachedrev = None
        flags = None
        rawtext = None
        if node == nullid:
            return ""
        if self._cache:
            if self._cache[0] == node:
                # _cache only stores rawtext
                if raw:
                    return self._cache[2]
                # duplicated, but good for perf
                if rev is None:
                    rev = self.rev(node)
                if flags is None:
                    flags = self.flags(rev)
                # no extra flags set, no flag processor runs, text = rawtext
                if flags == REVIDX_DEFAULT_FLAGS:
                    return self._cache[2]
                # rawtext is reusable. need to run flag processor
                rawtext = self._cache[2]

            cachedrev = self._cache[1]

        # look up what we need to read
        if rawtext is None:
            if rev is None:
                rev = self.rev(node)

            chain, stopped = self._deltachain(rev, stoprev=cachedrev)
            if stopped:
                rawtext = self._cache[2]

            # drop cache to save memory
            self._cache = None

            bins = self._chunks(chain, df=_df)
            if rawtext is None:
                rawtext = bytes(bins[0])
                bins = bins[1:]

            rawtext = mdiff.patches(rawtext, bins)
            self._cache = (node, rev, rawtext)

        if flags is None:
            if rev is None:
                rev = self.rev(node)
            flags = self.flags(rev)

        text, validatehash = self._processflags(rawtext, flags, 'read', raw=raw)
        if validatehash:
            self.checkhash(text, node, rev=rev)

        return text

    def hash(self, text, p1, p2):
        """Compute a node hash.

        Available as a function so that subclasses can replace the hash
        as needed.
        """
        return hash(text, p1, p2)

    def _processflags(self, text, flags, operation, raw=False):
        """Inspect revision data flags and applies transforms defined by
        registered flag processors.

        ``text`` - the revision data to process
        ``flags`` - the revision flags
        ``operation`` - the operation being performed (read or write)
        ``raw`` - an optional argument describing if the raw transform should be
        applied.

        This method processes the flags in the order (or reverse order if
        ``operation`` is 'write') defined by REVIDX_FLAGS_ORDER, applying the
        flag processors registered for present flags. The order of flags defined
        in REVIDX_FLAGS_ORDER needs to be stable to allow non-commutativity.

        Returns a 2-tuple of ``(text, validatehash)`` where ``text`` is the
        processed text and ``validatehash`` is a bool indicating whether the
        returned text should be checked for hash integrity.

        Note: If the ``raw`` argument is set, it has precedence over the
        operation and will only update the value of ``validatehash``.
        """
        # fast path: no flag processors will run
        if flags == 0:
            return text, True
        if not operation in ('read', 'write'):
            raise ProgrammingError(_("invalid '%s' operation ") % (operation))
        # Check all flags are known.
        if flags & ~REVIDX_KNOWN_FLAGS:
            raise RevlogError(_("incompatible revision flag '%#x'") %
                              (flags & ~REVIDX_KNOWN_FLAGS))
        validatehash = True
        # Depending on the operation (read or write), the order might be
        # reversed due to non-commutative transforms.
        orderedflags = REVIDX_FLAGS_ORDER
        if operation == 'write':
            orderedflags = reversed(orderedflags)

        for flag in orderedflags:
            # If a flagprocessor has been registered for a known flag, apply the
            # related operation transform and update result tuple.
            if flag & flags:
                vhash = True

                if flag not in _flagprocessors:
                    message = _("missing processor for flag '%#x'") % (flag)
                    raise RevlogError(message)

                processor = _flagprocessors[flag]
                if processor is not None:
                    readtransform, writetransform, rawtransform = processor

                    if raw:
                        vhash = rawtransform(self, text)
                    elif operation == 'read':
                        text, vhash = readtransform(self, text)
                    else: # write operation
                        text, vhash = writetransform(self, text)
                validatehash = validatehash and vhash

        return text, validatehash

    def checkhash(self, text, node, p1=None, p2=None, rev=None):
        """Check node hash integrity.

        Available as a function so that subclasses can extend hash mismatch
        behaviors as needed.
        """
        if p1 is None and p2 is None:
            p1, p2 = self.parents(node)
        if node != self.hash(text, p1, p2):
            revornode = rev
            if revornode is None:
                revornode = templatefilters.short(hex(node))
            raise RevlogError(_("integrity check failed on %s:%s")
                % (self.indexfile, pycompat.bytestr(revornode)))

    def checkinlinesize(self, tr, fp=None):
        """Check if the revlog is too big for inline and convert if so.

        This should be called after revisions are added to the revlog. If the
        revlog has grown too large to be an inline revlog, it will convert it
        to use multiple index and data files.
        """
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
                df.write(self._getsegmentforrevs(r, r)[1])
        finally:
            df.close()

        fp = self.opener(self.indexfile, 'w', atomictemp=True,
                         checkambig=self._checkambig)
        self.version &= ~FLAG_INLINE_DATA
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
                    node=None, flags=REVIDX_DEFAULT_FLAGS):
        """add a revision to the log

        text - the revision data to add
        transaction - the transaction object used for rollback
        link - the linkrev data to add
        p1, p2 - the parent nodeids of the revision
        cachedelta - an optional precomputed delta
        node - nodeid of revision; typically node is not specified, and it is
            computed by default as hash(text, p1, p2), however subclasses might
            use different hashing method (and override checkhash() in such case)
        flags - the known flags to set on the revision
        """
        if link == nullrev:
            raise RevlogError(_("attempted to add linkrev -1 to %s")
                              % self.indexfile)

        if flags:
            node = node or self.hash(text, p1, p2)

        rawtext, validatehash = self._processflags(text, flags, 'write')

        # If the flag processor modifies the revision data, ignore any provided
        # cachedelta.
        if rawtext != text:
            cachedelta = None

        if len(rawtext) > _maxentrysize:
            raise RevlogError(
                _("%s: size of %d bytes exceeds maximum revlog storage of 2GiB")
                % (self.indexfile, len(rawtext)))

        node = node or self.hash(rawtext, p1, p2)
        if node in self.nodemap:
            return node

        if validatehash:
            self.checkhash(rawtext, node, p1=p1, p2=p2)

        return self.addrawrevision(rawtext, transaction, link, p1, p2, node,
                                   flags, cachedelta=cachedelta)

    def addrawrevision(self, rawtext, transaction, link, p1, p2, node, flags,
                       cachedelta=None):
        """add a raw revision with known flags, node and parents
        useful when reusing a revision not stored in this revlog (ex: received
        over wire, or read from an external bundle).
        """
        dfh = None
        if not self._inline:
            dfh = self.opener(self.datafile, "a+")
        ifh = self.opener(self.indexfile, "a+", checkambig=self._checkambig)
        try:
            return self._addrevision(node, rawtext, transaction, link, p1, p2,
                                     flags, cachedelta, ifh, dfh)
        finally:
            if dfh:
                dfh.close()
            ifh.close()

    def compress(self, data):
        """Generate a possibly-compressed representation of data."""
        if not data:
            return '', data

        compressed = self._compressor.compress(data)

        if compressed:
            # The revlog compressor added the header in the returned data.
            return '', compressed

        if data[0:1] == '\0':
            return '', data
        return 'u', data

    def decompress(self, data):
        """Decompress a revlog chunk.

        The chunk is expected to begin with a header identifying the
        format type so it can be routed to an appropriate decompressor.
        """
        if not data:
            return data

        # Revlogs are read much more frequently than they are written and many
        # chunks only take microseconds to decompress, so performance is
        # important here.
        #
        # We can make a few assumptions about revlogs:
        #
        # 1) the majority of chunks will be compressed (as opposed to inline
        #    raw data).
        # 2) decompressing *any* data will likely by at least 10x slower than
        #    returning raw inline data.
        # 3) we want to prioritize common and officially supported compression
        #    engines
        #
        # It follows that we want to optimize for "decompress compressed data
        # when encoded with common and officially supported compression engines"
        # case over "raw data" and "data encoded by less common or non-official
        # compression engines." That is why we have the inline lookup first
        # followed by the compengines lookup.
        #
        # According to `hg perfrevlogchunks`, this is ~0.5% faster for zlib
        # compressed chunks. And this matters for changelog and manifest reads.
        t = data[0:1]

        if t == 'x':
            try:
                return _zlibdecompress(data)
            except zlib.error as e:
                raise RevlogError(_('revlog decompress error: %s') % str(e))
        # '\0' is more common than 'u' so it goes first.
        elif t == '\0':
            return data
        elif t == 'u':
            return util.buffer(data, 1)

        try:
            compressor = self._decompressors[t]
        except KeyError:
            try:
                engine = util.compengines.forrevlogheader(t)
                compressor = engine.revlogcompressor()
                self._decompressors[t] = compressor
            except KeyError:
                raise RevlogError(_('unknown compression type %r') % t)

        return compressor.decompress(data)

    def _isgooddelta(self, d, textlen):
        """Returns True if the given delta is good. Good means that it is within
        the disk span, disk size, and chain length bounds that we know to be
        performant."""
        if d is None:
            return False

        # - 'dist' is the distance from the base revision -- bounding it limits
        #   the amount of I/O we need to do.
        # - 'compresseddeltalen' is the sum of the total size of deltas we need
        #   to apply -- bounding it limits the amount of CPU we consume.
        dist, l, data, base, chainbase, chainlen, compresseddeltalen = d

        defaultmax = textlen * 4
        maxdist = self._maxdeltachainspan
        if not maxdist:
            maxdist = dist # ensure the conditional pass
        maxdist = max(maxdist, defaultmax)
        if (dist > maxdist or l > textlen or
            compresseddeltalen > textlen * 2 or
            (self._maxchainlen and chainlen > self._maxchainlen)):
            return False

        return True

    def _addrevision(self, node, rawtext, transaction, link, p1, p2, flags,
                     cachedelta, ifh, dfh, alwayscache=False):
        """internal function to add revisions to the log

        see addrevision for argument descriptions.

        note: "addrevision" takes non-raw text, "_addrevision" takes raw text.

        invariants:
        - rawtext is optional (can be None); if not set, cachedelta must be set.
          if both are set, they must correspond to each other.
        """
        if node == nullid:
            raise RevlogError(_("%s: attempt to add null revision") %
                              (self.indexfile))
        if node == wdirid:
            raise RevlogError(_("%s: attempt to add wdir revision") %
                              (self.indexfile))

        btext = [rawtext]
        def buildtext():
            if btext[0] is not None:
                return btext[0]
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
                if self._inline:
                    fh = ifh
                else:
                    fh = dfh
                basetext = self.revision(baserev, _df=fh, raw=True)
                btext[0] = mdiff.patch(basetext, delta)

            try:
                res = self._processflags(btext[0], flags, 'read', raw=True)
                btext[0], validatehash = res
                if validatehash:
                    self.checkhash(btext[0], node, p1=p1, p2=p2)
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
                    if self._inline:
                        fh = ifh
                    else:
                        fh = dfh
                    ptext = self.revision(rev, _df=fh, raw=True)
                    delta = mdiff.textdiff(ptext, t)
            header, data = self.compress(delta)
            deltalen = len(header) + len(data)
            chainbase = self.chainbase(rev)
            dist = deltalen + offset - self.start(chainbase)
            if self._generaldelta:
                base = rev
            else:
                base = chainbase
            chainlen, compresseddeltalen = self._chaininfo(rev)
            chainlen += 1
            compresseddeltalen += deltalen
            return (dist, deltalen, (header, data), base,
                    chainbase, chainlen, compresseddeltalen)

        curr = len(self)
        prev = curr - 1
        offset = self.end(prev)
        delta = None
        p1r, p2r = self.rev(p1), self.rev(p2)

        # full versions are inserted when the needed deltas
        # become comparable to the uncompressed text
        if rawtext is None:
            textlen = mdiff.patchedsize(self.rawsize(cachedelta[0]),
                                        cachedelta[1])
        else:
            textlen = len(rawtext)

        # should we try to build a delta?
        if prev != nullrev and self.storedeltachains:
            tested = set()
            # This condition is true most of the time when processing
            # changegroup data into a generaldelta repo. The only time it
            # isn't true is if this is the first revision in a delta chain
            # or if ``format.generaldelta=true`` disabled ``lazydeltabase``.
            if cachedelta and self._generaldelta and self._lazydeltabase:
                # Assume what we received from the server is a good choice
                # build delta will reuse the cache
                candidatedelta = builddelta(cachedelta[0])
                tested.add(cachedelta[0])
                if self._isgooddelta(candidatedelta, textlen):
                    delta = candidatedelta
            if delta is None and self._generaldelta:
                # exclude already lazy tested base if any
                parents = [p for p in (p1r, p2r)
                           if p != nullrev and p not in tested]
                if parents and not self._aggressivemergedeltas:
                    # Pick whichever parent is closer to us (to minimize the
                    # chance of having to build a fulltext).
                    parents = [max(parents)]
                tested.update(parents)
                pdeltas = []
                for p in parents:
                    pd = builddelta(p)
                    if self._isgooddelta(pd, textlen):
                        pdeltas.append(pd)
                if pdeltas:
                    delta = min(pdeltas, key=lambda x: x[1])
            if delta is None and prev not in tested:
                # other approach failed try against prev to hopefully save us a
                # fulltext.
                candidatedelta = builddelta(prev)
                if self._isgooddelta(candidatedelta, textlen):
                    delta = candidatedelta
        if delta is not None:
            dist, l, data, base, chainbase, chainlen, compresseddeltalen = delta
        else:
            rawtext = buildtext()
            data = self.compress(rawtext)
            l = len(data[1]) + len(data[0])
            base = chainbase = curr

        e = (offset_type(offset, flags), l, textlen,
             base, link, p1r, p2r, node)
        self.index.insert(-1, e)
        self.nodemap[node] = curr

        entry = self._io.packentry(e, self.node, self.version, curr)
        self._writeentry(transaction, ifh, dfh, entry, data, link, offset)

        if alwayscache and rawtext is None:
            rawtext = buildtext()

        if type(rawtext) == str: # only accept immutable objects
            self._cache = (node, curr, rawtext)
        self._chainbasecache[curr] = chainbase
        return node

    def _writeentry(self, transaction, ifh, dfh, entry, data, link, offset):
        # Files opened in a+ mode have inconsistent behavior on various
        # platforms. Windows requires that a file positioning call be made
        # when the file handle transitions between reads and writes. See
        # 3686fa2b8eee and the mixedfilemodewrapper in windows.py. On other
        # platforms, Python or the platform itself can be buggy. Some versions
        # of Solaris have been observed to not append at the end of the file
        # if the file was seeked to before the end. See issue4943 for more.
        #
        # We work around this issue by inserting a seek() before writing.
        # Note: This is likely not necessary on Python 3.
        ifh.seek(0, os.SEEK_END)
        if dfh:
            dfh.seek(0, os.SEEK_END)

        curr = len(self) - 1
        if not self._inline:
            transaction.add(self.datafile, offset)
            transaction.add(self.indexfile, curr * len(entry))
            if data[0]:
                dfh.write(data[0])
            dfh.write(data[1])
            ifh.write(entry)
        else:
            offset += curr * self._io.size
            transaction.add(self.indexfile, offset, curr)
            ifh.write(entry)
            ifh.write(data[0])
            ifh.write(data[1])
            self.checkinlinesize(transaction, ifh)

    def addgroup(self, deltas, linkmapper, transaction, addrevisioncb=None):
        """
        add a delta group

        given a set of deltas, add them to the revision log. the
        first delta is against its parent, which should be in our
        log, the rest are against the previous delta.

        If ``addrevisioncb`` is defined, it will be called with arguments of
        this revlog and the node that was added.
        """

        nodes = []

        r = len(self)
        end = 0
        if r:
            end = self.end(r - 1)
        ifh = self.opener(self.indexfile, "a+", checkambig=self._checkambig)
        isize = r * self._io.size
        if self._inline:
            transaction.add(self.indexfile, end + isize, r)
            dfh = None
        else:
            transaction.add(self.indexfile, isize, r)
            transaction.add(self.datafile, end)
            dfh = self.opener(self.datafile, "a+")
        def flush():
            if dfh:
                dfh.flush()
            ifh.flush()
        try:
            # loop through our set of deltas
            for data in deltas:
                node, p1, p2, linknode, deltabase, delta, flags = data
                link = linkmapper(linknode)
                flags = flags or REVIDX_DEFAULT_FLAGS

                nodes.append(node)

                if node in self.nodemap:
                    # this can happen if two branches make the same change
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

                if not flags and self._peek_iscensored(baserev, delta, flush):
                    flags |= REVIDX_ISCENSORED

                # We assume consumers of addrevisioncb will want to retrieve
                # the added revision, which will require a call to
                # revision(). revision() will fast path if there is a cache
                # hit. So, we tell _addrevision() to always cache in this case.
                # We're only using addgroup() in the context of changegroup
                # generation so the revision data can always be handled as raw
                # by the flagprocessor.
                self._addrevision(node, None, transaction, link,
                                  p1, p2, flags, (baserev, delta),
                                  ifh, dfh,
                                  alwayscache=bool(addrevisioncb))

                if addrevisioncb:
                    addrevisioncb(self, node)

                if not dfh and not self._inline:
                    # addrevision switched from inline to conventional
                    # reopen the index
                    ifh.close()
                    dfh = self.opener(self.datafile, "a+")
                    ifh = self.opener(self.indexfile, "a+",
                                      checkambig=self._checkambig)
        finally:
            if dfh:
                dfh.close()
            ifh.close()

        return nodes

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
        except IOError as inst:
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
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            di = 0

        return (dd, di)

    def files(self):
        res = [self.indexfile]
        if not self._inline:
            res.append(self.datafile)
        return res

    DELTAREUSEALWAYS = 'always'
    DELTAREUSESAMEREVS = 'samerevs'
    DELTAREUSENEVER = 'never'

    DELTAREUSEALL = {'always', 'samerevs', 'never'}

    def clone(self, tr, destrevlog, addrevisioncb=None,
              deltareuse=DELTAREUSESAMEREVS, aggressivemergedeltas=None):
        """Copy this revlog to another, possibly with format changes.

        The destination revlog will contain the same revisions and nodes.
        However, it may not be bit-for-bit identical due to e.g. delta encoding
        differences.

        The ``deltareuse`` argument control how deltas from the existing revlog
        are preserved in the destination revlog. The argument can have the
        following values:

        DELTAREUSEALWAYS
           Deltas will always be reused (if possible), even if the destination
           revlog would not select the same revisions for the delta. This is the
           fastest mode of operation.
        DELTAREUSESAMEREVS
           Deltas will be reused if the destination revlog would pick the same
           revisions for the delta. This mode strikes a balance between speed
           and optimization.
        DELTAREUSENEVER
           Deltas will never be reused. This is the slowest mode of execution.
           This mode can be used to recompute deltas (e.g. if the diff/delta
           algorithm changes).

        Delta computation can be slow, so the choice of delta reuse policy can
        significantly affect run time.

        The default policy (``DELTAREUSESAMEREVS``) strikes a balance between
        two extremes. Deltas will be reused if they are appropriate. But if the
        delta could choose a better revision, it will do so. This means if you
        are converting a non-generaldelta revlog to a generaldelta revlog,
        deltas will be recomputed if the delta's parent isn't a parent of the
        revision.

        In addition to the delta policy, the ``aggressivemergedeltas`` argument
        controls whether to compute deltas against both parents for merges.
        By default, the current default is used.
        """
        if deltareuse not in self.DELTAREUSEALL:
            raise ValueError(_('value for deltareuse invalid: %s') % deltareuse)

        if len(destrevlog):
            raise ValueError(_('destination revlog is not empty'))

        if getattr(self, 'filteredrevs', None):
            raise ValueError(_('source revlog has filtered revisions'))
        if getattr(destrevlog, 'filteredrevs', None):
            raise ValueError(_('destination revlog has filtered revisions'))

        # lazydeltabase controls whether to reuse a cached delta, if possible.
        oldlazydeltabase = destrevlog._lazydeltabase
        oldamd = destrevlog._aggressivemergedeltas

        try:
            if deltareuse == self.DELTAREUSEALWAYS:
                destrevlog._lazydeltabase = True
            elif deltareuse == self.DELTAREUSESAMEREVS:
                destrevlog._lazydeltabase = False

            destrevlog._aggressivemergedeltas = aggressivemergedeltas or oldamd

            populatecachedelta = deltareuse in (self.DELTAREUSEALWAYS,
                                                self.DELTAREUSESAMEREVS)

            index = self.index
            for rev in self:
                entry = index[rev]

                # Some classes override linkrev to take filtered revs into
                # account. Use raw entry from index.
                flags = entry[0] & 0xffff
                linkrev = entry[4]
                p1 = index[entry[5]][7]
                p2 = index[entry[6]][7]
                node = entry[7]

                # (Possibly) reuse the delta from the revlog if allowed and
                # the revlog chunk is a delta.
                cachedelta = None
                rawtext = None
                if populatecachedelta:
                    dp = self.deltaparent(rev)
                    if dp != nullrev:
                        cachedelta = (dp, str(self._chunk(rev)))

                if not cachedelta:
                    rawtext = self.revision(rev, raw=True)

                ifh = destrevlog.opener(destrevlog.indexfile, 'a+',
                                        checkambig=False)
                dfh = None
                if not destrevlog._inline:
                    dfh = destrevlog.opener(destrevlog.datafile, 'a+')
                try:
                    destrevlog._addrevision(node, rawtext, tr, linkrev, p1, p2,
                                            flags, cachedelta, ifh, dfh)
                finally:
                    if dfh:
                        dfh.close()
                    ifh.close()

                if addrevisioncb:
                    addrevisioncb(self, rev, node)
        finally:
            destrevlog._lazydeltabase = oldlazydeltabase
            destrevlog._aggressivemergedeltas = oldamd
