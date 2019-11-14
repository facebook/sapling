# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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

from __future__ import absolute_import

import os
import shutil
import tempfile

from . import (
    bundle2,
    changegroup,
    changelog,
    cmdutil,
    discovery,
    error,
    exchange,
    filelog,
    localrepo,
    manifest,
    mdiff,
    mutation,
    node as nodemod,
    pathutil,
    phases,
    pycompat,
    revlog,
    util,
    vfs as vfsmod,
    visibility,
)
from .i18n import _
from .node import nullid


class bundlerevlog(revlog.revlog):
    def __init__(self, opener, indexfile, cgunpacker, linkmapper):
        # How it works:
        # To retrieve a revision, we need to know the offset of the revision in
        # the bundle (an unbundle object). We store this offset in the index
        # (start). The base of the delta is stored in the base field.
        #
        # To differentiate a rev in the bundle from a rev in the revlog, we
        # check revision against repotiprev.
        opener = vfsmod.readonlyvfs(opener)
        revlog.revlog.__init__(self, opener, indexfile, index2=True)
        self.bundle = cgunpacker
        n = len(self)
        self.repotiprev = n - 1
        self.bundlerevs = set()  # used by 'bundle()' revset expression
        self.bundleheads = set()  # used by visibility
        for deltadata in cgunpacker.deltaiter():
            node, p1, p2, cs, deltabase, delta, flags = deltadata

            size = len(delta)
            start = cgunpacker.tell() - size

            link = linkmapper(cs)
            if node in self.nodemap:
                # this can happen if two branches make the same change
                self.bundlerevs.add(self.nodemap[node])
                continue

            for p in (p1, p2):
                if p not in self.nodemap:
                    raise error.LookupError(p, self.indexfile, _("unknown parent"))

            if deltabase not in self.nodemap:
                raise LookupError(deltabase, self.indexfile, _("unknown delta base"))

            baserev = self.rev(deltabase)
            p1rev = self.rev(p1)
            p2rev = self.rev(p2)
            # start, size, full unc. size, base (unused), link, p1, p2, node
            e = (
                revlog.offset_type(start, flags),
                size,
                -1,
                baserev,
                link,
                p1rev,
                p2rev,
                node,
            )
            self.index.insert(-1, e)
            self.index2.insert([p for p in (p1rev, p2rev) if p >= 0])
            self.nodemap[node] = n
            self.bundlerevs.add(n)
            self.bundleheads.add(n)
            self.bundleheads.discard(p1rev)
            self.bundleheads.discard(p2rev)

            n += 1

    def _chunk(self, rev, df=None):
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

        return mdiff.textdiff(
            self.revision(rev1, raw=True), self.revision(rev2, raw=True)
        )

    def revision(self, nodeorrev, _df=None, raw=False):
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

        rawtext = None
        chain = []
        iterrev = rev
        # reconstruct the revision if it is from a changegroup
        while iterrev > self.repotiprev:
            if self._cache and self._cache[1] == iterrev:
                rawtext = self._cache[2]
                break
            chain.append(iterrev)
            iterrev = self.index[iterrev][3]
        if rawtext is None:
            rawtext = self.baserevision(iterrev)

        while chain:
            delta = self._chunk(chain.pop())
            rawtext = mdiff.patches(rawtext, [delta])

        text, validatehash = self._processflags(
            rawtext, self.flags(rev), "read", raw=raw
        )
        if validatehash:
            self.checkhash(text, node, rev=rev)
        self._cache = (node, rev, rawtext)
        return text

    def baserevision(self, nodeorrev):
        # Revlog subclasses may override 'revision' method to modify format of
        # content retrieved from revlog. To use bundlerevlog with such class one
        # needs to override 'baserevision' and make more specific call here.
        return revlog.revlog.revision(self, nodeorrev, raw=True)

    def addrevision(self, *args, **kwargs):
        raise NotImplementedError

    def addgroup(self, *args, **kwargs):
        raise NotImplementedError

    def strip(self, *args, **kwargs):
        raise NotImplementedError

    def checksize(self):
        raise NotImplementedError


