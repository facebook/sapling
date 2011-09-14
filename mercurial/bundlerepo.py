# bundlerepo.py - repository class for viewing uncompressed bundles
#
# Copyright 2006, 2007 Benoit Boissinot <bboissin@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Repository class for viewing uncompressed bundles.

This provides a read-only repository interface to bundles as if they
were part of the actual repository.
"""

from node import nullid
from i18n import _
import os, tempfile, shutil
import changegroup, util, mdiff, discovery
import localrepo, changelog, manifest, filelog, revlog, error

class bundlerevlog(revlog.revlog):
    def __init__(self, opener, indexfile, bundle, linkmapper):
        # How it works:
        # to retrieve a revision, we need to know the offset of
        # the revision in the bundle (an unbundle object).
        #
        # We store this offset in the index (start), to differentiate a
        # rev in the bundle and from a rev in the revlog, we check
        # len(index[r]). If the tuple is bigger than 7, it is a bundle
        # (it is bigger since we store the node to which the delta is)
        #
        revlog.revlog.__init__(self, opener, indexfile)
        self.bundle = bundle
        self.basemap = {}
        n = len(self)
        chain = None
        while True:
            chunkdata = bundle.deltachunk(chain)
            if not chunkdata:
                break
            node = chunkdata['node']
            p1 = chunkdata['p1']
            p2 = chunkdata['p2']
            cs = chunkdata['cs']
            deltabase = chunkdata['deltabase']
            delta = chunkdata['delta']

            size = len(delta)
            start = bundle.tell() - size

            link = linkmapper(cs)
            if node in self.nodemap:
                # this can happen if two branches make the same change
                chain = node
                continue

            for p in (p1, p2):
                if not p in self.nodemap:
                    raise error.LookupError(p, self.indexfile,
                                            _("unknown parent"))
            # start, size, full unc. size, base (unused), link, p1, p2, node
            e = (revlog.offset_type(start, 0), size, -1, -1, link,
                 self.rev(p1), self.rev(p2), node)
            self.basemap[n] = deltabase
            self.index.insert(-1, e)
            self.nodemap[node] = n
            chain = node
            n += 1

    def inbundle(self, rev):
        """is rev from the bundle"""
        if rev < 0:
            return False
        return rev in self.basemap
    def bundlebase(self, rev):
        return self.basemap[rev]
    def _chunk(self, rev):
        # Warning: in case of bundle, the diff is against bundlebase,
        # not against rev - 1
        # XXX: could use some caching
        if not self.inbundle(rev):
            return revlog.revlog._chunk(self, rev)
        self.bundle.seek(self.start(rev))
        return self.bundle.read(self.length(rev))

    def revdiff(self, rev1, rev2):
        """return or calculate a delta between two revisions"""
        if self.inbundle(rev1) and self.inbundle(rev2):
            # hot path for bundle
            revb = self.rev(self.bundlebase(rev2))
            if revb == rev1:
                return self._chunk(rev2)
        elif not self.inbundle(rev1) and not self.inbundle(rev2):
            return revlog.revlog.revdiff(self, rev1, rev2)

        return mdiff.textdiff(self.revision(self.node(rev1)),
                         self.revision(self.node(rev2)))

    def revision(self, node):
        """return an uncompressed revision of a given"""
        if node == nullid:
            return ""

        text = None
        chain = []
        iter_node = node
        rev = self.rev(iter_node)
        # reconstruct the revision if it is from a changegroup
        while self.inbundle(rev):
            if self._cache and self._cache[0] == iter_node:
                text = self._cache[2]
                break
            chain.append(rev)
            iter_node = self.bundlebase(rev)
            rev = self.rev(iter_node)
        if text is None:
            text = revlog.revlog.revision(self, iter_node)

        while chain:
            delta = self._chunk(chain.pop())
            text = mdiff.patches(text, [delta])

        p1, p2 = self.parents(node)
        if node != revlog.hash(text, p1, p2):
            raise error.RevlogError(_("integrity check failed on %s:%d")
                                     % (self.datafile, self.rev(node)))

        self._cache = (node, self.rev(node), text)
        return text

    def addrevision(self, text, transaction, link, p1=None, p2=None, d=None):
        raise NotImplementedError
    def addgroup(self, revs, linkmapper, transaction):
        raise NotImplementedError
    def strip(self, rev, minlink):
        raise NotImplementedError
    def checksize(self):
        raise NotImplementedError

class bundlechangelog(bundlerevlog, changelog.changelog):
    def __init__(self, opener, bundle):
        changelog.changelog.__init__(self, opener)
        linkmapper = lambda x: x
        bundlerevlog.__init__(self, opener, self.indexfile, bundle,
                              linkmapper)

class bundlemanifest(bundlerevlog, manifest.manifest):
    def __init__(self, opener, bundle, linkmapper):
        manifest.manifest.__init__(self, opener)
        bundlerevlog.__init__(self, opener, self.indexfile, bundle,
                              linkmapper)

class bundlefilelog(bundlerevlog, filelog.filelog):
    def __init__(self, opener, path, bundle, linkmapper, repo):
        filelog.filelog.__init__(self, opener, path)
        bundlerevlog.__init__(self, opener, self.indexfile, bundle,
                              linkmapper)
        self._repo = repo

    def _file(self, f):
        self._repo.file(f)

class bundlerepository(localrepo.localrepository):
    def __init__(self, ui, path, bundlename):
        self._tempparent = None
        try:
            localrepo.localrepository.__init__(self, ui, path)
        except error.RepoError:
            self._tempparent = tempfile.mkdtemp()
            localrepo.instance(ui, self._tempparent, 1)
            localrepo.localrepository.__init__(self, ui, self._tempparent)

        if path:
            self._url = 'bundle:' + util.expandpath(path) + '+' + bundlename
        else:
            self._url = 'bundle:' + bundlename

        self.tempfile = None
        f = util.posixfile(bundlename, "rb")
        self.bundle = changegroup.readbundle(f, bundlename)
        if self.bundle.compressed():
            fdtemp, temp = tempfile.mkstemp(prefix="hg-bundle-",
                                            suffix=".hg10un", dir=self.path)
            self.tempfile = temp
            fptemp = os.fdopen(fdtemp, 'wb')

            try:
                fptemp.write("HG10UN")
                while True:
                    chunk = self.bundle.read(2**18)
                    if not chunk:
                        break
                    fptemp.write(chunk)
            finally:
                fptemp.close()

            f = util.posixfile(self.tempfile, "rb")
            self.bundle = changegroup.readbundle(f, bundlename)

        # dict with the mapping 'filename' -> position in the bundle
        self.bundlefilespos = {}

    @util.propertycache
    def changelog(self):
        # consume the header if it exists
        self.bundle.changelogheader()
        c = bundlechangelog(self.sopener, self.bundle)
        self.manstart = self.bundle.tell()
        return c

    @util.propertycache
    def manifest(self):
        self.bundle.seek(self.manstart)
        # consume the header if it exists
        self.bundle.manifestheader()
        m = bundlemanifest(self.sopener, self.bundle, self.changelog.rev)
        self.filestart = self.bundle.tell()
        return m

    @util.propertycache
    def manstart(self):
        self.changelog
        return self.manstart

    @util.propertycache
    def filestart(self):
        self.manifest
        return self.filestart

    def url(self):
        return self._url

    def file(self, f):
        if not self.bundlefilespos:
            self.bundle.seek(self.filestart)
            while True:
                chunkdata = self.bundle.filelogheader()
                if not chunkdata:
                    break
                fname = chunkdata['filename']
                self.bundlefilespos[fname] = self.bundle.tell()
                while True:
                    c = self.bundle.deltachunk(None)
                    if not c:
                        break

        if f[0] == '/':
            f = f[1:]
        if f in self.bundlefilespos:
            self.bundle.seek(self.bundlefilespos[f])
            return bundlefilelog(self.sopener, f, self.bundle,
                                 self.changelog.rev, self)
        else:
            return filelog.filelog(self.sopener, f)

    def close(self):
        """Close assigned bundle file immediately."""
        self.bundle.close()
        if self.tempfile is not None:
            os.unlink(self.tempfile)
        if self._tempparent:
            shutil.rmtree(self._tempparent, True)

    def cancopy(self):
        return False

    def getcwd(self):
        return os.getcwd() # always outside the repo

def instance(ui, path, create):
    if create:
        raise util.Abort(_('cannot create new bundle repository'))
    parentpath = ui.config("bundle", "mainreporoot", "")
    if parentpath:
        # Try to make the full path relative so we get a nice, short URL.
        # In particular, we don't want temp dir names in test outputs.
        cwd = os.getcwd()
        if parentpath == cwd:
            parentpath = ''
        else:
            cwd = os.path.join(cwd,'')
            if parentpath.startswith(cwd):
                parentpath = parentpath[len(cwd):]
    u = util.url(path)
    path = u.localpath()
    if u.scheme == 'bundle':
        s = path.split("+", 1)
        if len(s) == 1:
            repopath, bundlename = parentpath, s[0]
        else:
            repopath, bundlename = s
    else:
        repopath, bundlename = parentpath, path
    return bundlerepository(ui, repopath, bundlename)

def getremotechanges(ui, repo, other, onlyheads=None, bundlename=None,
                     force=False):
    '''obtains a bundle of changes incoming from other

    "onlyheads" restricts the returned changes to those reachable from the
      specified heads.
    "bundlename", if given, stores the bundle to this file path permanently;
      otherwise it's stored to a temp file and gets deleted again when you call
      the returned "cleanupfn".
    "force" indicates whether to proceed on unrelated repos.

    Returns a tuple (local, csets, cleanupfn):

    "local" is a local repo from which to obtain the actual incoming changesets; it
      is a bundlerepo for the obtained bundle when the original "other" is remote.
    "csets" lists the incoming changeset node ids.
    "cleanupfn" must be called without arguments when you're done processing the
      changes; it closes both the original "other" and the one returned here.
    '''
    tmp = discovery.findcommonincoming(repo, other, heads=onlyheads, force=force)
    common, incoming, rheads = tmp
    if not incoming:
        try:
            if bundlename:
                os.unlink(bundlename)
        except OSError:
            pass
        return other, [], other.close

    bundle = None
    bundlerepo = None
    localrepo = other
    if bundlename or not other.local():
        # create a bundle (uncompressed if other repo is not local)

        if other.capable('getbundle'):
            cg = other.getbundle('incoming', common=common, heads=rheads)
        elif onlyheads is None and not other.capable('changegroupsubset'):
            # compat with older servers when pulling all remote heads
            cg = other.changegroup(incoming, "incoming")
            rheads = None
        else:
            cg = other.changegroupsubset(incoming, rheads, 'incoming')
        bundletype = other.local() and "HG10BZ" or "HG10UN"
        fname = bundle = changegroup.writebundle(cg, bundlename, bundletype)
        # keep written bundle?
        if bundlename:
            bundle = None
        if not other.local():
            # use the created uncompressed bundlerepo
            localrepo = bundlerepo = bundlerepository(ui, repo.root, fname)
            # this repo contains local and other now, so filter out local again
            common = repo.heads()

    csets = localrepo.changelog.findmissing(common, rheads)

    def cleanup():
        if bundlerepo:
            bundlerepo.close()
        if bundle:
            os.unlink(bundle)
        other.close()

    return (localrepo, csets, cleanup)

