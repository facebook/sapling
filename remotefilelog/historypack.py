import errno, hashlib, lz4, mmap, os, struct, tempfile
from collections import defaultdict, deque
from mercurial import mdiff, osutil, util
from mercurial.node import nullid, bin, hex
from mercurial.i18n import _
import basepack, constants, shallowutil

# (filename hash, offset, size)
INDEXFORMAT = '!20sQQ'
INDEXENTRYLENGTH = 36
NODELENGTH = 20

# (node, p1, p2, linknode)
PACKFORMAT = "!20s20s20s20sH"
PACKENTRYLENGTH = 82

OFFSETSIZE = 4

INDEXSUFFIX = '.histidx'
PACKSUFFIX = '.histpack'

ANC_NODE = 0
ANC_P1NODE = 1
ANC_P2NODE = 2
ANC_LINKNODE = 3
ANC_COPYFROM = 4

class historypackstore(basepack.basepackstore):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def getpack(self, path):
        return historypack(path)

    def getancestors(self, name, node):
        for pack in self.packs:
            try:
                return pack.getancestors(name, node)
            except KeyError as ex:
                pass

        for pack in self.refresh():
            try:
                return pack.getancestors(name, node)
            except KeyError:
                pass

        raise KeyError((name, node))

    def add(self, filename, node, p1, p2, linknode, copyfrom):
        raise RuntimeError("cannot add to historypackstore (%s:%s)"
                           % (filename, hex(node)))

