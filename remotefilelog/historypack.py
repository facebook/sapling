import lz4, mmap, os, struct, tempfile
from collections import defaultdict, deque
from mercurial import mdiff, util
from mercurial.node import nullid, bin, hex
from mercurial.i18n import _
import shallowutil

# (filename hash, offset, size)
INDEXFORMAT = '!20sQQ'
INDEXENTRYLENGTH = 36
NODELENGTH = 20

# (node, p1, p2, linknode)
PACKFORMAT = "!20s20s20s20s"
PACKENTRYLENGTH = 80

# The fanout prefix is the number of bytes that can be addressed by the fanout
# table. Example: a fanout prefix of 1 means we use the first byte of a hash to
# look in the fanout table (which will be 2^8 entries long).
FANOUTPREFIX = 2
# The struct pack format for fanout table location (i.e. the format that
# converts the node prefix into an integer location in the fanout table).
FANOUTSTRUCT = '!H'
# The number of fanout table entries
FANOUTCOUNT = 2**(FANOUTPREFIX * 8)
# The total bytes used by the fanout table
FANOUTENTRYSTRUCT = '!I'
FANOUTENTRYSIZE = 4
FANOUTSIZE = FANOUTCOUNT * FANOUTENTRYSIZE

INDEXSUFFIX = '.histidx'
PACKSUFFIX = '.histpack'

VERSION = 0
VERSIONSIZE = 1

FANOUTSTART = VERSIONSIZE
INDEXSTART = FANOUTSTART + FANOUTSIZE

class AncestorIndicies(object):
    NODE = 0
    P1NODE = 1
    P2NODE = 2
    LINKNODE = 3

class historypackstore(object):
    def __init__(self, path):
        self.packs = []
        suffixlen = len(INDEXSUFFIX)
        for root, dirs, files in os.walk(path):
            for filename in files:
                packfilename = '%s%s' % (filename[:-suffixlen], PACKSUFFIX)
                if (filename[-suffixlen:] == INDEXSUFFIX
                    and packfilename in files):
                    packpath = os.path.join(root, filename)
                    self.packs.append(historypack(packpath[:-suffixlen]))

    def getmissing(self, keys):
        missing = keys
        for pack in self.packs:
            missing = pack.getmissing(missing)

        return missing

    def getparents(self, name, node):
        for pack in self.packs:
            try:
                return pack.getparents(name, node)
            except KeyError as ex:
                pass

        raise KeyError((name, node))

    def getancestors(self, name, node):
        for pack in self.packs:
            try:
                return pack.getancestors(name, node)
            except KeyError as ex:
                pass

        raise KeyError((name, node))

    def getlinknode(self, name, node):
        for pack in self.packs:
            try:
                return pack.getlinknode(name, node)
            except KeyError as ex:
                pass

        raise KeyError((name, node))

    def add(self, filename, node, p1, p2, linknode):
        raise RuntimeError("cannot add to historypackstore (%s:%s)"
                           % (filename, hex(node)))

    def markledger(self, ledger):
        for pack in self.packs:
            pack.markledger(ledger)

