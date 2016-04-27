import lz4, mmap, os, struct, tempfile
from collections import defaultdict
from mercurial import mdiff, util
from mercurial.node import nullid, bin, hex
from mercurial.i18n import _
import shallowutil

# Index entry format is: <node><delta offset><pack data offset><pack data size>
# See the mutabledatapack doccomment for more details.
INDEXFORMAT = '!20siQQ'
INDEXENTRYLENGTH = 40
NODELENGTH = 20

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
FANOUTSIZE = FANOUTCOUNT * 4

# The datapack version supported by this implementation. This will need to be
# rev'd whenever the byte format changes. Ex: changing the fanout prefix,
# changing any of the int sizes, changing the delta algorithm, etc.
VERSION = 0

# The indicator value in the index for a fulltext entry.
FULLTEXTINDEXMARK = -1

# Constant that indicates a fanout table entry hasn't been filled in. (This does
# not get serialized)
EMPTYFANOUT = -1

INDEXSUFFIX = '.dataidx'
PACKSUFFIX = '.datapack'

class datapackstore(object):
    def __init__(self, path):
        self.packs = []
        suffixlen = len(INDEXSUFFIX)
        for root, dirs, files in os.walk(path):
            for filename in files:
                if (filename[-suffixlen:] == INDEXSUFFIX
                    and ('%s%s' % (filename[:-suffixlen], PACKSUFFIX)) in files):
                    packpath = os.path.join(root, filename)
                    self.packs.append(datapack(packpath[:-suffixlen]))

    def getmissing(self, keys):
        missing = keys
        for pack in self.packs:
            missing = pack.getmissing(missing)

        return missing

    def get(self, name, node):
        raise Exception("must use getdeltachain with datapackstore")

    def getdeltachain(self, name, node):
        for pack in self.packs:
            try:
                return pack.getdeltachain(name, node)
            except KeyError as ex:
                pass

        raise KeyError((name, node))

    def add(self, name, node, data):
        raise Exception("cannot add to datapackstore")

class datapack(object):
    def __init__(self, path):
        self.path = path
        self.packpath = path + PACKSUFFIX
        self.indexpath = path + INDEXSUFFIX
        # TODO: use an opener/vfs to access these paths
        self.indexfp = open(self.indexpath, 'r+b')
        self.datafp = open(self.packpath, 'r+b')

        self.indexsize = os.stat(self.indexpath).st_size
        self.datasize = os.stat(self.packpath).st_size

        self._index = mmap.mmap(self.indexfp.fileno(), 0)

        # memory-map the file, size 0 means whole file
        self._data = mmap.mmap(self.datafp.fileno(), 0)
        version = struct.unpack('!B', self._data[0])[0]
        if version != VERSION:
            raise RuntimeError("unsupported datapack version '%s'" %
                               version)

        rawfanout = self._index[:FANOUTSIZE]
        self._fanouttable = []
        for i in range(0, FANOUTCOUNT):
            loc = i * 4
            self._fanouttable.append(struct.unpack('!I', rawfanout[loc:loc + 4])[0])

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            value = self._find(node)
            if not value:
                missing.append((name, node))

        return missing

    def get(self, name, node):
        raise Exception("must use getdeltachain with datapack (%s:%s)"
                        % (name, hex(node)))

    def getdeltachain(self, name, node):
        value = self._find(node)
        if value is None:
            raise KeyError((name, node))

        # Precompute chains
        chain = [value]
        deltabaseoffset = value[1]
        while deltabaseoffset != -1:
            loc = FANOUTSIZE + deltabaseoffset
            value = struct.unpack(INDEXFORMAT, self._index[loc:loc +
                                                           INDEXENTRYLENGTH])
            deltabaseoffset = value[1]
            chain.append(value)

        # Read chain data
        deltachain = []
        for node, deltabaseoffset, offset, size in chain:
            rawentry = self._data[offset:offset + size]

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
            delta = lz4.decompress(delta)

            deltachain.append((filename, node, filename, deltabasenode, delta))

        return deltachain

    def add(self, name, node, data):
        raise Exception("cannot add to datapack (%s:%s)" % (name, node))

    def _find(self, node):
        fanoutkey = struct.unpack(FANOUTSTRUCT, node[:FANOUTPREFIX])[0]
        fanout = self._fanouttable

        start = fanout[fanoutkey] + FANOUTSIZE
        # Scan forward to find the first non-same entry, which is the upper
        # bound.
        for i in range(fanoutkey + 1, FANOUTCOUNT):
            end = self._fanouttable[i] + FANOUTSIZE
            if end != start:
                break
        else:
            end = self.indexsize

        # Bisect between start and end to find node
        index = self._index
        startnode = self._index[start:start + NODELENGTH]
        endnode = self._index[end:end + NODELENGTH]
        if startnode == node:
            entry = self._index[start:start + INDEXENTRYLENGTH]
        elif endnode == node:
            entry = self._index[end:end + INDEXENTRYLENGTH]
        else:
            iteration = 0
            while start < end - INDEXENTRYLENGTH:
                iteration += 1
                mid = start  + (end - start) / 2
                mid = mid - ((mid - FANOUTSIZE) % INDEXENTRYLENGTH)
                midnode = self._index[mid:mid + NODELENGTH]
                if midnode == node:
                    entry = self._index[mid:mid + INDEXENTRYLENGTH]
                    break
                if node > midnode:
                    start = mid
                    startnode = midnode
                elif node < midnode:
                    end = mid
                    endnode = midnode
            else:
                return None

        return struct.unpack(INDEXFORMAT, entry)

