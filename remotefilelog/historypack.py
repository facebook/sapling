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
FANOUTSIZE = FANOUTCOUNT * 4

INDEXSUFFIX = '.histidx'
PACKSUFFIX = '.histpack'

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
        - pack content size (The on-disk length of this file section's pack data)

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
    def __init__(self, path):
        self.path = path
        self.entries = []
        self.packfp, self.historypackpath = tempfile.mkstemp(suffix=PACKSUFFIX + '-tmp', dir=path)
        self.idxfp, self.historyidxpath = tempfile.mkstemp(suffix=INDEXSUFFIX + '-tmp', dir=path)
        self.packfp = os.fdopen(self.packfp, 'w+')
        self.idxfp = os.fdopen(self.idxfp, 'w+')
        self.sha = util.sha1()

        # Write header
        # TODO: make it extensible
        version = struct.pack('!B', 0) # unsigned 1 byte int
        self.writeraw(version)

        self.pastfiles = {}
        self.currentfile = None
        self.currentfilestart = 0

    def add(self, filename, node, p1, p2, linknode):
        if filename != self.currentfile:
            if filename in self.pastfiles:
                raise Exception("cannot add file node after another file's "
                                "nodes have been added")
            if self.currentfile:
                self.pastfiles[self.currentfile] = (
                    self.currentfilestart,
                    self.packfp.tell() - self.currentfilestart
                )
            self.currentfile = filename
            self.currentfilestart = self.packfp.tell()
            # TODO: prefix the filename section with the number of entries
            self.writeraw("%s%s" % (
                struct.pack('!H', len(filename)),
                filename,
            ))

        rawdata = struct.pack('!20s20s20s20s', node, p1, p2, linknode)
        self.writeraw(rawdata)

    def writeraw(self, data):
        self.packfp.write(data)
        self.sha.update(data)

    def close(self):
        if self.currentfile:
            self.pastfiles[self.currentfile] = (
                self.currentfilestart,
                self.packfp.tell() - self.currentfilestart
            )

        sha = self.sha.hexdigest()
        self.packfp.close()
        self.writeindex()

        os.rename(self.historypackpath, os.path.join(self.path, sha +
                                                     PACKSUFFIX))
        os.rename(self.historyidxpath, os.path.join(self.path, sha +
                                                    INDEXSUFFIX))

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
            rawfanouttable += struct.pack('!I', offset)

        # TODO: add version number to the index
        self.idxfp.write(rawfanouttable)
        self.idxfp.write(rawindex)
        self.idxfp.close()

class historygc(object):
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

            # Order the nodes children first
            orderednodes = reversed(self._toposort(ancestors))

            # Write to the pack
            dontprocess = set()
            for node in orderednodes:
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

                target.add(filename, node, p1, p2, linknode)

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