class bundlechangelog(bundlerevlog, changelog.changelog):
    def __init__(self, opener, cgunpacker, uiconfig):
        changelog.changelog.__init__(self, opener, uiconfig)
        linkmapper = lambda x: x
        bundlerevlog.__init__(self, opener, self.indexfile, cgunpacker, linkmapper)
        self._visibleheads.addbundleheads([self.node(r) for r in self.bundleheads])

    def baserevision(self, nodeorrev):
        # Although changelog doesn't override 'revision' method, some extensions
        # may replace this class with another that does. Same story with
        # manifest and filelog classes.

        # This bypasses filtering on changelog.node() and rev() because we need
        # revision text of the bundle base even if it is hidden.
        oldfilter = self.filteredrevs
        try:
            self.filteredrevs = ()
            return changelog.changelog.revision(self, nodeorrev, raw=True)
        finally:
            self.filteredrevs = oldfilter

    def _loadvisibleheads(self, opener):
        return visibility.bundlevisibleheads(opener)


class bundlemanifest(bundlerevlog, manifest.manifestrevlog):
    def __init__(self, opener, cgunpacker, linkmapper, dirlogstarts=None, dir=""):
        manifest.manifestrevlog.__init__(self, opener, dir=dir)
        bundlerevlog.__init__(self, opener, self.indexfile, cgunpacker, linkmapper)
        if dirlogstarts is None:
            dirlogstarts = {}
            if self.bundle.version == "03":
                dirlogstarts = _getfilestarts(self.bundle)
        self._dirlogstarts = dirlogstarts
        self._linkmapper = linkmapper

    def baserevision(self, nodeorrev):
        node = nodeorrev
        if isinstance(node, int):
            node = self.node(node)

        if node in self.fulltextcache:
            result = "%s" % self.fulltextcache[node]
        else:
            result = manifest.manifestrevlog.revision(self, nodeorrev, raw=True)
        return result

    def dirlog(self, d):
        if d in self._dirlogstarts:
            self.bundle.seek(self._dirlogstarts[d])
            return bundlemanifest(
                self.opener, self.bundle, self._linkmapper, self._dirlogstarts, dir=d
            )
        return super(bundlemanifest, self).dirlog(d)


class bundlefilelog(bundlerevlog, filelog.filelog):
    def __init__(self, opener, path, cgunpacker, linkmapper):
        filelog.filelog.__init__(self, opener, path)
        bundlerevlog.__init__(self, opener, self.indexfile, cgunpacker, linkmapper)

    def baserevision(self, nodeorrev):
        return filelog.filelog.revision(self, nodeorrev, raw=True)


class bundlepeer(localrepo.localpeer):
    def canpush(self):
        return False


class bundlephasecache(phases.phasecache):
    def __init__(self, *args, **kwargs):
        super(bundlephasecache, self).__init__(*args, **kwargs)
        if util.safehasattr(self, "opener"):
            self.opener = vfsmod.readonlyvfs(self.opener)

    def write(self):
        raise NotImplementedError

    def _write(self, fp):
        raise NotImplementedError

    def _updateroots(self, phase, newroots, tr):
        self.phaseroots[phase] = newroots
        self.invalidate()
        self.dirty = True


def _getfilestarts(cgunpacker):
    filespos = {}
    for chunkdata in iter(cgunpacker.filelogheader, {}):
        fname = chunkdata["filename"]
        filespos[fname] = cgunpacker.tell()
        for chunk in iter(lambda: cgunpacker.deltachunk(None), {}):
            pass
    return filespos


