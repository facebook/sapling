# changegroup.py - Mercurial changegroup manipulation functions
#
#  Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
from node import nullrev, hex
import mdiff, util, dagutil
import struct, os, bz2, zlib, tempfile

_BUNDLE10_DELTA_HEADER = "20s20s20s20s"

def readexactly(stream, n):
    '''read n bytes from stream.read and abort if less was available'''
    s = stream.read(n)
    if len(s) < n:
        raise util.Abort(_("stream ended unexpectedly"
                           " (got %d bytes, expected %d)")
                          % (len(s), n))
    return s

def getchunk(stream):
    """return the next chunk from stream as a string"""
    d = readexactly(stream, 4)
    l = struct.unpack(">l", d)[0]
    if l <= 4:
        if l:
            raise util.Abort(_("invalid chunk length %d") % l)
        return ""
    return readexactly(stream, l - 4)

def chunkheader(length):
    """return a changegroup chunk header (string)"""
    return struct.pack(">l", length + 4)

def closechunk():
    """return a changegroup chunk header (string) for a zero-length chunk"""
    return struct.pack(">l", 0)

class nocompress(object):
    def compress(self, x):
        return x
    def flush(self):
        return ""

bundletypes = {
    "": ("", nocompress), # only when using unbundle on ssh and old http servers
                          # since the unification ssh accepts a header but there
                          # is no capability signaling it.
    "HG10UN": ("HG10UN", nocompress),
    "HG10BZ": ("HG10", lambda: bz2.BZ2Compressor()),
    "HG10GZ": ("HG10GZ", lambda: zlib.compressobj()),
}

# hgweb uses this list to communicate its preferred type
bundlepriority = ['HG10GZ', 'HG10BZ', 'HG10UN']

def writebundle(cg, filename, bundletype):
    """Write a bundle file and return its filename.

    Existing files will not be overwritten.
    If no filename is specified, a temporary file is created.
    bz2 compression can be turned off.
    The bundle file will be deleted in case of errors.
    """

    fh = None
    cleanup = None
    try:
        if filename:
            fh = open(filename, "wb")
        else:
            fd, filename = tempfile.mkstemp(prefix="hg-bundle-", suffix=".hg")
            fh = os.fdopen(fd, "wb")
        cleanup = filename

        header, compressor = bundletypes[bundletype]
        fh.write(header)
        z = compressor()

        # parse the changegroup data, otherwise we will block
        # in case of sshrepo because we don't know the end of the stream

        # an empty chunkgroup is the end of the changegroup
        # a changegroup has at least 2 chunkgroups (changelog and manifest).
        # after that, an empty chunkgroup is the end of the changegroup
        empty = False
        count = 0
        while not empty or count <= 2:
            empty = True
            count += 1
            while True:
                chunk = getchunk(cg)
                if not chunk:
                    break
                empty = False
                fh.write(z.compress(chunkheader(len(chunk))))
                pos = 0
                while pos < len(chunk):
                    next = pos + 2**20
                    fh.write(z.compress(chunk[pos:next]))
                    pos = next
            fh.write(z.compress(closechunk()))
        fh.write(z.flush())
        cleanup = None
        return filename
    finally:
        if fh is not None:
            fh.close()
        if cleanup is not None:
            os.unlink(cleanup)

def decompressor(fh, alg):
    if alg == 'UN':
        return fh
    elif alg == 'GZ':
        def generator(f):
            zd = zlib.decompressobj()
            for chunk in util.filechunkiter(f):
                yield zd.decompress(chunk)
    elif alg == 'BZ':
        def generator(f):
            zd = bz2.BZ2Decompressor()
            zd.decompress("BZ")
            for chunk in util.filechunkiter(f, 4096):
                yield zd.decompress(chunk)
    else:
        raise util.Abort("unknown bundle compression '%s'" % alg)
    return util.chunkbuffer(generator(fh))

