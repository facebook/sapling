# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import struct

from edenscm.mercurial import error, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex, nullid
from edenscmnative.bindings import revisionstore

from . import basepack, constants, shallowutil
from .lz4wrapper import lz4compress, lz4decompress


try:
    xrange(0)
except NameError:
    xrange = range

try:
    from edenscmnative import cstore

    cstore.datapack
except ImportError:
    cstore = None

NODELENGTH = 20

# The indicator value in the index for a fulltext entry.
FULLTEXTINDEXMARK = -1
NOBASEINDEXMARK = -2

INDEXSUFFIX = ".dataidx"
PACKSUFFIX = ".datapack"


class datapackstore(basepack.basepackstore):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def __init__(
        self,
        ui,
        path,
        usecdatapack=False,
        deletecorruptpacks=False,
        userustdatapack=False,
    ):
        self.usecdatapack = usecdatapack
        self.userustdatapack = userustdatapack
        super(datapackstore, self).__init__(
            ui, path, deletecorruptpacks=deletecorruptpacks
        )

    def getpack(self, path):
        if self.userustdatapack:
            return revisionstore.datapack(path)
        elif self.usecdatapack:
            return fastdatapack(path)
        else:
            return datapack(path)

    def get(self, name, node):
        raise RuntimeError("must use getdeltachain with datapackstore")

    def getmeta(self, name, node):
        def func(pack):
            return pack.getmeta(name, node)

        for meta in self.runonpacks(func):
            return meta

        raise KeyError((name, hex(node)))

    def getdelta(self, name, node):
        def func(pack):
            return pack.getdelta(name, node)

        for delta in self.runonpacks(func):
            return delta

        raise KeyError((name, hex(node)))

    def getdeltachain(self, name, node):
        def func(pack):
            return pack.getdeltachain(name, node)

        for deltachain in self.runonpacks(func):
            return deltachain

        raise KeyError((name, hex(node)))

    def add(self, name, node, data):
        raise RuntimeError("cannot add to datapackstore")

    def repackstore(self, incremental=True):
        if self.fetchpacksenabled:
            revisionstore.repackincrementaldatapacks(self.path, self.path)