class mutabledatapack(object):
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
    """
    def __init__(self, packdir):
        self.packdir = packdir
        self.entries = {}
        self.packfp, self.datapackpath = tempfile.mkstemp(
            suffix=PACKSUFFIX + '-tmp',
            dir=packdir)
        self.idxfp, self.dataidxpath = tempfile.mkstemp(
            suffix=INDEXSUFFIX + '-tmp',
            dir=packdir)
        self.packfp = os.fdopen(self.packfp, 'w+')
        self.idxfp = os.fdopen(self.idxfp, 'w+')
        self.sha = util.sha1()

        # Write header
        # TODO: make it extensible (ex: allow specifying compression algorithm,
        # a flexible key/value header, delta algorithm, fanout size, etc)
        version = struct.pack('!B', VERSION) # unsigned 1 byte int
        self.writeraw(version)

    def add(self, name, node, deltabasenode, delta):
        if len(name) > 2**16:
            raise RuntimeError(_("name too long %s") % name)
        if len(node) != 20:
            raise RuntimeError(_("node should be 20 bytes %s") % node)

        if node in self.entries:
            # The revision has already been added
            return

        # TODO: allow configurable compression
        delta = lz4.compress(delta)
        rawdata = "%s%s%s%s%s%s" % (
            struct.pack('!H', len(name)), # unsigned 2 byte int
            name,
            node,
            deltabasenode,
            struct.pack('!Q', len(delta)), # unsigned 8 byte int
            delta)

        offset = self.packfp.tell()

        size = len(rawdata)

        self.entries[node] = (deltabasenode, offset, size)

        self.writeraw(rawdata)

    def writeraw(self, data):
        self.packfp.write(data)
        self.sha.update(data)

    def close(self):
        sha = self.sha.hexdigest()
        self.packfp.close()
        self.writeindex()

        os.rename(self.datapackpath, os.path.join(self.packdir, sha + PACKSUFFIX))
        os.rename(self.dataidxpath, os.path.join(self.packdir, sha + INDEXSUFFIX))
        return os.path.join(self.packdir, sha)

    def writeindex(self):
        entries = sorted((n, db, o, s) for n, (db, o, s)
                         in self.entries.iteritems())
        rawindex = ''

        fanouttable = [EMPTYFANOUT] * FANOUTCOUNT

        # Precompute the location of each entry
        locations = {}
        deltaslots = {}
        count = 0
        for node, deltabase, offset, size in entries:
            location = count * INDEXENTRYLENGTH
            locations[node] = location
            count += 1

            # Must use [0] on the unpack result since it's always a tuple.
            fanoutkey = struct.unpack(FANOUTSTRUCT, node[:FANOUTPREFIX])[0]
            if fanouttable[fanoutkey] == EMPTYFANOUT:
                fanouttable[fanoutkey] = location

        for node, deltabase, offset, size in entries:
            if deltabase == nullid:
                deltabaselocation = FULLTEXTINDEXMARK
            else:
                # Instead of storing the deltabase node in the index, let's
                # store a pointer directly to the index entry for the deltabase.
                deltabaselocation = locations[deltabase]

            entry = struct.pack(INDEXFORMAT, node, deltabaselocation, offset, size)
            rawindex += entry

        rawfanouttable = ''
        last = 0
        for offset in fanouttable:
            offset = offset if offset != EMPTYFANOUT else last
            last = offset
            rawfanouttable += struct.pack('!I', offset)

        # TODO: add version number header
        self.idxfp.write(rawfanouttable)
        self.idxfp.write(rawindex)
        self.idxfp.close()

class datagc(object):
    def __init__(self, repo, content, metadata):
        self.repo = repo
        self.content = content
        self.metadata = metadata

    def run(self, source, target):
        ui = self.repo.ui

        files = sorted(source.getfiles())
        count = 0
        for filename, nodes in files:
            ancestors = {}
            for node in nodes:
                ancestors.update(self.metadata.getancestors(filename, node))

            # Order the nodes children first, so we can produce reverse deltas
            orderednodes = reversed(self._toposort(ancestors))

            # getancestors() will return the ancestry of a commit, even across
            # renames. We currently don't support producing deltas across
            # renames, so we use dontprocess to store when an ancestory
            # traverses across a rename, so we can avoid processing those.
            dontprocess = set()

            # Compute deltas and write to the pack
            deltabases = defaultdict(lambda: nullid)
            nodes = set(nodes)
            for node in orderednodes:
                # orderednodes is all ancestors, but we only want to serialize
                # the files we have.
                if node not in nodes:
                    continue
                # Find delta base
                # TODO: allow delta'ing against most recent descendant instead
                # of immediate child
                deltabase = deltabases[node]

                # Record this child as the delta base for its parents.
                # This may be non optimal, since the parents may have many
                # children, and this will only choose the last one.
                # TODO: record all children and try all deltas to find best
                p1, p2, linknode, copyfrom = ancestors[node]

                if node in dontprocess:
                    if p1 != nullid:
                        dontprocess.add(p1)
                    if p2 != nullid:
                        dontprocess.add(p2)
                    continue

                if copyfrom:
                    dontprocess.add(p1)
                    p1 = nullid

                if p1 != nullid:
                    deltabases[p1] = node
                if p2 != nullid:
                    deltabases[p2] = node

                # Compute delta
                # TODO: reuse existing deltas if it matches our deltabase
                if deltabase != nullid:
                    deltabasetext = self.content.get(filename, deltabase)
                    original = self.content.get(filename, node)
                    delta = mdiff.textdiff(deltabasetext, original)
                else:
                    delta = self.content.get(filename, node)

                # TODO: don't use the delta if it's larger than the fulltext
                target.add(filename, node, deltabase, delta)

            count += 1
            ui.progress(_("repacking"), count, unit="files", total=len(files))

        ui.progress(_("repacking"), None)
        target.close()

    def _toposort(self, ancestors):
        def parentfunc(node):
            p1, p2, linknode, copyfrom = ancestors[node]
            parents = []
            if p1 != nullid:
                parents.append(p1)
            if p2 != nullid:
                parents.append(p2)
            return parents

        sortednodes = shallowutil.sortnodes(ancestors.keys(), parentfunc)
        return sortednodes
