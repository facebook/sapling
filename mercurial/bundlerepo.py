"""
bundlerepo.py - repository class for viewing uncompressed bundles

This provides a read-only repository interface to bundles as if
they were part of the actual repository.

Copyright 2006 Benoit Boissinot <benoit.boissinot@ens-lyon.org>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

from node import *
from i18n import gettext as _
from demandload import demandload
demandload(globals(), "util os struct")

import localrepo, changelog, manifest, filelog, revlog

def getchunk(source):
    """get a chunk from a group"""
    d = source.read(4)
    if not d:
        return ""
    l = struct.unpack(">l", d)[0]
    if l <= 4:
        return ""
    d = source.read(l - 4)
    if len(d) < l - 4:
        raise util.Abort(_("premature EOF reading chunk"
                           " (got %d bytes, expected %d)")
                          % (len(d), l - 4))
    return d

class bundlerevlog(revlog.revlog):
    def __init__(self, opener, indexfile, datafile, bundlefile,
                 linkmapper=None):
        # How it works:
        # to retrieve a revision, we need to know the offset of
        # the revision in the bundlefile (an opened file).
        #
        # We store this offset in the index (start), to differentiate a
        # rev in the bundle and from a rev in the revlog, we check
        # len(index[r]). If the tuple is bigger than 7, it is a bundle
        # (it is bigger since we store the node to which the delta is)
        #
        revlog.revlog.__init__(self, opener, indexfile, datafile)
        self.bundlefile = bundlefile
        def genchunk():
            while 1:
                pos = bundlefile.tell()
                chunk = getchunk(bundlefile)
                if not chunk:
                    break
                yield chunk, pos + 4 # XXX struct.calcsize(">l") == 4
        n = self.count()
        prev = None
        for chunk, start in genchunk():
            size = len(chunk)
            if size < 80:
                raise util.Abort("invalid changegroup")
            start += 80
            size -= 80
            node, p1, p2, cs = struct.unpack("20s20s20s20s", chunk[:80])
            if node in self.nodemap:
                prev = node
                continue
            for p in (p1, p2):
                if not p in self.nodemap:
                    raise RevlogError(_("unknown parent %s") % short(p1))
            if linkmapper is None:
                link = n
            else:
                link = linkmapper(cs)

            if not prev:
                prev = p1
            # start, size, base is not used, link, p1, p2, delta ref
            e = (start, size, None, link, p1, p2, node, prev)
            self.index.append(e)
            self.nodemap[node] = n
            prev = node
            n += 1

    def bundle(self, rev):
        """is rev from the bundle"""
        if rev < 0:
            return False
        return len(self.index[rev]) > 7
    def bundlebase(self, rev): return self.index[rev][7]
    def chunk(self, rev):
        # Warning: in case of bundle, the diff is against bundlebase,
        # not against rev - 1
        # XXX: could use some caching
        if not self.bundle(rev):
            return revlog.revlog.chunk(self, rev)
        self.bundlefile.seek(self.start(rev))
        return self.bundlefile.read(self.length(rev))

    def revdiff(self, rev1, rev2):
        """return or calculate a delta between two revisions"""
        if self.bundle(rev1) and self.bundle(rev2):
            # hot path for bundle
            revb = self.rev(self.bundlebase(rev2))
            if revb == rev1:
                return self.chunk(rev2)
        elif not self.bundle(rev1) and not self.bundle(rev2):
            return revlog.revlog.chunk(self, rev1, rev2)

        return self.diff(self.revision(self.node(rev1)),
                         self.revision(self.node(rev2)))

    def revision(self, node):
        """return an uncompressed revision of a given"""
        if node == nullid: return ""

        text = None
        chain = []
        iter_node = node
        rev = self.rev(iter_node)
        # reconstruct the revision if it is from a changegroup
        while self.bundle(rev):
            if self.cache and self.cache[0] == iter_node:
                text = self.cache[2]
                break
            chain.append(rev)
            iter_node = self.bundlebase(rev)
            rev = self.rev(iter_node)
        if text is None:
            text = revlog.revlog.revision(self, iter_node)

        while chain:
            delta = self.chunk(chain.pop())
            text = self.patches(text, [delta])

        p1, p2 = self.parents(node)
        if node != revlog.hash(text, p1, p2):
            raise RevlogError(_("integrity check failed on %s:%d")
                          % (self.datafile, self.rev(node)))

        self.cache = (node, rev, text)
        return text

    def addrevision(self, text, transaction, link, p1=None, p2=None, d=None):
        raise NotImplementedError
    def addgroup(self, revs, linkmapper, transaction, unique=0):
        raise NotImplementedError
    def strip(self, rev, minlink):
        raise NotImplementedError
    def checksize(self):
        raise NotImplementedError

class bundlechangelog(bundlerevlog, changelog.changelog):
    def __init__(self, opener, bundlefile):
        changelog.changelog.__init__(self, opener)
        bundlerevlog.__init__(self, opener, "00changelog.i", "00changelog.d",
                              bundlefile)

class bundlemanifest(bundlerevlog, manifest.manifest):
    def __init__(self, opener, bundlefile, linkmapper):
        manifest.manifest.__init__(self, opener)
        bundlerevlog.__init__(self, opener, self.indexfile, self.datafile,
                              bundlefile, linkmapper)

class bundlefilelog(bundlerevlog, filelog.filelog):
    def __init__(self, opener, path, bundlefile, linkmapper):
        filelog.filelog.__init__(self, opener, path)
        bundlerevlog.__init__(self, opener, self.indexfile, self.datafile,
                              bundlefile, linkmapper)

class bundlerepository(localrepo.localrepository):
    def __init__(self, ui, path, bundlename):
        localrepo.localrepository.__init__(self, ui, path)
        f = open(bundlename, "rb")
        s = os.fstat(f.fileno())
        self.bundlefile = f
        header = self.bundlefile.read(4)
        if header == "HG10":
            raise util.Abort(_("%s: compressed bundle not supported")
                             % bundlename)
        elif header != "HG11":
            raise util.Abort(_("%s: not a Mercurial bundle file") % bundlename)
        self.changelog = bundlechangelog(self.opener, self.bundlefile)
        self.manifest = bundlemanifest(self.opener, self.bundlefile,
                                       self.changelog.rev)
        # dict with the mapping 'filename' -> position in the bundle
        self.bundlefilespos = {}
        while 1:
                f = getchunk(self.bundlefile)
                if not f:
                    break
                self.bundlefilespos[f] = self.bundlefile.tell()
                while getchunk(self.bundlefile):
                    pass

    def dev(self):
        return -1

    def file(self, f):
        if f[0] == '/':
            f = f[1:]
        if f in self.bundlefilespos:
            self.bundlefile.seek(self.bundlefilespos[f])
            return bundlefilelog(self.opener, f, self.bundlefile,
                                 self.changelog.rev)
        else:
            return filelog.filelog(self.opener, f)