class historypack(basepack.basepack):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            try:
                section = self._findsection(name)
                value = self._findnode(section, node)
            except KeyError:
                missing.append((name, node))

        return missing

    def getancestors(self, name, node):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode, copyfrom),
           ...
        }
        """
        filename, offset, size = self._findsection(name)
        ancestors = set((node,))
        data = self._data[offset:offset + size]
        results = {}
        o = 0
        while o < len(data):
            entry = struct.unpack(PACKFORMAT, data[o:o + PACKENTRYLENGTH])
            o += PACKENTRYLENGTH
            copyfrom = None
            copyfromlen = entry[ANC_COPYFROM]
            if copyfromlen != 0:
                copyfrom = data[o:o + copyfromlen]
                o += copyfromlen

            if entry[ANC_NODE] in ancestors:
                ancestors.add(entry[ANC_P1NODE])
                ancestors.add(entry[ANC_P2NODE])
                result = (entry[ANC_P1NODE],
                          entry[ANC_P2NODE],
                          entry[ANC_LINKNODE],
                          copyfrom)
                results[entry[ANC_NODE]] = result

            self._pagedin += PACKENTRYLENGTH

        # If we've read a lot of data from the mmap, free some memory.
        self.freememory()

        if not results:
            raise KeyError((name, node))
        return results

    def add(self, filename, node, p1, p2, linknode, copyfrom):
        raise RuntimeError("cannot add to historypack (%s:%s)" %
                           (filename, hex(node)))

    def _findnode(self, section, node):
        name, offset, size = section
        data = self._data
        o = offset
        while o < offset + size:
            entry = struct.unpack(PACKFORMAT,
                                  data[o:o + PACKENTRYLENGTH])
            o += PACKENTRYLENGTH

            if entry[0] == node:
                copyfrom = None
                copyfromlen = entry[ANC_COPYFROM]
                if copyfromlen != 0:
                    copyfrom = data[o:o + copyfromlen]

                # If we've read a lot of data from the mmap, free some memory.
                self._pagedin += o - offset
                self.freememory()
                return (entry[ANC_P1NODE],
                        entry[ANC_P2NODE],
                        entry[ANC_LINKNODE],
                        copyfrom)

            o += entry[ANC_COPYFROM]

        raise KeyError("unable to find history for %s:%s" % (name, hex(node)))

    def _findsection(self, name):
        params = self.params
        namehash = hashlib.sha1(name).digest()
        fanoutkey = struct.unpack(params.fanoutstruct,
                                  namehash[:params.fanoutprefix])[0]
        fanout = self._fanouttable

        start = fanout[fanoutkey] + params.indexstart
        for i in xrange(fanoutkey + 1, params.fanoutcount):
            end = fanout[i] + params.indexstart
            if end != start:
                break
        else:
            end = self.indexsize

        # Bisect between start and end to find node
        index = self._index
        startnode = self._index[start:start + NODELENGTH]
        endnode = self._index[end:end + NODELENGTH]
        if startnode == namehash:
            entry = self._index[start:start + INDEXENTRYLENGTH]
        elif endnode == namehash:
            entry = self._index[end:end + INDEXENTRYLENGTH]
        else:
            iteration = 0
            while start < end - INDEXENTRYLENGTH:
                iteration += 1
                mid = start  + (end - start) / 2
                mid = mid - ((mid - params.indexstart) % INDEXENTRYLENGTH)
                midnode = self._index[mid:mid + NODELENGTH]
                if midnode == namehash:
                    entry = self._index[mid:mid + INDEXENTRYLENGTH]
                    break
                if namehash > midnode:
                    start = mid
                    startnode = midnode
                elif namehash < midnode:
                    end = mid
                    endnode = midnode
            else:
                raise KeyError(name)

        filenamehash, offset, size = struct.unpack(INDEXFORMAT, entry)
        filenamelength = struct.unpack('!H', self._data[offset:offset +
                                                    constants.FILENAMESIZE])[0]
        offset += constants.FILENAMESIZE

        actualname = self._data[offset:offset + filenamelength]
        offset += filenamelength

        if name != actualname:
            raise KeyError("found file name %s when looking for %s" %
                           (actualname, name))

        revcount = struct.unpack('!I', self._data[offset:offset +
                                                  OFFSETSIZE])[0]
        offset += OFFSETSIZE

        return (name, offset, size - constants.FILENAMESIZE - filenamelength
                              - OFFSETSIZE)

    def markledger(self, ledger):
        for filename, node in self:
            ledger.markhistoryentry(self, filename, node)

    def cleanup(self, ledger):
        entries = ledger.sources.get(self, [])
        allkeys = set(self)
        repackedkeys = set((e.filename, e.node) for e in entries if
                           e.historyrepacked)

        if len(allkeys - repackedkeys) == 0:
            if self.path not in ledger.created:
                util.unlinkpath(self.indexpath, ignoremissing=True)
                util.unlinkpath(self.packpath, ignoremissing=True)

    def __iter__(self):
        for f, n, x, x, x, x in self.iterentries():
            yield f, n

    def iterentries(self):
        # Start at 1 to skip the header
        offset = 1
        while offset < self.datasize:
            data = self._data
            # <2 byte len> + <filename>
            filenamelen = struct.unpack('!H', data[offset:offset +
                                                   constants.FILENAMESIZE])[0]
            assert (filenamelen > 0)
            offset += constants.FILENAMESIZE
            filename = data[offset:offset + filenamelen]
            offset += filenamelen

            revcount = struct.unpack('!I', data[offset:offset +
                                                OFFSETSIZE])[0]
            offset += OFFSETSIZE

            for i in xrange(revcount):
                entry = struct.unpack(PACKFORMAT, data[offset:offset +
                                                              PACKENTRYLENGTH])
                offset += PACKENTRYLENGTH

                copyfrom = data[offset:offset + entry[ANC_COPYFROM]]
                offset += entry[ANC_COPYFROM]

                yield (filename, entry[ANC_NODE], entry[ANC_P1NODE],
                        entry[ANC_P2NODE], entry[ANC_LINKNODE], copyfrom)

                self._pagedin += PACKENTRYLENGTH

            # If we've read a lot of data from the mmap, free some memory.
            self.freememory()

class mutablehistorypack(basepack.mutablebasepack):
    """A class for constructing and serializing a histpack file and index.

    A history pack is a pair of files that contain the revision history for
    various file revisions in Mercurial. It contains only revision history (like
    parent pointers and linknodes), not any revision content information.

    It consists of two files, with the following format:

    .histpack
        The pack itself is a series of file revisions with some basic header
        information on each.

        datapack = <version: 1 byte>
                   [<filesection>,...]
        filesection = <filename len: 2 byte unsigned int>
                      <filename>
                      <revision count: 4 byte unsigned int>
                      [<revision>,...]
        revision = <node: 20 byte>
                   <p1node: 20 byte>
                   <p2node: 20 byte>
                   <linknode: 20 byte>
                   <copyfromlen: 2 byte>
                   <copyfrom>

        The revisions within each filesection are stored in topological order
        (newest first). If a given entry has a parent from another file (a copy)
        then p1node is the node from the other file, and copyfrom is the
        filepath of the other file.

    .histidx
        The index file provides a mapping from filename to the file section in
        the histpack. It consists of two parts, the fanout and the index.

        The index is a list of index entries, sorted by filename hash (one per
        file section in the pack). Each entry has:

        - node (The 20 byte hash of the filename)
        - pack entry offset (The location of this file section in the histpack)
        - pack content size (The on-disk length of this file section's pack
                             data)

        The fanout is a quick lookup table to reduce the number of steps for
        bisecting the index. It is a series of 4 byte pointers to positions
        within the index. It has 2^16 entries, which corresponds to hash
        prefixes [00, 01, 02,..., FD, FE, FF]. Example: the pointer in slot 4F
        points to the index position of the first revision whose node starts
        with 4F. This saves log(2^16) bisect steps.

        dataidx = <fanouttable>
                  <index>
        fanouttable = [<index offset: 4 byte unsigned int>,...] (2^16 entries)
        index = [<index entry>,...]
        indexentry = <node: 20 byte>
                     <pack file section offset: 8 byte unsigned int>
                     <pack file section size: 8 byte unsigned int>
    """
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX
    INDEXENTRYLENGTH = INDEXENTRYLENGTH

    def __init__(self, ui, opener):
        super(mutablehistorypack, self).__init__(ui, opener)
        self.pastfiles = {}
        self.currentfile = None
        self.currententries = []

    def add(self, filename, node, p1, p2, linknode, copyfrom):
        if filename != self.currentfile:
            if filename in self.pastfiles:
                raise RuntimeError("cannot add file node after another file's "
                                   "nodes have been added")
            if self.currentfile:
                self._writependingsection()

            self.currentfile = filename
            self.currententries = []

        copyfrom = copyfrom or ''
        copyfromlen = struct.pack('!H', len(copyfrom))
        self.currententries.append((node, p1, p2, linknode, copyfromlen,
                                    copyfrom))

    def _writependingsection(self):
        filename = self.currentfile
        sectionstart = self.packfp.tell()

        # Write the file section header
        self.writeraw("%s%s%s" % (
            struct.pack('!H', len(filename)),
            filename,
            struct.pack('!I', len(self.currententries)),
        ))
        sectionlen = constants.FILENAMESIZE + len(filename) + 4

        # Write the file section content
        rawdata = ''.join('%s%s%s%s%s%s' % e for e in self.currententries)
        sectionlen += len(rawdata)

        self.writeraw(rawdata)

        self.pastfiles[filename] = (sectionstart, sectionlen)
        node = hashlib.sha1(filename).digest()
        self.entries[node] = node

    def close(self, ledger=None):
        if self.currentfile:
            self._writependingsection()

        return super(mutablehistorypack, self).close(ledger=ledger)

    def createindex(self, nodelocations):
        files = ((hashlib.sha1(filename).digest(), offset, size)
                for filename, (offset, size) in self.pastfiles.iteritems())
        files = sorted(files)

        rawindex = ""
        for namehash, offset, size in files:
            rawindex += struct.pack(INDEXFORMAT, namehash, offset, size)

        return rawindex
