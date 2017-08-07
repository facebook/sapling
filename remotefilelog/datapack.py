from __future__ import absolute_import

import struct
from mercurial import (
    error,
    util,
)
from mercurial.node import nullid, hex
from mercurial.i18n import _
from . import basepack, constants, shallowutil
from .lz4wrapper import lz4compress, lz4decompress
try:
    import cstore
    cstore.datapack
except ImportError:
    cstore = None

NODELENGTH = 20

# The indicator value in the index for a fulltext entry.
FULLTEXTINDEXMARK = -1
NOBASEINDEXMARK = -2

INDEXSUFFIX = '.dataidx'
PACKSUFFIX = '.datapack'

class datapackstore(basepack.basepackstore):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def __init__(self, ui, path, usecdatapack=False):
        self.usecdatapack = usecdatapack
        super(datapackstore, self).__init__(ui, path)

    def getpack(self, path):
        if self.usecdatapack:
            return fastdatapack(path)
        else:
            return datapack(path)

    def get(self, name, node):
        raise RuntimeError("must use getdeltachain with datapackstore")

    def getmeta(self, name, node):
        for pack in self.packs:
            try:
                return pack.getmeta(name, node)
            except KeyError:
                pass

        for pack in self.refresh():
            try:
                return pack.getmeta(name, node)
            except KeyError:
                pass

        raise KeyError((name, hex(node)))

    def getdeltachain(self, name, node):
        for pack in self.packs:
            try:
                return pack.getdeltachain(name, node)
            except KeyError:
                pass

        for pack in self.refresh():
            try:
                return pack.getdeltachain(name, node)
            except KeyError:
                pass

        raise KeyError((name, hex(node)))

    def add(self, name, node, data):
        raise RuntimeError("cannot add to datapackstore")