class unbundle10(object):
    deltaheader = _BUNDLE10_DELTA_HEADER
    deltaheadersize = struct.calcsize(deltaheader)
    def __init__(self, fh, alg):
        self._stream = decompressor(fh, alg)
        self._type = alg
        self.callback = None
    def compressed(self):
        return self._type != 'UN'
    def read(self, l):
        return self._stream.read(l)
    def seek(self, pos):
        return self._stream.seek(pos)
    def tell(self):
        return self._stream.tell()
    def close(self):
        return self._stream.close()

    def chunklength(self):
        d = readexactly(self._stream, 4)
        l = struct.unpack(">l", d)[0]
        if l <= 4:
            if l:
                raise util.Abort(_("invalid chunk length %d") % l)
            return 0
        if self.callback:
            self.callback()
        return l - 4

    def changelogheader(self):
        """v10 does not have a changelog header chunk"""
        return {}

    def manifestheader(self):
        """v10 does not have a manifest header chunk"""
        return {}

    def filelogheader(self):
        """return the header of the filelogs chunk, v10 only has the filename"""
        l = self.chunklength()
        if not l:
            return {}
        fname = readexactly(self._stream, l)
        return dict(filename=fname)

    def _deltaheader(self, headertuple, prevnode):
        node, p1, p2, cs = headertuple
        if prevnode is None:
            deltabase = p1
        else:
            deltabase = prevnode
        return node, p1, p2, deltabase, cs

    def deltachunk(self, prevnode):
        l = self.chunklength()
        if not l:
            return {}
        headerdata = readexactly(self._stream, self.deltaheadersize)
        header = struct.unpack(self.deltaheader, headerdata)
        delta = readexactly(self._stream, l - self.deltaheadersize)
        node, p1, p2, deltabase, cs = self._deltaheader(header, prevnode)
        return dict(node=node, p1=p1, p2=p2, cs=cs,
                    deltabase=deltabase, delta=delta)

class headerlessfixup(object):
    def __init__(self, fh, h):
        self._h = h
        self._fh = fh
    def read(self, n):
        if self._h:
            d, self._h = self._h[:n], self._h[n:]
            if len(d) < n:
                d += readexactly(self._fh, n - len(d))
            return d
        return readexactly(self._fh, n)

def readbundle(fh, fname):
    header = readexactly(fh, 6)

    if not fname:
        fname = "stream"
        if not header.startswith('HG') and header.startswith('\0'):
            fh = headerlessfixup(fh, header)
            header = "HG10UN"

    magic, version, alg = header[0:2], header[2:4], header[4:6]

    if magic != 'HG':
        raise util.Abort(_('%s: not a Mercurial bundle') % fname)
    if version != '10':
        raise util.Abort(_('%s: unknown bundle version %s') % (fname, version))
    return unbundle10(fh, alg)

