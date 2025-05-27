# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# revlog.py - storage back-end for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Storage back-end for Mercurial.

This provides efficient delta storage with O(1) retrieve and append
and O(changes) merge between branches.
"""

import errno
import hashlib
import heapq
import os
import struct
import zlib
from typing import IO, Optional, Tuple, Union

import bindings

from . import ancestor, error, mdiff, templatefilters, util
from .i18n import _

# import stuff from node for others to import from revlog
from .node import bbin, bhex, nullid, nullrev, wdirid, wdirrev

parsers = bindings.cext.parsers

# Aliased for performance.
_zlibdecompress = zlib.decompress

# revlog header flags
REVLOGV0 = 0
REVLOGV1 = 1
FLAG_INLINE_DATA = 1 << 16
FLAG_GENERALDELTA = 1 << 17
REVLOG_DEFAULT_FLAGS = FLAG_INLINE_DATA
REVLOG_DEFAULT_FORMAT = REVLOGV1
REVLOG_DEFAULT_VERSION = REVLOG_DEFAULT_FORMAT | REVLOG_DEFAULT_FLAGS
REVLOGV1_FLAGS = FLAG_INLINE_DATA | FLAG_GENERALDELTA

# revlog index flags
REVIDX_ISCENSORED = 1 << 15  # revision has censor metadata, must be verified
REVIDX_ELLIPSIS = 1 << 14  # revision hash does not match data (narrowhg)
REVIDX_EXTSTORED = 1 << 13  # revision data is stored externally
REVIDX_DEFAULT_FLAGS = 0
# stable order in which flags need to be processed and their processors applied
REVIDX_FLAGS_ORDER = [REVIDX_ISCENSORED, REVIDX_ELLIPSIS, REVIDX_EXTSTORED]
REVIDX_KNOWN_FLAGS = util.bitsfrom(REVIDX_FLAGS_ORDER)

# max size of revlog with inline data
_maxinline = 131072
_chunksize = 1048576

RevlogError = error.RevlogError
LookupError = error.LookupError
CensoredNodeError = error.CensoredNodeError
ProgrammingError = error.ProgrammingError

# Store flag processors (cf. 'addflagprocessor()' to register)
_flagprocessors = {REVIDX_ISCENSORED: None}


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
        msg = _("cannot register processor on unknown flag '%#x'.") % flag
        raise ProgrammingError(msg)
    if flag not in REVIDX_FLAGS_ORDER:
        msg = _("flag '%#x' undefined in REVIDX_FLAGS_ORDER.") % flag
        raise ProgrammingError(msg)
    if flag in _flagprocessors:
        msg = _("cannot register multiple processors on flag '%#x'.") % flag
        raise error.Abort(msg)
    _flagprocessors[flag] = processor


def getoffset(q):
    return int(q >> 16)


def gettype(q):
    return int(q & 0xFFFF)


def offset_type(offset, type):
    if (type & ~REVIDX_KNOWN_FLAGS) != 0:
        raise ValueError("unknown revlog index flags")
    return int(int(offset) << 16 | type)


_nullhash = hashlib.sha1(nullid)


def hash(text: bytes, p1: bytes, p2: bytes) -> bytes:
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


def textwithheader(text: bytes, p1: bytes, p2: bytes) -> bytes:
    """Similar to `hash`, but only return the content before calculating SHA1."""
    assert isinstance(p1, bytes)
    assert isinstance(p2, bytes)
    if p1 < p2:
        a = p1
        b = p2
    else:
        a = p2
        b = p1
    return b"%s%s%s" % (a, b, text)


def _trimchunk(revlog, revs, startidx, endidx=None):
    """returns revs[startidx:endidx] without empty trailing revs"""
    length = revlog.length

    if endidx is None:
        endidx = len(revs)

    # Trim empty revs at the end, but never the very first revision of a chain
    while endidx > 1 and endidx > startidx and length(revs[endidx - 1]) == 0:
        endidx -= 1

    return revs[startidx:endidx]


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
_maxentrysize = 0x7FFFFFFF


class revlogio:
    def __init__(self):
        self.size = indexformatng.size

    def parseindex(self, data, inline):
        # call the C implementation to parse the index data
        index, cache = parsers.parse_index2(data, inline)
        return index, getattr(index, "nodemap", None), cache

    def packentry(self, entry, node, version, rev):
        p = indexformatng_pack(*entry)
        if rev == 0:
            p = versionformat_pack(version) + p[4:]
        return p


class revlog:
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

    def __init__(
        self,
        opener,
        indexfile,
        datafile=None,
        checkambig=False,
        mmaplargeindex=False,
        index2=False,
    ):
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
        self._chunkcache = (0, b"")
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
        self._compengine = "zlib"
        self._maxdeltachainspan = -1

        mmapindexthreshold = None
        v = REVLOG_DEFAULT_VERSION
        opts = getattr(opener, "options", None)
        if opts is not None:
            if "revlogv1" in opts:
                if "generaldelta" in opts:
                    v |= FLAG_GENERALDELTA
            else:
                v = 0
            if "chunkcachesize" in opts:
                self._chunkcachesize = opts["chunkcachesize"]
            if "maxchainlen" in opts:
                self._maxchainlen = opts["maxchainlen"]
            if "aggressivemergedeltas" in opts:
                self._aggressivemergedeltas = opts["aggressivemergedeltas"]
            self._lazydeltabase = bool(opts.get("lazydeltabase", False))
            if "compengine" in opts:
                self._compengine = opts["compengine"]
            if mmaplargeindex and "mmapindexthreshold" in opts:
                mmapindexthreshold = opts["mmapindexthreshold"]

        if self._chunkcachesize <= 0:
            raise RevlogError(
                _("revlog chunk cache size %r is not greater than 0")
                % self._chunkcachesize
            )
        elif self._chunkcachesize & (self._chunkcachesize - 1):
            raise RevlogError(
                _("revlog chunk cache size %r is not a power of 2")
                % self._chunkcachesize
            )

        if index2:
            nodemapfile = indexfile[:-2] + ".nodemap"
            self.index2 = bindings.revlogindex.revlogindex(
                opener.join(indexfile), opener.join(nodemapfile)
            )
            # Use indexdata read by Rust to be consistent.
            # indexdata is alive as long as index2 is alive.
            indexdata = self.index2.indexdata().asref()
            # Rust code uses mmap to read. Avoid mmap if the config is not set.
            if not (
                mmapindexthreshold is not None and len(indexdata) >= mmapindexthreshold
            ):
                indexdata = bytes(indexdata)
        else:
            # Load indexdata from disk.
            indexdata = b""
            try:
                f = self.opener(self.indexfile)
                if (
                    mmapindexthreshold is not None
                    and self.opener.fstat(f).st_size >= mmapindexthreshold
                ):
                    indexdata = util.buffer(util.mmapread(f))
                else:
                    indexdata = f.read()
                f.close()
            except IOError as inst:
                if inst.errno != errno.ENOENT:
                    raise
        if len(indexdata) > 0:
            v = versionformat_unpack(indexdata[:4])[0]
            self._initempty = False
        else:
            self._initempty = True

        # Expose indexdata for easier debugging.
        self.indexdata = indexdata

        self.version = v
        self._inline = v & FLAG_INLINE_DATA
        self._generaldelta = v & FLAG_GENERALDELTA
        flags = v & ~0xFFFF
        fmt = v & 0xFFFF
        if fmt == REVLOGV0:
            raise error.Abort(_("repo is corrupted: %s") % (self.indexfile,))
        elif fmt == REVLOGV1:
            if flags & ~REVLOGV1_FLAGS:
                raise RevlogError(
                    _("unknown flags (%#04x) in version %d revlog %s")
                    % (flags >> 16, fmt, self.indexfile)
                )
        else:
            raise RevlogError(
                _("unknown version (%d) in revlog %s") % (fmt, self.indexfile)
            )

        self.storedeltachains = True

        self._io = revlogio()
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
        # Whether to bypass ftruncate-based transaction framework.
        # If set (usually for changelog), commits are always flushed
        # without buffering or rolling back, and commit references
        # like visibleheads and bookmarks control the commit graph.
        self._bypasstransaction = bool(opts and opts.get("bypass-revlog-transaction"))

    @util.propertycache
    def _compressor(self):
        return util.compengines[self._compengine].revlogcompressor()

    def __len__(self) -> int:
        return len(self.index) - 1

    def __iter__(self):
        return iter(range(len(self)))

    def revs(self, start=0, stop=None):
        """iterate over all rev in this revlog (from start to stop)"""
        step = 1
        if stop is not None:
            if start > stop:
                step = -1
            stop += step
        else:
            stop = len(self)
        return range(start, stop, step)

    def rev(self, node):
        try:
            return self._nodecache[node]
        except TypeError:
            raise
        except RevlogError:
            # parsers.c radix tree lookup failed
            if node == wdirid:
                raise error.WdirUnsupported
            raise LookupError(node, self.indexfile, _("no node"))
        except KeyError:
            # pure python cache lookup failed
            n = self._nodecache
            i = self.index
            p = self._nodepos
            if p is None:
                p = len(i) - 2
            for r in range(p, -1, -1):
                v = i[r][7]
                n[v] = r
                if v == node:
                    self._nodepos = r - 1
                    return r
            if node == wdirid:
                raise error.WdirUnsupported
            raise LookupError(node, self.indexfile, _("no node"))

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

    def size(self, rev: int) -> int:
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
        return i[d[5]][7], i[d[6]][7]  # map revisions to nodes inline

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

        return ancestor.lazyancestors(
            self.parentrevs, revs, stoprev=stoprev, inclusive=inclusive
        )

    def commonancestorsheads(self, a, b):
        """calculate all the heads of the common ancestors of nodes a and b"""
        a, b = self.rev(a), self.rev(b)
        try:
            ancs = self.index.commonancestorsheads(a, b)
        except (AttributeError, OverflowError):  # C implementation failed
            ancs = ancestor.commonancestorsheads(self.parentrevs, a, b)
        return list(map(self.node, ancs))

    def _match(self, id):
        if isinstance(id, int):
            # rev
            return self.node(id)
        if len(id) == 20:
            # possibly a binary node
            # odds of a binary node being all hex in ASCII are 1 in 10**25
            try:
                node = id
                self.rev(node)  # quick search the index
                return node
            except LookupError:
                pass  # may be partial hex id
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
                node = bbin(id)
                self.rev(node)
                return node
            except (TypeError, LookupError):
                pass

    def lookup(self, id: "Union[int, str, bytes]") -> bytes:
        """locate a node based on:
        - revision number or str(revision number)
        - nodeid or subset of hex nodeid
        """
        n = self._match(id)
        if n is not None:
            return n
        raise LookupError(id, self.indexfile, _("no match found"))

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
        reallength = ((offset + length + cachesize) & ~(cachesize - 1)) - realoffset
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
                return d  # avoid a copy
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

        slicedchunks = (revs,)

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
        self._chunkcache = (0, b"")

    def candelta(self, baserev, rev):
        """whether two revisions (prev, rev) can be delta-ed or not"""
        # disable delta if either rev uses non-default flag (ex. LFS)
        if self.flags(baserev) or self.flags(rev):
            return False
        return True

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

        if rev1 > -1 and (self.flags(rev1) or self.flags(rev2)):
            raise error.ProgrammingError("cannot revdiff revisions with non-zero flags")

        return mdiff.textdiff(
            self.revision(rev1, raw=True), self.revision(rev2, raw=True)
        )

    def revision(
        self,
        nodeorrev: "Union[int, bytes]",
        _df: "Optional[IO]" = None,
        raw: bool = False,
    ) -> bytes:
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
            return b""
        cache = self._cache
        if cache is not None:
            if cache[0] == node:
                # _cache only stores rawtext
                if raw:
                    return cache[2]
                # duplicated, but good for perf
                if rev is None:
                    rev = self.rev(node)
                if flags is None:
                    flags = self.flags(rev)
                # no extra flags set, no flag processor runs, text = rawtext
                if flags == REVIDX_DEFAULT_FLAGS:
                    return cache[2]
                # rawtext is reusable. need to run flag processor
                rawtext = cache[2]

            cachedrev = cache[1]

        # look up what we need to read
        if rawtext is None:
            if rev is None:
                rev = self.rev(node)

            chain, stopped = self._deltachain(rev, stoprev=cachedrev)
            if cache is not None:
                if stopped:
                    rawtext = cache[2]

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

        text, validatehash = self._processflags(rawtext, flags, "read", raw=raw)
        if validatehash:
            self.checkhash(text, node, rev=rev)

        return text

    def hash(self, text: bytes, p1: bytes, p2: bytes) -> bytes:
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
        if not operation in ("read", "write"):
            raise ProgrammingError(_("invalid '%s' operation ") % operation)
        # Check all flags are known.
        if flags & ~REVIDX_KNOWN_FLAGS:
            raise RevlogError(
                _("incompatible revision flag '%#x'") % (flags & ~REVIDX_KNOWN_FLAGS)
            )
        validatehash = True
        # Depending on the operation (read or write), the order might be
        # reversed due to non-commutative transforms.
        orderedflags = REVIDX_FLAGS_ORDER
        if operation == "write":
            orderedflags = reversed(orderedflags)

        for flag in orderedflags:
            # If a flagprocessor has been registered for a known flag, apply the
            # related operation transform and update result tuple.
            if flag & flags:
                vhash = True

                if flag not in _flagprocessors:
                    message = _("missing processor for flag '%#x'") % flag
                    raise RevlogError(message)

                processor = _flagprocessors[flag]
                if processor is not None:
                    readtransform, writetransform, rawtransform = processor

                    if raw:
                        vhash = rawtransform(self, text)
                    elif operation == "read":
                        text, vhash = readtransform(self, text)
                    else:  # write operation
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
                revornode = templatefilters.short(bhex(node))
            raise RevlogError(
                _("integrity check failed on %s:%s") % (self.indexfile, str(revornode))
            )

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
            raise RevlogError(_("%s not found in the transaction") % self.indexfile)

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

        df = self.opener(self.datafile, "w")
        try:
            for r in self:
                df.write(self._getsegmentforrevs(r, r)[1])
        finally:
            df.close()

        fp = self.opener(
            self.indexfile, "w", atomictemp=True, checkambig=self._checkambig
        )
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

    def _contains(self, node, p1node, p2node):
        """An efficient version of contains that bounds the search based on the
        parents.
        """
        if p1node == nullid and p2node == nullid:
            return node in self.nodemap

        maxparent = max(self.rev(p1node), self.rev(p2node))

        # If the parents are far from the tip, computing the descendants will be
        # expensive, so just revert back to scanning the revlog in C.
        tip = len(self) - 1
        if tip - maxparent > 1000:
            return node in self.nodemap

        getnode = self.node
        for i in self.revs(start=maxparent + 1):
            if node == getnode(i):
                return True
        return False

    def addrevision(
        self,
        text,
        transaction,
        link,
        p1,
        p2,
        cachedelta=None,
        node=None,
        flags=REVIDX_DEFAULT_FLAGS,
    ):
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
            raise RevlogError(_("attempted to add linkrev -1 to %s") % self.indexfile)

        if flags:
            node = node or self.hash(text, p1, p2)

        rawtext, validatehash = self._processflags(text, flags, "write")

        # If the flag processor modifies the revision data, ignore any provided
        # cachedelta.
        if rawtext != text:
            cachedelta = None

        if len(rawtext) > _maxentrysize:
            raise RevlogError(
                _("%s: size of %d bytes exceeds maximum revlog storage of 2GiB")
                % (self.indexfile, len(rawtext))
            )

        # Only validate the hash if it was provided to us
        validatehash &= bool(node)
        node = node or self.hash(rawtext, p1, p2)
        if self._contains(node, p1, p2):
            return node

        if validatehash:
            self.checkhash(rawtext, node, p1=p1, p2=p2)

        return self.addrawrevision(
            rawtext, transaction, link, p1, p2, node, flags, cachedelta=cachedelta
        )

    def addrawrevision(
        self, rawtext, transaction, link, p1, p2, node, flags, cachedelta=None
    ):
        """add a raw revision with known flags, node and parents
        useful when reusing a revision not stored in this revlog (ex: received
        over wire, or read from an external bundle).
        """
        dfh = None
        if not self._inline:
            dfh = self.opener(self.datafile, "a+")
        ifh = self.opener(self.indexfile, "a+", checkambig=self._checkambig)
        try:
            return self._addrevision(
                node, rawtext, transaction, link, p1, p2, flags, cachedelta, ifh, dfh
            )
        finally:
            if dfh:
                dfh.close()
            ifh.close()

    def compress(self, data: bytes) -> "Tuple[bytes, bytes]":
        """Generate a possibly-compressed representation of data."""
        if not data:
            return b"", data

        compressed = self._compressor.compress(data)

        if compressed:
            # The revlog compressor added the header in the returned data.
            return b"", compressed

        if data[0:1] == b"\0":
            return b"", data
        return b"u", data

    def decompress(self, data: bytes) -> bytes:
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
        t = bytes(data[0:1])

        if t == b"x":
            try:
                return _zlibdecompress(data)
            except zlib.error as e:
                raise RevlogError(_("revlog decompress error: %s") % str(e))
        # '\0' is more common than 'u' so it goes first.
        elif t == b"\0":
            return data
        elif t == b"u":
            return util.buffer(data, 1)

        try:
            compressor = self._decompressors[t]
        except KeyError:
            try:
                engine = util.compengines.forrevlogheader(t)
                compressor = engine.revlogcompressor()
                self._decompressors[t] = compressor
            except KeyError:
                raise RevlogError(_("unknown compression type %r") % t)

        return compressor.decompress(data)

    def _isgooddelta(self, d, textlen):
        """Returns True if the given delta is good. Good means that it is within
        the disk span, disk size, and chain length bounds that we know to be
        performant."""
        if d is None:
            return False

        # - 'compresseddeltalen' is the sum of the total size of deltas we need
        #   to apply -- bounding it limits the amount of CPU we consume.
        _dist, l, data, base, chainbase, chainlen, compresseddeltalen = d

        # Criteria:
        # 1. the delta is not larger than the full text
        # 2. the delta chain cumulative size is not greater than twice the
        #    fulltext
        # 3. The chain length is less than the maximum
        #
        # This differs from upstream Mercurial's criteria. They prevent the
        # total ondisk span from chain base to rev from being greater than 4x
        # the full text len. This isn't good enough in our world since if we
        # have 10+ branches going on at once, we can easily exceed the 4x limit
        # and cause full texts to be written over and over again.
        if (
            l > textlen
            or compresseddeltalen > textlen * 2
            or (self._maxchainlen and chainlen > self._maxchainlen)
        ):
            return False

        return True

    def _addrevision(
        self, node, rawtext, transaction, link, p1, p2, flags, cachedelta, ifh, dfh
    ):
        """internal function to add revisions to the log

        see addrevision for argument descriptions.

        note: "addrevision" takes non-raw text, "_addrevision" takes raw text.

        invariants:
        - rawtext is optional (can be None); if not set, cachedelta must be set.
          if both are set, they must correspond to each other.
        """
        if node == nullid:
            raise RevlogError(_("%s: attempt to add null revision") % (self.indexfile))
        if node == wdirid:
            raise RevlogError(_("%s: attempt to add wdir revision") % (self.indexfile))

        if isinstance(rawtext, memoryview):
            btext = [bytes(rawtext)]
        else:
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
            if delta[:hlen] == mdiff.replacediffheader(
                self.rawsize(baserev), len(delta) - hlen
            ):
                btext[0] = delta[hlen:]
            else:
                if self._inline:
                    fh = ifh
                else:
                    fh = dfh
                # Deltas are against "flags=0 rawtext".Need "flags=0" rawtext
                # here, which is equivalent to non-raw text.
                basetext = self.revision(baserev, _df=fh, raw=False)
                btext[0] = mdiff.patch(basetext, delta)

            try:
                res = self._processflags(btext[0], flags, "read", raw=True)
                btext[0], validatehash = res
                if validatehash:
                    self.checkhash(btext[0], node, p1=p1, p2=p2)
                if flags & REVIDX_ISCENSORED:
                    raise RevlogError(_("node %s is not censored") % node)
            except CensoredNodeError:
                # must pass the censored index flag to add censored revisions
                if not flags & REVIDX_ISCENSORED:
                    raise
            return btext[0]

        def builddelta(rev):
            chainbase = self.chainbase(rev)
            if self._generaldelta:
                base = rev
            else:
                base = chainbase
            # Refuse to build delta if deltabase rev has a non-zero flag
            if self.flags(base):
                return None
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
            dist = deltalen + offset - self.start(chainbase)
            chainlen, compresseddeltalen = self._chaininfo(rev)
            chainlen += 1
            compresseddeltalen += deltalen
            return (
                dist,
                deltalen,
                (header, data),
                base,
                chainbase,
                chainlen,
                compresseddeltalen,
            )

        curr = len(self)
        prev = curr - 1
        offset = self.end(prev)
        delta = None
        p1r, p2r = self.rev(p1), self.rev(p2)

        # full versions are inserted when the needed deltas
        # become comparable to the uncompressed text
        if rawtext is None:
            # need flags=0 rawtext size, which is the non-raw size
            # use revlog explicitly, filelog.size would be wrong
            textlen = mdiff.patchedsize(revlog.size(self, cachedelta[0]), cachedelta[1])
        else:
            textlen = len(rawtext)

        # should we try to build a delta?
        if prev != nullrev and self.storedeltachains and not flags:
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
                parents = [p for p in (p1r, p2r) if p != nullrev and p not in tested]
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

        # Segmented changelog might use u64 ids, that cannot be packed as i32.
        # Change them so indexformatng_pack(*entry) won't error out like:
        # error: 'i' format requires -2147483648 <= number <= 2147483647
        # Segmented changelog repos in tests have "invalidatelinkrev"
        # requirement set, and won't respect the linkrevs stored in revlogs.
        if link > 2147483647:
            link = -1

        e = (offset_type(offset, flags), l, textlen, base, link, p1r, p2r, node)
        if self.index is not None:
            self.index.insert(-1, e)
        index2 = getattr(self, "index2", None)
        if index2 is not None:
            index2.insert(node, [p for p in (p1r, p2r) if p >= 0])
        self.nodemap[node] = curr

        entry = self._io.packentry(e, self.node, self.version, curr)
        self._writeentry(transaction, ifh, dfh, entry, data, link, offset)

        if type(rawtext) == bytes:  # only accept immutable objects
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
            if not self._bypasstransaction:
                transaction.add(self.datafile, offset)
                transaction.add(self.indexfile, curr * len(entry))
            if data[0]:
                dfh.write(data[0])
            dfh.write(data[1])
            ifh.write(entry)
        else:
            offset += curr * self._io.size
            if not self._bypasstransaction:
                transaction.add(self.indexfile, offset, curr)
            ifh.write(entry)
            ifh.write(data[0])
            ifh.write(data[1])
            if not self._bypasstransaction:
                self.checkinlinesize(transaction, ifh)

    def addgroup(self, deltas, linkmapper, transaction):
        """
        add a delta group

        given a set of deltas, add them to the revision log. the
        first delta is against its parent, which should be in our
        log, the rest are against the previous delta.
        """

        nodes = []

        r = len(self)
        end = 0
        if r:
            end = self.end(r - 1)
        ifh = self.opener(self.indexfile, "a+", checkambig=self._checkambig)
        isize = r * self._io.size
        if self._inline:
            if not self._bypasstransaction:
                transaction.add(self.indexfile, end + isize, r)
            dfh = None
        else:
            if not self._bypasstransaction:
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

                for p in (p1, p2):
                    if p not in self.nodemap:
                        raise LookupError(p, self.indexfile, _("unknown parent"))

                if self._contains(node, p1, p2):
                    # this can happen if two branches make the same change
                    continue

                if deltabase not in self.nodemap:
                    raise LookupError(
                        deltabase, self.indexfile, _("unknown delta base")
                    )

                baserev = self.rev(deltabase)

                if baserev != nullrev and self.iscensored(baserev):
                    # if base is censored, delta must be full replacement in a
                    # single patch operation
                    hlen = struct.calcsize(">lll")
                    oldlen = self.rawsize(baserev)
                    newlen = len(delta) - hlen
                    if delta[:hlen] != mdiff.replacediffheader(oldlen, newlen):
                        raise error.CensoredBaseError(
                            self.indexfile, self.node(baserev)
                        )

                if not flags and self._peek_iscensored(baserev, delta, flush):
                    flags |= REVIDX_ISCENSORED

                self._addrevision(
                    node,
                    None,
                    transaction,
                    link,
                    p1,
                    p2,
                    flags,
                    (baserev, delta),
                    ifh,
                    dfh,
                )

                if not dfh and not self._inline:
                    # addrevision switched from inline to conventional
                    # reopen the index
                    ifh.close()
                    dfh = self.opener(self.datafile, "a+")
                    ifh = self.opener(self.indexfile, "a+", checkambig=self._checkambig)
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

    DELTAREUSEALWAYS = "always"
    DELTAREUSESAMEREVS = "samerevs"
    DELTAREUSENEVER = "never"

    DELTAREUSEFULLADD = "fulladd"

    DELTAREUSEALL = {"always", "samerevs", "never", "fulladd"}