class datapack(basepack.basepack):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    # Format is <node><delta offset><pack data offset><pack data size>
    # See the mutabledatapack doccomment for more details.
    INDEXFORMAT = '!20siQQ'
    INDEXENTRYLENGTH = 40

    SUPPORTED_VERSIONS = [0, 1]

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            value = self._find(node)
            if not value:
                missing.append((name, node))

        return missing

    def get(self, name, node):
        raise RuntimeError("must use getdeltachain with datapack (%s:%s)"
                           % (name, hex(node)))

    def getmeta(self, name, node):
        value = self._find(node)
        if value is None:
            raise KeyError((name, hex(node)))

        # version 0 does not support metadata
        if self.VERSION == 0:
            return {}

        node, deltabaseoffset, offset, size = value
        rawentry = self._data[offset:offset + size]

        # see docstring of mutabledatapack for the format
        offset = 0
        offset += struct.unpack_from('!H', rawentry, offset)[0] + 2 # filename
        offset += 40 # node, deltabase node
        offset += struct.unpack_from('!Q', rawentry, offset)[0] + 8 # delta

        metalen = struct.unpack_from('!I', rawentry, offset)[0]
        offset += 4

        meta = shallowutil.parsepackmeta(rawentry[offset:offset + metalen])

        return meta

    def getdeltachain(self, name, node):
        value = self._find(node)
        if value is None:
            raise KeyError((name, hex(node)))

        params = self.params

        # Precompute chains
        chain = [value]
        deltabaseoffset = value[1]
        entrylen = self.INDEXENTRYLENGTH
        while (deltabaseoffset != FULLTEXTINDEXMARK
               and deltabaseoffset != NOBASEINDEXMARK):
            loc = params.indexstart + deltabaseoffset
            value = struct.unpack(self.INDEXFORMAT,
                                  self._index[loc:loc + entrylen])
            deltabaseoffset = value[1]
            chain.append(value)

        # Read chain data
        deltachain = []
        for node, deltabaseoffset, offset, size in chain:
            rawentry = self._data[offset:offset + size]
            self._pagedin += len(rawentry)

            # <2 byte len> + <filename>
            lengthsize = 2
            filenamelen = struct.unpack('!H', rawentry[:2])[0]
            filename = rawentry[lengthsize:lengthsize + filenamelen]

            # <20 byte node> + <20 byte deltabase>
            nodestart = lengthsize + filenamelen
            deltabasestart = nodestart + NODELENGTH
            node = rawentry[nodestart:deltabasestart]
            deltabasenode = rawentry[deltabasestart:deltabasestart + NODELENGTH]

            # <8 byte len> + <delta>
            deltastart = deltabasestart + NODELENGTH
            rawdeltalen = rawentry[deltastart:deltastart + 8]
            deltalen = struct.unpack('!Q', rawdeltalen)[0]

            delta = rawentry[deltastart + 8:deltastart + 8 + deltalen]
            delta = lz4decompress(delta)

            deltachain.append((filename, node, filename, deltabasenode, delta))

        # If we've read a lot of data from the mmap, free some memory.
        self.freememory()

        return deltachain

    def add(self, name, node, data):
        raise RuntimeError("cannot add to datapack (%s:%s)" % (name, node))

    def _find(self, node):
        params = self.params
        fanoutkey = struct.unpack(params.fanoutstruct,
                                  node[:params.fanoutprefix])[0]
        fanout = self._fanouttable

        start = fanout[fanoutkey] + params.indexstart
        indexend = self._indexend

        # Scan forward to find the first non-same entry, which is the upper
        # bound.
        for i in xrange(fanoutkey + 1, params.fanoutcount):
            end = fanout[i] + params.indexstart
            if end != start:
                break
        else:
            end = indexend

        # Bisect between start and end to find node
        index = self._index
        startnode = index[start:start + NODELENGTH]
        endnode = index[end:end + NODELENGTH]
        entrylen = self.INDEXENTRYLENGTH
        if startnode == node:
            entry = index[start:start + entrylen]
        elif endnode == node:
            entry = index[end:end + entrylen]
        else:
            while start < end - entrylen:
                mid = start  + (end - start) / 2
                mid = mid - ((mid - params.indexstart) % entrylen)
                midnode = index[mid:mid + NODELENGTH]
                if midnode == node:
                    entry = index[mid:mid + entrylen]
                    break
                if node > midnode:
                    start = mid
                    startnode = midnode
                elif node < midnode:
                    end = mid
                    endnode = midnode
            else:
                return None

        return struct.unpack(self.INDEXFORMAT, entry)

    def markledger(self, ledger):
        for filename, node in self:
            ledger.markdataentry(self, filename, node)

    def cleanup(self, ledger):
        entries = ledger.sources.get(self, [])
        allkeys = set(self)
        repackedkeys = set((e.filename, e.node) for e in entries if
                           e.datarepacked or e.gced)

        if len(allkeys - repackedkeys) == 0:
            if self.path not in ledger.created:
                util.unlinkpath(self.indexpath, ignoremissing=True)
                util.unlinkpath(self.packpath, ignoremissing=True)

    def __iter__(self):
        for f, n, deltabase, deltalen in self.iterentries():
            yield f, n

    def iterentries(self):
        # Start at 1 to skip the header
        offset = 1
        data = self._data
        while offset < self.datasize:
            oldoffset = offset

            # <2 byte len> + <filename>
            filenamelen = struct.unpack('!H', data[offset:offset + 2])[0]
            offset += 2
            filename = data[offset:offset + filenamelen]
            offset += filenamelen

            # <20 byte node>
            node = data[offset:offset + constants.NODESIZE]
            offset += constants.NODESIZE
            # <20 byte deltabase>
            deltabase = data[offset:offset + constants.NODESIZE]
            offset += constants.NODESIZE

            # <8 byte len> + <delta>
            rawdeltalen = data[offset:offset + 8]
            deltalen = struct.unpack('!Q', rawdeltalen)[0]
            offset += 8

            # it has to be at least long enough for the lz4 header.
            assert deltalen >= 4

            # python-lz4 stores the length of the uncompressed field as a
            # little-endian 32-bit integer at the start of the data.
            uncompressedlen = struct.unpack('<I', data[offset:offset + 4])[0]
            offset += deltalen

            if self.VERSION == 1:
                # <4 byte len> + <metadata-list>
                metalen = struct.unpack_from('!I', data, offset)[0]
                offset += 4 + metalen

            yield (filename, node, deltabase, uncompressedlen)

            # If we've read a lot of data from the mmap, free some memory.
            self._pagedin += offset - oldoffset
            if self.freememory():
                data = self._data

class fastdatapack(basepack.basepack):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def __init__(self, path):
        self.path = path
        self.packpath = path + self.PACKSUFFIX
        self.indexpath = path + self.INDEXSUFFIX
        self.datapack = cstore.datapack(path)

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            value = self.datapack._find(node)
            if not value:
                missing.append((name, node))

        return missing

    def get(self, name, node):
        raise RuntimeError("must use getdeltachain with datapack (%s:%s)"
                           % (name, hex(node)))

    def getmeta(self, name, node):
        return self.datapack.getmeta(node)

    def getdeltachain(self, name, node):
        result = self.datapack.getdeltachain(node)
        if result is None:
            raise KeyError((name, hex(node)))

        return result

    def add(self, name, node, data):
        raise RuntimeError("cannot add to datapack (%s:%s)" % (name, node))

    def markledger(self, ledger):
        for filename, node in self:
            ledger.markdataentry(self, filename, node)

    def cleanup(self, ledger):
        entries = ledger.sources.get(self, [])
        allkeys = set(self)
        repackedkeys = set((e.filename, e.node) for e in entries if
                           e.datarepacked or e.gced)

        if len(allkeys - repackedkeys) == 0:
            if self.path not in ledger.created:
                util.unlinkpath(self.indexpath, ignoremissing=True)
                util.unlinkpath(self.packpath, ignoremissing=True)

    def __iter__(self):
        return self.datapack.__iter__()

    def iterentries(self):
        return self.datapack.iterentries()

