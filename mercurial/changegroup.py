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
        self._progress = repo.ui.progress
    def close(self):
        return closechunk()

    def fileheader(self, fname):
        return chunkheader(len(fname)) + fname

    def group(self, nodelist, revlog, lookup, units=None, reorder=None):
        """Calculate a delta group, yielding a sequence of changegroup chunks
        (strings).

        Given a list of changeset revs, return a set of deltas and
        metadata corresponding to nodes. The first delta is
        first parent(nodelist[0]) -> nodelist[0], the receiver is
        guaranteed to have this parent as it has all history before
        these changesets. In the case firstparent is nullrev the
        changegroup starts with a full revision.

        If units is not None, progress detail will be generated, units specifies
        the type of revlog that is touched (changelog, manifest, etc.).
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
        total = len(revs) - 1
        msgbundling = _('bundling')
        for r in xrange(len(revs) - 1):
            if units is not None:
                self._progress(msgbundling, r + 1, unit=units, total=total)
            prev, curr = revs[r], revs[r + 1]
            linknode = lookup(revlog.node(curr))
            for c in self.revchunk(revlog, curr, prev, linknode):
                yield c

        yield self.close()

    # filter any nodes that claim to be part of the known set
    def prune(self, revlog, missing, commonrevs, source):
        rr, rl = revlog.rev, revlog.linkrev
        return [n for n in missing if rl(rr(n)) not in commonrevs]

    def generate(self, commonrevs, clnodes, fastpathlinkrev, source):
        '''yield a sequence of changegroup chunks (strings)'''
        repo = self._repo
        cl = self._changelog
        mf = self._manifest
        reorder = self._reorder
        progress = self._progress

        # for progress output
        msgbundling = _('bundling')

        mfs = {} # needed manifests
        fnodes = {} # needed file nodes
        changedfiles = set()

        # Callback for the changelog, used to collect changed files and manifest
        # nodes.
        # Returns the linkrev node (identity in the changelog case).
        def lookupcl(x):
            c = cl.read(x)
            changedfiles.update(c[3])
            # record the first changeset introducing this manifest version
            mfs.setdefault(c[0], x)
            return x

        # Callback for the manifest, used to collect linkrevs for filelog
        # revisions.
        # Returns the linkrev node (collected in lookupcl).
        def lookupmf(x):
            clnode = mfs[x]
            if not fastpathlinkrev:
                mdata = mf.readfast(x)
                for f, n in mdata.iteritems():
                    if f in changedfiles:
                        # record the first changeset introducing this filelog
                        # version
                        fnodes[f].setdefault(n, clnode)
            return clnode

        for chunk in self.group(clnodes, cl, lookupcl, units=_('changesets'),
                                reorder=reorder):
            yield chunk
        progress(msgbundling, None)

        for f in changedfiles:
            fnodes[f] = {}
        mfnodes = self.prune(mf, mfs, commonrevs, source)
        for chunk in self.group(mfnodes, mf, lookupmf, units=_('manifests'),
                                reorder=reorder):
            yield chunk
        progress(msgbundling, None)

        mfs.clear()
        needed = set(cl.rev(x) for x in clnodes)

        def linknodes(filerevlog, fname):
            if fastpathlinkrev:
                ln, llr = filerevlog.node, filerevlog.linkrev
                def genfilenodes():
                    for r in filerevlog:
                        linkrev = llr(r)
                        if linkrev in needed:
                            yield filerevlog.node(r), cl.node(linkrev)
                fnodes[fname] = dict(genfilenodes())
            return fnodes.get(fname, {})

        for chunk in self.generatefiles(changedfiles, linknodes, commonrevs,
                                        source):
            yield chunk

        yield self.close()
        progress(msgbundling, None)

        if clnodes:
            repo.hook('outgoing', node=hex(clnodes[0]), source=source)

    def generatefiles(self, changedfiles, linknodes, commonrevs, source):
        repo = self._repo
        progress = self._progress
        reorder = self._reorder
        msgbundling = _('bundling')

        total = len(changedfiles)
        # for progress output
        msgfiles = _('files')
        for i, fname in enumerate(sorted(changedfiles)):
            filerevlog = repo.file(fname)
            if not filerevlog:
                raise util.Abort(_("empty or missing revlog for %s") % fname)

            linkrevnodes = linknodes(filerevlog, fname)
            # Lookup for filenodes, we collected the linkrev nodes above in the
            # fastpath case and with lookupmf in the slowpath case.
            def lookupfilelog(x):
                return linkrevnodes[x]

            filenodes = self.prune(filerevlog, linkrevnodes, commonrevs, source)
            if filenodes:
                progress(msgbundling, i + 1, item=fname, unit=msgfiles,
                         total=total)
                yield self.fileheader(fname)
                for chunk in self.group(filenodes, filerevlog, lookupfilelog,
                                        reorder=reorder):
                    yield chunk

    def revchunk(self, revlog, rev, prev, linknode):
        node = revlog.node(rev)
        p1, p2 = revlog.parentrevs(rev)
        base = prev

        prefix = ''
        if base == nullrev:
            delta = revlog.revision(node)
            prefix = mdiff.trivialdiffheader(len(delta))
        else:
            delta = revlog.revdiff(base, rev)
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