class datapack(basepack.basepack):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    # Format is <node><delta offset><pack data offset><pack data size>
    # See the mutabledatapack doccomment for more details.
    INDEXFORMAT = "!20siQQ"
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
        raise RuntimeError(
            "must use getdeltachain with datapack (%s:%s)" % (name, hex(node))
        )

    def getmeta(self, name, node):
        value = self._find(node)
        if value is None:
            raise KeyError((name, hex(node)))

        # version 0 does not support metadata
        if self.VERSION == 0:
            return {}

        node, deltabaseoffset, offset, size = value
        rawentry = self._data[offset : offset + size]

        # see docstring of mutabledatapack for the format
        offset = 0
        offset += struct.unpack_from("!H", rawentry, offset)[0] + 2  # filename
        offset += 40  # node, deltabase node
        offset += struct.unpack_from("!Q", rawentry, offset)[0] + 8  # delta

        metalen = struct.unpack_from("!I", rawentry, offset)[0]
        offset += 4

        meta = shallowutil.parsepackmeta(rawentry[offset : offset + metalen])

        return meta

    def getdelta(self, name, node):
        value = self._find(node)
        if value is None:
            raise KeyError((name, hex(node)))

        node, deltabaseoffset, offset, size = value
        entry = self._readentry(offset, size, getmeta=True)
        filename, node, deltabasenode, delta, meta = entry

        # If we've read a lot of data from the mmap, free some memory.
        self.freememory()

        return delta, filename, deltabasenode, meta

    def getdeltachain(self, name, node):
        value = self._find(node)
        if value is None:
            raise KeyError((name, hex(node)))

        params = self.params

        # Precompute chains
        chain = [value]
        deltabaseoffset = value[1]
        entrylen = self.INDEXENTRYLENGTH
        while (
            deltabaseoffset != FULLTEXTINDEXMARK and deltabaseoffset != NOBASEINDEXMARK
        ):
            loc = params.indexstart + deltabaseoffset
            value = struct.unpack(self.INDEXFORMAT, self._index[loc : loc + entrylen])
            deltabaseoffset = value[1]
            chain.append(value)

        # Read chain data
        deltachain = []
        for node, deltabaseoffset, offset, size in chain:
            filename, node, deltabasenode, delta = self._readentry(offset, size)
            deltachain.append((filename, node, filename, deltabasenode, delta))

        # If we've read a lot of data from the mmap, free some memory.
        self.freememory()

        return deltachain

    def _readentry(self, offset, size, getmeta=False):
        rawentry = self._data[offset : offset + size]
        self._pagedin += len(rawentry)
        return _readdataentry(rawentry, self.VERSION, getmeta=getmeta)

    def add(self, name, node, data):
        raise RuntimeError("cannot add to datapack (%s:%s)" % (name, node))

    def _find(self, node):
        params = self.params
        fanoutkey = struct.unpack(params.fanoutstruct, node[: params.fanoutprefix])[0]
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
        startnode = index[start : start + NODELENGTH]
        endnode = index[end : end + NODELENGTH]
        entrylen = self.INDEXENTRYLENGTH
        if startnode == node:
            entry = index[start : start + entrylen]
        elif endnode == node:
            entry = index[end : end + entrylen]
        else:
            while start < end - entrylen:
                mid = start + (end - start) / 2
                mid = mid - ((mid - params.indexstart) % entrylen)
                midnode = index[mid : mid + NODELENGTH]
                if midnode == node:
                    entry = index[mid : mid + entrylen]
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

    def markledger(self, ledger, options=None):
        if options and options.get(constants.OPTION_LOOSEONLY):
            return

        with ledger.location(self._path):
            for filename, node in self:
                ledger.markdataentry(self, filename, node)

    def cleanup(self, ledger):
        entries = ledger.sources.get(self, [])
        allkeys = set(self)
        repackedkeys = set(
            (e.filename, e.node) for e in entries if e.datarepacked or e.gced
        )

        if len(allkeys - repackedkeys) == 0:
            if self._path not in ledger.created:
                util.unlinkpath(self.indexpath(), ignoremissing=True)
                util.unlinkpath(self.packpath(), ignoremissing=True)

    def __iter__(self):
        for f, n, deltabase, deltalen in self.iterentries():
            yield f, n

    def iterentries(self, yieldall=False):
        """Yields (filename, node, deltabase, datalength) for each entry.

        If ``yieldall`` is True, yields
          (filename, node, deltabase, datalength, delta, meta).
        """
        # Start at 1 to skip the header
        offset = 1
        data = self._data
        delta = None
        meta = None
        while offset < self.datasize:
            oldoffset = offset

            # <2 byte len> + <filename>
            filenamelen = struct.unpack("!H", data[offset : offset + 2])[0]
            offset += 2
            filename = data[offset : offset + filenamelen]
            offset += filenamelen

            # <20 byte node>
            node = data[offset : offset + constants.NODESIZE]
            offset += constants.NODESIZE
            # <20 byte deltabase>
            deltabase = data[offset : offset + constants.NODESIZE]
            offset += constants.NODESIZE

            # <8 byte len> + <delta>
            rawdeltalen = data[offset : offset + 8]
            deltalen = struct.unpack("!Q", rawdeltalen)[0]
            offset += 8

            # it has to be at least long enough for the lz4 header.
            assert deltalen >= 4

            if yieldall:
                delta = lz4decompress(data[offset : offset + deltalen])

            # python-lz4 stores the length of the uncompressed field as a
            # little-endian 32-bit integer at the start of the data.
            uncompressedlen = struct.unpack("<I", data[offset : offset + 4])[0]
            offset += deltalen

            if self.VERSION == 1:
                # <4 byte len> + <metadata-list>
                metalen = struct.unpack("!I", data[offset : offset + 4])[0]
                offset += 4
                if yieldall:
                    meta = data[offset : offset + metalen]
                offset += metalen

            if yieldall:
                yield (filename, node, deltabase, uncompressedlen, delta, meta)
            else:
                yield (filename, node, deltabase, uncompressedlen)

            # If we've read a lot of data from the mmap, free some memory.
            self._pagedin += offset - oldoffset
            if self.freememory():
                data = self._data


def _readdataentry(rawentry, version, getmeta=False):
    # <2 byte len> + <filename>
    lengthsize = 2
    filenamelen = struct.unpack("!H", rawentry[:2])[0]
    filename = rawentry[lengthsize : lengthsize + filenamelen]

    # <20 byte node> + <20 byte deltabase>
    nodestart = lengthsize + filenamelen
    deltabasestart = nodestart + NODELENGTH
    node = rawentry[nodestart:deltabasestart]
    deltabasenode = rawentry[deltabasestart : deltabasestart + NODELENGTH]

    # <8 byte len> + <delta>
    deltastart = deltabasestart + NODELENGTH
    rawdeltalen = rawentry[deltastart : deltastart + 8]
    deltalen = struct.unpack("!Q", rawdeltalen)[0]

    delta = rawentry[deltastart + 8 : deltastart + 8 + deltalen]
    delta = lz4decompress(delta)

    if getmeta:
        if version == 0:
            meta = {}
        else:
            metastart = deltastart + 8 + deltalen
            metalen = struct.unpack_from("!I", rawentry, metastart)[0]

            rawmeta = rawentry[metastart + 4 : metastart + 4 + metalen]
            meta = shallowutil.parsepackmeta(rawmeta)
        return filename, node, deltabasenode, delta, meta
    else:
        return filename, node, deltabasenode, delta