class mutabledatapack(basepack.mutablebasepack):
    """A class for constructing and serializing a datapack file and index.

    A datapack is a pair of files that contain the revision contents for various
    file revisions in Mercurial. It contains only revision contents (like file
    contents), not any history information.

    It consists of two files, with the following format. All bytes are in
    network byte order (big endian).

    .datapack
        The pack itself is a series of revision deltas with some basic header
        information on each. A revision delta may be a fulltext, represented by
        a deltabasenode equal to the nullid.

        datapack = <version: 1 byte>
                   [<revision>,...]
        revision = <filename len: 2 byte unsigned int>
                   <filename>
                   <node: 20 byte>
                   <deltabasenode: 20 byte>
                   <delta len: 8 byte unsigned int>
                   <delta>
                   <metadata-list len: 4 byte unsigned int> [1]
                   <metadata-list>                          [1]
        metadata-list = [<metadata-item>, ...]
        metadata-item = <metadata-key: 1 byte>
                        <metadata-value len: 2 byte unsigned>
                        <metadata-value>

        metadata-key could be METAKEYFLAG or METAKEYSIZE or other single byte
        value in the future.

    .dataidx
        The index file consists of two parts, the fanout and the index.

        The index is a list of index entries, sorted by node (one per revision
        in the pack). Each entry has:

        - node (The 20 byte node of the entry; i.e. the commit hash, file node
                hash, etc)
        - deltabase index offset (The location in the index of the deltabase for
                                  this entry. The deltabase is the next delta in
                                  the chain, with the chain eventually
                                  terminating in a full-text, represented by a
                                  deltabase offset of -1. This lets us compute
                                  delta chains from the index, then do
                                  sequential reads from the pack if the revision
                                  are nearby on disk.)
        - pack entry offset (The location of this entry in the datapack)
        - pack content size (The on-disk length of this entry's pack data)

        The fanout is a quick lookup table to reduce the number of steps for
        bisecting the index. It is a series of 4 byte pointers to positions
        within the index. It has 2^16 entries, which corresponds to hash
        prefixes [0000, 0001,..., FFFE, FFFF]. Example: the pointer in slot
        4F0A points to the index position of the first revision whose node
        starts with 4F0A. This saves log(2^16)=16 bisect steps.

        dataidx = <fanouttable>
                  <index>
        fanouttable = [<index offset: 4 byte unsigned int>,...] (2^16 entries)
        index = [<index entry>,...]
        indexentry = <node: 20 byte>
                     <deltabase location: 4 byte signed int>
                     <pack entry offset: 8 byte unsigned int>
                     <pack entry size: 8 byte unsigned int>

    [1]: new in version 1.
    """
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    # v[01] index format: <node><delta offset><pack data offset><pack data size>
    INDEXFORMAT = datapack.INDEXFORMAT
    INDEXENTRYLENGTH = datapack.INDEXENTRYLENGTH

    # v1 has metadata support
    SUPPORTED_VERSIONS = [0, 1]

    def add(self, name, node, deltabasenode, delta, metadata=None):
        # metadata is a dict, ex. {METAKEYFLAG: flag}
        if len(name) > 2**16:
            raise RuntimeError(_("name too long %s") % name)
        if len(node) != 20:
            raise RuntimeError(_("node should be 20 bytes %s") % node)

        if node in self.entries:
            # The revision has already been added
            return

        # TODO: allow configurable compression
        delta = lz4compress(delta)

        rawdata = ''.join((
            struct.pack('!H', len(name)), # unsigned 2 byte int
            name,
            node,
            deltabasenode,
            struct.pack('!Q', len(delta)), # unsigned 8 byte int
            delta,
        ))

        if self.VERSION == 1:
            # v1 support metadata
            rawmeta = shallowutil.buildpackmeta(metadata)
            rawdata += struct.pack('!I', len(rawmeta)) # unsigned 4 byte
            rawdata += rawmeta
        else:
            # v0 cannot store metadata, raise if metadata contains flag
            if metadata and metadata.get(constants.METAKEYFLAG, 0) != 0:
                raise error.ProgrammingError('v0 pack cannot store flags')

        offset = self.packfp.tell()

        size = len(rawdata)

        self.entries[node] = (deltabasenode, offset, size)

        self.writeraw(rawdata)

    def createindex(self, nodelocations, indexoffset):
        entries = sorted((n, db, o, s) for n, (db, o, s)
                         in self.entries.iteritems())

        rawindex = ''
        fmt = self.INDEXFORMAT
        for node, deltabase, offset, size in entries:
            if deltabase == nullid:
                deltabaselocation = FULLTEXTINDEXMARK
            else:
                # Instead of storing the deltabase node in the index, let's
                # store a pointer directly to the index entry for the deltabase.
                deltabaselocation = nodelocations.get(deltabase,
                                                      NOBASEINDEXMARK)

            entry = struct.pack(fmt, node, deltabaselocation, offset, size)
            rawindex += entry

        return rawindex
