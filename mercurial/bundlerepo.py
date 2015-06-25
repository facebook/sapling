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
import changegroup, util, mdiff, discovery, cmdutil, scmutil, exchange
import localrepo, changelog, manifest, filelog, revlog, error, phases, bundle2
import pathutil

class bundlerevlog(revlog.revlog):
    def __init__(self, opener, indexfile, bundle, linkmapper):
        # How it works:
        # To retrieve a revision, we need to know the offset of the revision in
        # the bundle (an unbundle object). We store this offset in the index
        # (start). The base of the delta is stored in the base field.
        #
        # To differentiate a rev in the bundle from a rev in the revlog, we
        # check revision against repotiprev.
        opener = scmutil.readonlyvfs(opener)
        revlog.revlog.__init__(self, opener, indexfile)
        self.bundle = bundle
        n = len(self)
        self.repotiprev = n - 1
        chain = None
        self.bundlerevs = set() # used by 'bundle()' revset expression
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
                self.bundlerevs.add(self.nodemap[node])
                continue

            for p in (p1, p2):
                if p not in self.nodemap:
                    raise error.LookupError(p, self.indexfile,
                                            _("unknown parent"))

            if deltabase not in self.nodemap:
                raise LookupError(deltabase, self.indexfile,
                                  _('unknown delta base'))

            baserev = self.rev(deltabase)
            # start, size, full unc. size, base (unused), link, p1, p2, node
            e = (revlog.offset_type(start, 0), size, -1, baserev, link,
                 self.rev(p1), self.rev(p2), node)
            self.index.insert(-1, e)
            self.nodemap[node] = n
            self.bundlerevs.add(n)
            chain = node
            n += 1

    def _chunk(self, rev):
        # Warning: in case of bundle, the diff is against what we stored as
        # delta base, not against rev - 1
        # XXX: could use some caching
        if rev <= self.repotiprev:
            return revlog.revlog._chunk(self, rev)
        self.bundle.seek(self.start(rev))
        return self.bundle.read(self.length(rev))

    def revdiff(self, rev1, rev2):
        """return or calculate a delta between two revisions"""
        if rev1 > self.repotiprev and rev2 > self.repotiprev:
            # hot path for bundle
            revb = self.index[rev2][3]
            if revb == rev1:
                return self._chunk(rev2)
        elif rev1 <= self.repotiprev and rev2 <= self.repotiprev:
            return revlog.revlog.revdiff(self, rev1, rev2)

        return mdiff.textdiff(self.revision(self.node(rev1)),
                              self.revision(self.node(rev2)))

    def revision(self, nodeorrev):
        """return an uncompressed revision of a given node or revision
        number.
        """
        if isinstance(nodeorrev, int):
            rev = nodeorrev
            node = self.node(rev)
        else:
            node = nodeorrev
            rev = self.rev(node)

        if node == nullid:
            return ""

        text = None
        chain = []
        iterrev = rev
        # reconstruct the revision if it is from a changegroup
        while iterrev > self.repotiprev:
            if self._cache and self._cache[1] == iterrev:
                text = self._cache[2]
                break
            chain.append(iterrev)
            iterrev = self.index[iterrev][3]
        if text is None:
            text = self.baserevision(iterrev)

        while chain:
            delta = self._chunk(chain.pop())
            text = mdiff.patches(text, [delta])

        self._checkhash(text, node, rev)
        self._cache = (node, rev, text)
        return text

    def baserevision(self, nodeorrev):
        # Revlog subclasses may override 'revision' method to modify format of
        # content retrieved from revlog. To use bundlerevlog with such class one
        # needs to override 'baserevision' and make more specific call here.
        return revlog.revlog.revision(self, nodeorrev)

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

    def baserevision(self, nodeorrev):
        # Although changelog doesn't override 'revision' method, some extensions
        # may replace this class with another that does. Same story with
        # manifest and filelog classes.

        # This bypasses filtering on changelog.node() and rev() because we need
        # revision text of the bundle base even if it is hidden.
        oldfilter = self.filteredrevs
        try:
            self.filteredrevs = ()
            return changelog.changelog.revision(self, nodeorrev)
        finally:
            self.filteredrevs = oldfilter