class fastdatapack(basepack.basepack):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def __init__(self, path):
        self._path = path
        self._packpath = path + self.PACKSUFFIX
        self._indexpath = path + self.INDEXSUFFIX
        self.datapack = cstore.datapack(path)

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            value = self.datapack._find(node)
            if not value:
                missing.append((name, node))

        return missing

    def get(self, name, node):
        raise RuntimeError(
            "must use getdeltachain with datapack (%s:%s)" % (name, hex(node))
        )

    def getmeta(self, name, node):
        return self.datapack.getmeta(node)

    def getdelta(self, name, node):
        result = self.datapack.getdelta(node)
        if result is None:
            raise KeyError((name, hex(node)))

        delta, deltabasenode, meta = result
        return delta, name, deltabasenode, meta

    def getdeltachain(self, name, node):
        result = self.datapack.getdeltachain(node)
        if result is None:
            raise KeyError((name, hex(node)))

        return result

    def add(self, name, node, data):
        raise RuntimeError("cannot add to datapack (%s:%s)" % (name, node))

    def markledger(self, ledger, options=None):
        if options and options.get(constants.OPTION_LOOSEONLY):
            return

        with ledger.location(self._path):
            for filename, node in self:
                ledger.markdataentry(self, filename, node)

    def cleanup(self, ledger):
        entries = ledger.sources.get(self, [])
        allkeys = set(self)
        repackedkeys = set(
            (e.filename, e.node) for e in entries if e.datarepacked or e.gced
        )

        if len(allkeys - repackedkeys) == 0:
            if self._path not in ledger.created:
                util.unlinkpath(self.indexpath(), ignoremissing=True)
                util.unlinkpath(self.packpath(), ignoremissing=True)

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
        if len(name) > 2 ** 16:
            raise RuntimeError(_("name too long %s") % name)
        if len(node) != 20:
            raise RuntimeError(_("node should be 20 bytes %s") % node)

        if node in self.entries:
            # The revision has already been added
            return

        # TODO: allow configurable compression
        delta = lz4compress(delta)

        rawdata = "%s%s%s%s%s%s" % (
            struct.pack("!H", len(name)),  # unsigned 2 byte int
            name,
            node,
            deltabasenode,
            struct.pack("!Q", len(delta)),  # unsigned 8 byte int
            delta,
        )

        if self.VERSION == 1:
            # v1 support metadata
            rawmeta = shallowutil.buildpackmeta(metadata)
            rawdata += struct.pack("!I", len(rawmeta))  # unsigned 4 byte
            rawdata += rawmeta
        else:
            # v0 cannot store metadata, raise if metadata contains flag
            if metadata and metadata.get(constants.METAKEYFLAG, 0) != 0:
                raise error.ProgrammingError("v0 pack cannot store flags")

        offset = self.packfp.tell()

        size = len(rawdata)

        self.entries[node] = (deltabasenode, offset, size)

        self.writeraw(rawdata)

    def createindex(self, nodelocations, indexoffset):
        entries = sorted((n, db, o, s) for n, (db, o, s) in self.entries.iteritems())

        rawindex = ""
        fmt = self.INDEXFORMAT
        for node, deltabase, offset, size in entries:
            if deltabase == nullid:
                deltabaselocation = FULLTEXTINDEXMARK
            else:
                # Instead of storing the deltabase node in the index, let's
                # store a pointer directly to the index entry for the deltabase.
                deltabaselocation = nodelocations.get(deltabase, NOBASEINDEXMARK)

            entry = struct.pack(fmt, node, deltabaselocation, offset, size)
            rawindex += entry

        return rawindex

    def get(self, name, node):
        raise RuntimeError("must use getdeltachain with mutabledatapack")

    def getmeta(self, name, node):
        delta, deltaname, deltabasenode, meta = self.getdelta(name, node)
        return meta

    def getdelta(self, name, node):
        value = self.entries.get(node)
        if value is None:
            raise KeyError(name, hex(node))

        deltabasenode, offset, size = self.entries[node]

        try:
            # Seek to data
            self.packfp.seek(offset, os.SEEK_SET)
            data = self.packfp.read(size)
        finally:
            # Seek back to the end
            self.packfp.seek(0, os.SEEK_END)

        entry = _readdataentry(data, self.VERSION, getmeta=True)
        filename, node, deltabasenode, delta, meta = entry
        return delta, filename, deltabasenode, meta

    def getdeltachain(self, name, node):
        deltachain = []
        while node != nullid:
            try:
                value = self.getdelta(name, node)
                delta, deltaname, deltabasenode, meta = value
                deltachain.append((name, node, deltaname, deltabasenode, delta))
                name = deltaname
                node = deltabasenode
            except KeyError:
                # If we don't even have the first entry, throw. Otherwise return
                # what we have
                if not deltachain:
                    raise
                break

        return deltachain

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            value = self.entries.get(node)
            if value is None:
                missing.append((name, node))

        return missing


class memdatapack(object):
    def __init__(self):
        self.data = {}
        self.meta = {}

    def add(self, name, node, deltabase, delta):
        self.data[(name, node)] = (deltabase, delta)

    def getdelta(self, name, node):
        deltabase, delta = self.data[(name, node)]
        return (delta, name, deltabase, self.getmeta(name, node))

    def getdeltachain(self, name, node):
        deltabase, delta = self.data[(name, node)]
        return [(name, node, name, deltabase, delta)]

    def getmeta(self, name, node):
        return self.meta[(name, node)]

    def getmissing(self, keys):
        missing = []
        for key in keys:
            if key not in self.data:
                missing.append(key)
        return missing