class historypack(object):
    def __init__(self, path):
        self.path = path
        self.packpath = path + PACKSUFFIX
        self.indexpath = path + INDEXSUFFIX
        self.indexfp = open(self.indexpath, 'r+b')
        self.datafp = open(self.packpath, 'r+b')

        self.indexsize = os.stat(self.indexpath).st_size
        self.datasize = os.stat(self.packpath).st_size

        # memory-map the file, size 0 means whole file
        self._index = mmap.mmap(self.indexfp.fileno(), 0)
        self._data = mmap.mmap(self.datafp.fileno(), 0)

        version = struct.unpack('!B', self._data[:VERSIONSIZE])[0]
        if version != VERSION:
            raise RuntimeError("unsupported histpack version '%s'" %
                               version)
        version = struct.unpack('!B', self._index[:VERSIONSIZE])[0]
        if version != VERSION:
            raise RuntimeError("unsupported histpack index version '%s'" %
                               version)

        rawfanout = self._index[FANOUTSTART:FANOUTSTART + FANOUTSIZE]
        self._fanouttable = []
        for i in range(0, FANOUTCOUNT):
            loc = i * FANOUTENTRYSIZE
            fanoutentry = struct.unpack(FANOUTENTRYSTRUCT,
                    rawfanout[loc:loc + FANOUTENTRYSIZE])[0]
            self._fanouttable.append(fanoutentry)

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            section = self._findsection(name)
            if not section:
                missing.append((name, node))
                continue
            try:
                value = self._findnode(section, node)
            except KeyError:
                missing.append((name, node))

        return missing

    def getparents(self, name, node):
        section = self._findsection(name)
        node, p1, p2, linknode = self._findnode(section, node)
        return p1, p2

    def getancestors(self, name, node):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode),
           ...
        }
        """
        filename, offset, size = self._findsection(name)
        ancestors = set((node,))
        results = {}
        for o in range(offset, offset + size, PACKENTRYLENGTH):
            entry = struct.unpack(PACKFORMAT,
                                  self._data[o:o + PACKENTRYLENGTH])
            if entry[AncestorIndicies.NODE] in ancestors:
                ancestors.add(entry[AncestorIndicies.P1NODE])
                ancestors.add(entry[AncestorIndicies.P2NODE])
                result = (entry[AncestorIndicies.P1NODE],
                          entry[AncestorIndicies.P2NODE],
                          entry[AncestorIndicies.LINKNODE],
                          # Add a fake None for the copyfrom entry for now
                          # TODO: remove copyfrom from getancestor api
                          None)
                results[entry[AncestorIndicies.NODE]] = result

        if not results:
            raise KeyError((name, node))
        return results

    def getlinknode(self, name, node):
        section = self._findsection(name)
        node, p1, p2, linknode = self._findnode(section, node)
        return linknode

    def add(self, filename, node, p1, p2, linknode):
        raise RuntimeError("cannot add to historypack (%s:%s)" %
                           (filename, hex(node)))

    def _findnode(self, section, node):
        name, offset, size = section
        data = self._data
        for i in range(offset, offset + size, PACKENTRYLENGTH):
            entry = struct.unpack(PACKFORMAT,
                                  data[i:i + PACKENTRYLENGTH])
            if entry[0] == node:
                return entry

        raise KeyError("unable to find history for %s:%s" % (name, hex(node)))

    def _findsection(self, name):
        namehash = util.sha1(name).digest()
        fanoutkey = struct.unpack(FANOUTSTRUCT, namehash[:FANOUTPREFIX])[0]
        fanout = self._fanouttable

        start = fanout[fanoutkey] + INDEXSTART
        if fanoutkey < FANOUTCOUNT - 1:
            end = self._fanouttable[fanoutkey + 1] + INDEXSTART
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
                mid = mid - ((mid - INDEXSTART) % INDEXENTRYLENGTH)
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
        filenamelength = struct.unpack('!H', self._data[offset:offset + 2])[0]
        offset += 2

        actualname = self._data[offset:offset + filenamelength]
        offset += filenamelength

        if name != actualname:
            raise KeyError("found file name %s when looking for %s" %
                           (actualname, name))

        revcount = struct.unpack('!I', self._data[offset:offset + 4])[0]
        offset += 4

        return (name, offset, revcount * PACKENTRYLENGTH)

    def markledger(self, ledger):
        for filename, node in self._iterkeys():
            ledger.markhistoryentry(self, filename, node)

    def cleanup(self, ledger):
        entries = ledger.sources.get(self, [])
        allkeys = set(self._iterkeys())
        repackedkeys = set((e.filename, e.node) for e in entries if
                           e.historyrepacked)

        if len(allkeys - repackedkeys) == 0:
            util.unlinkpath(self.indexpath, ignoremissing=True)
            util.unlinkpath(self.packpath, ignoremissing=True)

    def _iterkeys(self):
        # Start at 1 to skip the header
        offset = 1
        data = self._data
        while offset < self.datasize:
            # <2 byte len> + <filename>
            filenamelen = struct.unpack('!H', data[offset:offset + 2])[0]
            assert (filenamelen > 0)
            offset += 2
            filename = data[offset:offset + filenamelen]
            offset += filenamelen

            revcount = struct.unpack('!I', data[offset:offset + 4])[0]
            offset += 4

            assert (offset + 80 * revcount <= self.datasize)
            for i in xrange(revcount):
                node = data[offset:offset + 20]
                offset += 80
                yield (filename, node)

class mutablehistorypack(object):
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

        The revisions within each filesection are stored in topological order
        (newest first).

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
    def __init__(self, packdir):
        self.packdir = packdir
        self.entries = []
        self.packfp, self.historypackpath = tempfile.mkstemp(
                suffix=PACKSUFFIX + '-tmp',
                dir=packdir)
        self.idxfp, self.historyidxpath = tempfile.mkstemp(
                suffix=INDEXSUFFIX + '-tmp',
                dir=packdir)
        self.packfp = os.fdopen(self.packfp, 'w+')
        self.idxfp = os.fdopen(self.idxfp, 'w+')
        self.sha = util.sha1()
        self._closed = False

        # Write header
        # TODO: make it extensible
        version = struct.pack('!B', VERSION) # unsigned 1 byte int
        self.writeraw(version)

        self.pastfiles = {}
        self.currentfile = None
        self.currententries = []

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        if exc_type is None:
            if not self._closed:
                self.close()
        else:
            # Unclean exit
            try:
                os.unlink(self.historypackpath)
                os.unlink(self.historyidxpath)
            except Exception:
                pass

    def add(self, filename, node, p1, p2, linknode):
        if filename != self.currentfile:
            if filename in self.pastfiles:
                raise RuntimeError("cannot add file node after another file's "
                                   "nodes have been added")
            if self.currentfile:
                self._writependingsection()

            self.currentfile = filename
            self.currententries = []

        self.currententries.append((node, p1, p2, linknode))

    def _writependingsection(self):
        filename = self.currentfile
        self.pastfiles[filename] = (
            self.packfp.tell(),
            # Length of the section = filename len (2) + length of
            # filename + entry count (4) + entries (80 each)
            2 + len(filename) + 4 + (len(self.currententries) * 80),
        )

        # Write the file section header
        self.writeraw("%s%s%s" % (
            struct.pack('!H', len(filename)),
            filename,
            struct.pack('!I', len(self.currententries)),
        ))

        # Write the file section content
        rawdata = ''.join('%s%s%s%s' % e for e in self.currententries)
        self.writeraw(rawdata)

    def writeraw(self, data):
        self.packfp.write(data)
        self.sha.update(data)

    def close(self):
        if self.currentfile:
            self._writependingsection()

        sha = self.sha.hexdigest()
        self.packfp.close()
        self.writeindex()

        os.rename(self.historypackpath, os.path.join(self.packdir, sha +
                                                     PACKSUFFIX))
        os.rename(self.historyidxpath, os.path.join(self.packdir, sha +
                                                    INDEXSUFFIX))

        self._closed = True
        return os.path.join(self.packdir, sha)

    def writeindex(self):
        files = ((util.sha1(node).digest(), offset, size)
                for node, (offset, size) in self.pastfiles.iteritems())
        files = sorted(files)
        rawindex = ""

        fanouttable = [-1] * FANOUTCOUNT

        count = 0
        for namehash, offset, size in files:
            location = count * INDEXENTRYLENGTH
            count += 1

            fanoutkey = struct.unpack(FANOUTSTRUCT, namehash[:FANOUTPREFIX])[0]
            if fanouttable[fanoutkey] == -1:
                fanouttable[fanoutkey] = location

            rawindex += struct.pack(INDEXFORMAT, namehash, offset, size)

        rawfanouttable = ''
        last = 0
        for offset in fanouttable:
            offset = offset if offset != -1 else last
            last = offset
            rawfanouttable += struct.pack(FANOUTENTRYSTRUCT, offset)

        self.idxfp.write(struct.pack('!B', VERSION))
        self.idxfp.write(rawfanouttable)
        self.idxfp.write(rawindex)
        self.idxfp.close()