class bundlemanifest(bundlerevlog, manifest.manifest):
    def __init__(self, opener, bundle, linkmapper):
        manifest.manifest.__init__(self, opener)
        bundlerevlog.__init__(self, opener, self.indexfile, bundle,
                              linkmapper)

    def baserevision(self, nodeorrev):
        return manifest.manifest.revision(self, nodeorrev)

class bundlefilelog(bundlerevlog, filelog.filelog):
    def __init__(self, opener, path, bundle, linkmapper):
        filelog.filelog.__init__(self, opener, path)
        bundlerevlog.__init__(self, opener, self.indexfile, bundle,
                              linkmapper)

    def baserevision(self, nodeorrev):
        return filelog.filelog.revision(self, nodeorrev)

class bundlepeer(localrepo.localpeer):
    def canpush(self):
        return False

class bundlephasecache(phases.phasecache):
    def __init__(self, *args, **kwargs):
        super(bundlephasecache, self).__init__(*args, **kwargs)
        if util.safehasattr(self, 'opener'):
            self.opener = scmutil.readonlyvfs(self.opener)

    def write(self):
        raise NotImplementedError

    def _write(self, fp):
        raise NotImplementedError

    def _updateroots(self, phase, newroots, tr):
        self.phaseroots[phase] = newroots
        self.invalidate()
        self.dirty = True

class bundlerepository(localrepo.localrepository):
    def __init__(self, ui, path, bundlename):
        self._tempparent = None
        try:
            localrepo.localrepository.__init__(self, ui, path)
        except error.RepoError:
            self._tempparent = tempfile.mkdtemp()
            localrepo.instance(ui, self._tempparent, 1)
            localrepo.localrepository.__init__(self, ui, self._tempparent)
        self.ui.setconfig('phases', 'publish', False, 'bundlerepo')

        if path:
            self._url = 'bundle:' + util.expandpath(path) + '+' + bundlename
        else:
            self._url = 'bundle:' + bundlename

        self.tempfile = None
        f = util.posixfile(bundlename, "rb")
        self.bundlefile = self.bundle = exchange.readbundle(ui, f, bundlename)
        if self.bundle.compressed():
            fdtemp, temp = self.vfs.mkstemp(prefix="hg-bundle-",
                                            suffix=".hg10un")
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

            f = self.vfs.open(self.tempfile, mode="rb")
            self.bundlefile = self.bundle = exchange.readbundle(ui, f,
                                                                bundlename,
                                                                self.vfs)

        if isinstance(self.bundle, bundle2.unbundle20):
            cgparts = [part for part in self.bundle.iterparts()
                       if (part.type == 'changegroup')
                       and (part.params.get('version', '01')
                            in changegroup.packermap)]

            if not cgparts:
                raise util.Abort('No changegroups found')
            version = cgparts[0].params.get('version', '01')
            cgparts = [p for p in cgparts
                       if p.params.get('version', '01') == version]
            if len(cgparts) > 1:
                raise NotImplementedError("Can't process multiple changegroups")
            part = cgparts[0]

            part.seek(0)
            self.bundle = changegroup.packermap[version][1](part, 'UN')

        # dict with the mapping 'filename' -> position in the bundle
        self.bundlefilespos = {}

        self.firstnewrev = self.changelog.repotiprev + 1
        phases.retractboundary(self, None, phases.draft,
                               [ctx.node() for ctx in self[self.firstnewrev:]])

    @localrepo.unfilteredpropertycache
    def _phasecache(self):
        return bundlephasecache(self, self._phasedefaults)

    @localrepo.unfilteredpropertycache
    def changelog(self):
        # consume the header if it exists
        self.bundle.changelogheader()
        c = bundlechangelog(self.svfs, self.bundle)
        self.manstart = self.bundle.tell()
        return c

    @localrepo.unfilteredpropertycache
    def manifest(self):
        self.bundle.seek(self.manstart)
        # consume the header if it exists
        self.bundle.manifestheader()
        m = bundlemanifest(self.svfs, self.bundle, self.changelog.rev)
        self.filestart = self.bundle.tell()
        return m

    @localrepo.unfilteredpropertycache
    def manstart(self):
        self.changelog
        return self.manstart

    @localrepo.unfilteredpropertycache
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

        if f in self.bundlefilespos:
            self.bundle.seek(self.bundlefilespos[f])
            return bundlefilelog(self.svfs, f, self.bundle, self.changelog.rev)
        else:
            return filelog.filelog(self.svfs, f)

    def close(self):
        """Close assigned bundle file immediately."""
        self.bundlefile.close()
        if self.tempfile is not None:
            self.vfs.unlink(self.tempfile)
        if self._tempparent:
            shutil.rmtree(self._tempparent, True)

    def cancopy(self):
        return False

    def peer(self):
        return bundlepeer(self)

    def getcwd(self):
        return os.getcwd() # always outside the repo