class bundle10(object):
    deltaheader = _BUNDLE10_DELTA_HEADER
    def __init__(self, repo, bundlecaps=None):
        """Given a source repo, construct a bundler.

        bundlecaps is optional and can be used to specify the set of
        capabilities which can be used to build the bundle.
        """
        # Set of capabilities we can use to build the bundle.
        if bundlecaps is None:
            bundlecaps = set()
        self._bundlecaps = bundlecaps
        self._changelog = repo.changelog
        self._manifest = repo.manifest
        reorder = repo.ui.config('bundle', 'reorder', 'auto')
        if reorder == 'auto':
            reorder = None
        else:
            reorder = util.parsebool(reorder)
        self._repo = repo
        self._reorder = reorder
        self.count = [0, 0]
    def start(self, lookup):
        self._lookup = lookup
    def close(self):
        return closechunk()

    def fileheader(self, fname):
        return chunkheader(len(fname)) + fname

    def group(self, nodelist, revlog, reorder=None):
        """Calculate a delta group, yielding a sequence of changegroup chunks
        (strings).

        Given a list of changeset revs, return a set of deltas and
        metadata corresponding to nodes. The first delta is
        first parent(nodelist[0]) -> nodelist[0], the receiver is
        guaranteed to have this parent as it has all history before
        these changesets. In the case firstparent is nullrev the
        changegroup starts with a full revision.
        """

        # if we don't have any revisions touched by these changesets, bail
        if len(nodelist) == 0:
            yield self.close()
            return

        # for generaldelta revlogs, we linearize the revs; this will both be
        # much quicker and generate a much smaller bundle
        if (revlog._generaldelta and reorder is not False) or reorder:
            dag = dagutil.revlogdag(revlog)
            revs = set(revlog.rev(n) for n in nodelist)
            revs = dag.linearize(revs)
        else:
            revs = sorted([revlog.rev(n) for n in nodelist])

        # add the parent of the first rev
        p = revlog.parentrevs(revs[0])[0]
        revs.insert(0, p)

        # build deltas
        for r in xrange(len(revs) - 1):
            prev, curr = revs[r], revs[r + 1]
            for c in self.revchunk(revlog, curr, prev):
                yield c

        yield self.close()

    def generate(self, commonrevs, clnodes, fastpathlinkrev, source):
        '''yield a sequence of changegroup chunks (strings)'''
        repo = self._repo
        cl = self._changelog
        mf = self._manifest
        reorder = self._reorder
        progress = repo.ui.progress
        count = self.count
        _bundling = _('bundling')
        _changesets = _('changesets')
        _manifests = _('manifests')
        _files = _('files')

        mfs = {} # needed manifests
        fnodes = {} # needed file nodes
        changedfiles = set()
        fstate = ['', {}]

        # filter any nodes that claim to be part of the known set
        def prune(revlog, missing):
            rr, rl = revlog.rev, revlog.linkrev
            return [n for n in missing
                    if rl(rr(n)) not in commonrevs]

        def lookup(revlog, x):
            if revlog == cl:
                c = cl.read(x)
                changedfiles.update(c[3])
                mfs.setdefault(c[0], x)
                count[0] += 1
                progress(_bundling, count[0],
                         unit=_changesets, total=count[1])
                return x
            elif revlog == mf:
                clnode = mfs[x]
                if not fastpathlinkrev:
                    mdata = mf.readfast(x)
                    for f, n in mdata.iteritems():
                        if f in changedfiles:
                            fnodes[f].setdefault(n, clnode)
                count[0] += 1
                progress(_bundling, count[0],
                         unit=_manifests, total=count[1])
                return clnode
            else:
                progress(_bundling, count[0], item=fstate[0],
                         unit=_files, total=count[1])
                return fstate[1][x]

        self.start(lookup)

        def getmfnodes():
            for f in changedfiles:
                fnodes[f] = {}
            count[:] = [0, len(mfs)]
            return prune(mf, mfs)
        def getfiles():
            mfs.clear()
            return changedfiles
        def getfilenodes(fname, filerevlog):
            if fastpathlinkrev:
                ln, llr = filerevlog.node, filerevlog.linkrev
                def genfilenodes():
                    for r in filerevlog:
                        linkrev = llr(r)
                        if linkrev not in commonrevs:
                            yield filerevlog.node(r), cl.node(linkrev)
                fnodes[fname] = dict(genfilenodes())
            fstate[0] = fname
            fstate[1] = fnodes.pop(fname, {})
            return prune(filerevlog, fstate[1])


        count[:] = [0, len(clnodes)]
        for chunk in self.group(clnodes, cl, reorder=reorder):
            yield chunk
        progress(_bundling, None)

        for chunk in self.group(getmfnodes(), mf, reorder=reorder):
            yield chunk
        progress(_bundling, None)

        changedfiles = getfiles()
        count[:] = [0, len(changedfiles)]
        for fname in sorted(changedfiles):
            filerevlog = repo.file(fname)
            if not len(filerevlog):
                raise util.Abort(_("empty or missing revlog for %s")
                                 % fname)
            nodelist = getfilenodes(fname, filerevlog)
            if nodelist:
                count[0] += 1
                yield self.fileheader(fname)
                for chunk in self.group(nodelist, filerevlog, reorder):
                    yield chunk
        yield self.close()
        progress(_bundling, None)

        if clnodes:
            repo.hook('outgoing', node=hex(clnodes[0]), source=source)

    def revchunk(self, revlog, rev, prev):
        node = revlog.node(rev)
        p1, p2 = revlog.parentrevs(rev)
        base = prev

        prefix = ''
        if base == nullrev:
            delta = revlog.revision(node)
            prefix = mdiff.trivialdiffheader(len(delta))
        else:
            delta = revlog.revdiff(base, rev)
        linknode = self._lookup(revlog, node)
        p1n, p2n = revlog.parents(node)
        basenode = revlog.node(base)
        meta = self.builddeltaheader(node, p1n, p2n, basenode, linknode)
        meta += prefix
        l = len(meta) + len(delta)
        yield chunkheader(l)
        yield meta
        yield delta
    def builddeltaheader(self, node, p1n, p2n, basenode, linknode):
        # do nothing with basenode, it is implicitly the previous one in HG10
        return struct.pack(self.deltaheader, node, p1n, p2n, linknode)