class bundlerepository(localrepo.localrepository):
    """A repository instance that is a union of a local repo and a bundle.

    Instances represent a read-only repository composed of a local repository
    with the contents of a bundle file applied. The repository instance is
    conceptually similar to the state of a repository after an
    ``hg unbundle`` operation. However, the contents of the bundle are never
    applied to the actual base repository.
    """

    def __init__(self, ui, repopath, bundlepath):
        self._tempparent = None
        try:
            localrepo.localrepository.__init__(self, ui, repopath)
        except error.RepoError:
            self._tempparent = tempfile.mkdtemp()
            localrepo.instance(ui, self._tempparent, 1)
            localrepo.localrepository.__init__(self, ui, self._tempparent)
        self.ui.setconfig("phases", "publish", False, "bundlerepo")

        if repopath:
            self._url = "bundle:" + util.expandpath(repopath) + "+" + bundlepath
        else:
            self._url = "bundle:" + bundlepath

        self.tempfile = None
        f = util.posixfile(bundlepath, "rb")
        bundle = exchange.readbundle(ui, f, bundlepath)

        if isinstance(bundle, bundle2.unbundle20):
            self._bundlefile = bundle
            self._cgunpacker = None

            cgpart = None
            for part in bundle.iterparts(seekable=True):
                if part.type == "changegroup":
                    if cgpart:
                        raise NotImplementedError(
                            "can't process " "multiple changegroups"
                        )
                    cgpart = part

                self._handlebundle2part(bundle, part)

            if not cgpart:
                raise error.Abort(_("No changegroups found"))

            # This is required to placate a later consumer, which expects
            # the payload offset to be at the beginning of the changegroup.
            # We need to do this after the iterparts() generator advances
            # because iterparts() will seek to end of payload after the
            # generator returns control to iterparts().
            cgpart.seek(0, os.SEEK_SET)

        elif isinstance(bundle, changegroup.cg1unpacker):
            if bundle.compressed():
                f = self._writetempbundle(bundle.read, ".hg10un", header="HG10UN")
                bundle = exchange.readbundle(ui, f, bundlepath, self.localvfs)

            self._bundlefile = bundle
            self._cgunpacker = bundle
        else:
            raise error.Abort(_("bundle type %s cannot be read") % type(bundle))

        # dict with the mapping 'filename' -> position in the changegroup.
        self._cgfilespos = {}

        self.firstnewrev = self.changelog.repotiprev + 1
        phases.retractboundary(
            self, None, phases.draft, [ctx.node() for ctx in self[self.firstnewrev :]]
        )

    def _handlebundle2part(self, bundle, part):
        if part.type != "changegroup":
            return

        cgstream = part
        version = part.params.get("version", "01")
        legalcgvers = changegroup.supportedincomingversions(self)
        if version not in legalcgvers:
            msg = _("Unsupported changegroup version: %s")
            raise error.Abort(msg % version)
        cgstream = self._writetempbundle(part.read, ".cg%sun" % version)

        self._cgunpacker = changegroup.getunbundler(version, cgstream, "UN")

    def _writetempbundle(self, readfn, suffix, header=""):
        """Write a temporary file to disk
        """
        fdtemp, temp = self.localvfs.mkstemp(prefix="hg-bundle-", suffix=suffix)
        self.tempfile = temp

        with util.fdopen(fdtemp, pycompat.sysstr("wb")) as fptemp:
            fptemp.write(header)
            while True:
                chunk = readfn(2 ** 18)
                if not chunk:
                    break
                fptemp.write(chunk)

        return self.localvfs.open(self.tempfile, mode="rb")

    @localrepo.unfilteredpropertycache
    def _phasecache(self):
        return bundlephasecache(self, self._phasedefaults)

    @localrepo.unfilteredpropertycache
    def _mutationstore(self):
        return mutation.bundlemutationstore(self)

    @localrepo.unfilteredpropertycache
    def changelog(self):
        # consume the header if it exists
        self._cgunpacker.changelogheader()
        c = bundlechangelog(self.svfs, self._cgunpacker, self.ui.uiconfig())
        self.manstart = self._cgunpacker.tell()
        return c

    @localrepo.unfilteredpropertycache
    def manifestlog(self):
        return super(bundlerepository, self).manifestlog

    def _constructmanifest(self):
        self._cgunpacker.seek(self.manstart)
        # consume the header if it exists
        self._cgunpacker.manifestheader()
        linkmapper = self.unfiltered().changelog.rev
        m = bundlemanifest(self.svfs, self._cgunpacker, linkmapper)
        self.filestart = self._cgunpacker.tell()
        return m

    def _consumemanifest(self):
        """Consumes the manifest portion of the bundle, setting filestart so the
        file portion can be read."""
        self._cgunpacker.seek(self.manstart)
        self._cgunpacker.manifestheader()
        for delta in self._cgunpacker.deltaiter():
            pass

        # Changegroup v3 supports additional manifest entries that we need to
        # skip.
        if self._cgunpacker.version == "03":
            for chunkdata in iter(self._cgunpacker.filelogheader, {}):
                # If we get here, there are directory manifests in the changegroup
                for delta in self._cgunpacker.deltaiter():
                    pass

        self.filestart = self._cgunpacker.tell()

    @localrepo.unfilteredpropertycache
    def manstart(self):
        self.changelog
        return self.manstart

    @localrepo.unfilteredpropertycache
    def filestart(self):
        self.manifestlog

        # If filestart was not set by self.manifestlog, that means the
        # manifestlog implementation did not consume the manifests from the
        # changegroup (ex: it might be consuming trees from a separate bundle2
        # part instead). So we need to manually consume it.
        if "filestart" not in self.__dict__:
            self._consumemanifest()

        return self.filestart

    def url(self):
        return self._url

    def file(self, f):
        if not self._cgfilespos:
            self._cgunpacker.seek(self.filestart)
            self._cgfilespos = _getfilestarts(self._cgunpacker)

        if f in self._cgfilespos:
            self._cgunpacker.seek(self._cgfilespos[f])
            linkmapper = self.unfiltered().changelog.rev
            return bundlefilelog(self.svfs, f, self._cgunpacker, linkmapper)
        else:
            return filelog.filelog(self.svfs, f)

    def close(self):
        """Close assigned bundle file immediately."""
        self._bundlefile.close()
        if self.tempfile is not None:
            self.localvfs.unlink(self.tempfile)
        if self._tempparent:
            shutil.rmtree(self._tempparent, True)

    def cancopy(self):
        return False

    def peer(self):
        return bundlepeer(self)

    def getcwd(self):
        return pycompat.getcwd()  # always outside the repo

    # Check if parents exist in localrepo before setting
    def setparents(self, p1, p2=nullid):
        p1rev = self.changelog.rev(p1)
        p2rev = self.changelog.rev(p2)
        msg = _("setting parent to node %s that only exists in the bundle\n")
        if self.changelog.repotiprev < p1rev:
            self.ui.warn(msg % nodemod.hex(p1))
        if self.changelog.repotiprev < p2rev:
            self.ui.warn(msg % nodemod.hex(p2))
        return super(bundlerepository, self).setparents(p1, p2)