def instance(ui, path, create):
    if create:
        raise util.Abort(_('cannot create new bundle repository'))
    # internal config: bundle.mainreporoot
    parentpath = ui.config("bundle", "mainreporoot", "")
    if not parentpath:
        # try to find the correct path to the working directory repo
        parentpath = cmdutil.findrepo(os.getcwd())
        if parentpath is None:
            parentpath = ''
    if parentpath:
        # Try to make the full path relative so we get a nice, short URL.
        # In particular, we don't want temp dir names in test outputs.
        cwd = os.getcwd()
        if parentpath == cwd:
            parentpath = ''
        else:
            cwd = pathutil.normasprefix(cwd)
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

class bundletransactionmanager(object):
    def transaction(self):
        return None

    def close(self):
        raise NotImplementedError

    def release(self):
        raise NotImplementedError

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

    "local" is a local repo from which to obtain the actual incoming
      changesets; it is a bundlerepo for the obtained bundle when the
      original "other" is remote.
    "csets" lists the incoming changeset node ids.
    "cleanupfn" must be called without arguments when you're done processing
      the changes; it closes both the original "other" and the one returned
      here.
    '''
    tmp = discovery.findcommonincoming(repo, other, heads=onlyheads,
                                       force=force)
    common, incoming, rheads = tmp
    if not incoming:
        try:
            if bundlename:
                os.unlink(bundlename)
        except OSError:
            pass
        return repo, [], other.close

    commonset = set(common)
    rheads = [x for x in rheads if x not in commonset]

    bundle = None
    bundlerepo = None
    localrepo = other.local()
    if bundlename or not localrepo:
        # create a bundle (uncompressed if other repo is not local)

        if other.capable('getbundle'):
            cg = other.getbundle('incoming', common=common, heads=rheads)
        elif onlyheads is None and not other.capable('changegroupsubset'):
            # compat with older servers when pulling all remote heads
            cg = other.changegroup(incoming, "incoming")
            rheads = None
        else:
            cg = other.changegroupsubset(incoming, rheads, 'incoming')
        if localrepo:
            bundletype = "HG10BZ"
        else:
            bundletype = "HG10UN"
        fname = bundle = changegroup.writebundle(ui, cg, bundlename, bundletype)
        # keep written bundle?
        if bundlename:
            bundle = None
        if not localrepo:
            # use the created uncompressed bundlerepo
            localrepo = bundlerepo = bundlerepository(repo.baseui, repo.root,
                                                      fname)
            # this repo contains local and other now, so filter out local again
            common = repo.heads()
    if localrepo:
        # Part of common may be remotely filtered
        # So use an unfiltered version
        # The discovery process probably need cleanup to avoid that
        localrepo = localrepo.unfiltered()

    csets = localrepo.changelog.findmissing(common, rheads)

    if bundlerepo:
        reponodes = [ctx.node() for ctx in bundlerepo[bundlerepo.firstnewrev:]]
        remotephases = other.listkeys('phases')

        pullop = exchange.pulloperation(bundlerepo, other, heads=reponodes)
        pullop.trmanager = bundletransactionmanager()
        exchange._pullapplyphases(pullop, remotephases)

    def cleanup():
        if bundlerepo:
            bundlerepo.close()
        if bundle:
            os.unlink(bundle)
        other.close()

    return (localrepo, csets, cleanup)