def instance(ui, path, create):
    if create:
        raise error.Abort(_("cannot create new bundle repository"))
    # internal config: bundle.mainreporoot
    parentpath = ui.config("bundle", "mainreporoot")
    if not parentpath:
        # try to find the correct path to the working directory repo
        parentpath = cmdutil.findrepo(pycompat.getcwd())
        if parentpath is None:
            parentpath = ""
    if parentpath:
        # Try to make the full path relative so we get a nice, short URL.
        # In particular, we don't want temp dir names in test outputs.
        cwd = pycompat.getcwd()
        if parentpath == cwd:
            parentpath = ""
        else:
            cwd = pathutil.normasprefix(cwd)
            if parentpath.startswith(cwd):
                parentpath = parentpath[len(cwd) :]
    u = util.url(path)
    path = u.localpath()
    if u.scheme == "bundle":
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


def getremotechanges(ui, repo, other, onlyheads=None, bundlename=None, force=False):
    """obtains a bundle of changes incoming from other

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
    """
    tmp = discovery.findcommonincoming(repo, other, heads=onlyheads, force=force)
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

        # developer config: devel.legacy.exchange
        legexc = ui.configlist("devel", "legacy.exchange")
        forcebundle1 = "bundle2" not in legexc and "bundle1" in legexc
        canbundle2 = (
            not forcebundle1 and other.capable("getbundle") and other.capable("bundle2")
        )
        if canbundle2:
            kwargs = {}
            kwargs[r"common"] = common
            kwargs[r"heads"] = rheads
            kwargs[r"bundlecaps"] = exchange.caps20to10(repo)
            kwargs[r"cg"] = True
            b2 = other.getbundle("incoming", **kwargs)
            fname = bundle = changegroup.writechunks(
                ui, b2._forwardchunks(), bundlename
            )
        else:
            if other.capable("getbundle"):
                cg = other.getbundle("incoming", common=common, heads=rheads)
            elif onlyheads is None and not other.capable("changegroupsubset"):
                # compat with older servers when pulling all remote heads
                cg = other.changegroup(incoming, "incoming")
                rheads = None
            else:
                cg = other.changegroupsubset(incoming, rheads, "incoming")
            if localrepo:
                bundletype = "HG10BZ"
            else:
                bundletype = "HG10UN"
            fname = bundle = bundle2.writebundle(ui, cg, bundlename, bundletype)
        # keep written bundle?
        if bundlename:
            bundle = None
        if not localrepo:
            # use the created uncompressed bundlerepo
            localrepo = bundlerepo = bundlerepository(repo.baseui, repo.root, fname)
            # this repo contains local and other now, so filter out local again
            common = repo.heads()
    if localrepo:
        # Part of common may be remotely filtered
        # So use an unfiltered version
        # The discovery process probably need cleanup to avoid that
        localrepo = localrepo.unfiltered()

    csets = localrepo.changelog.findmissing(common, rheads)

    if bundlerepo:
        reponodes = [ctx.node() for ctx in bundlerepo[bundlerepo.firstnewrev :]]
        remotephases = other.listkeys("phases")

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
