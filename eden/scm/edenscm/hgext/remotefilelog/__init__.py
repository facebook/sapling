# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# __init__.py - remotefilelog extension
"""minimize and speed up large repositories

remotefilelog allows leaving file contents on the server and only downloading
them ondemand as needed.

Configs:

    ``packs.maxchainlen`` specifies the maximum delta chain length in pack files

    ``packs.maxpacksize`` specifies the maximum pack file size

    ``packs.maxpackfilecount`` specifies the maximum number of packs in the
    shared cache (trees only for now)

    ``remotefilelog.backgroundprefetch`` runs prefetch in background when True

    update, and on other commands that use them. Different from pullprefetch.

    ``remotefilelog.gcrepack`` does garbage collection during repack when True

    ``remotefilelog.nodettl`` specifies maximum TTL of a node in seconds before
    it is garbage collected

    ``remotefilelog.localdatarepack`` runs repack on local data loose files

    ``remotefilelog.getfilesstep`` the number of files per batch during fetching

    ``remotefilelog.prefetchdays`` specifies the maximum age of a commit in
    days after which it is no longer prefetched.

    ``remotefilelog.prefetchchunksize`` specifies how many files to fetch from the
    server in one go.

    ``remotefilelog.prefetchdelay`` specifies delay between background
    prefetches in seconds after operations that change the working copy parent

    ``remotefilelog.data.gencountlimit`` constraints the minimum number of data
    pack files required to be considered part of a generation. In particular,
    minimum number of packs files > gencountlimit.

    ``remotefilelog.data.generations`` list for specifying the lower bound of
    each generation of the data pack files. For example, list ['100MB','1MB']
    or ['1MB', '100MB'] will lead to three generations: [0, 1MB), [
    1MB, 100MB) and [100MB, infinity).

    ``remotefilelog.data.maxrepackpacks`` the maximum number of pack files to
    include in an incremental data repack.

    ``remotefilelog.data.repackmaxpacksize`` the maximum size of a pack file for
    it to be considered for an incremental data repack.

    ``remotefilelog.data.repacksizelimit`` the maximum total size of pack files
    to include in an incremental data repack.

    ``remotefilelog.history.gencountlimit`` constraints the minimum number of
    history pack files required to be considered part of a generation. In
    particular, minimum number of packs files > gencountlimit.

    ``remotefilelog.history.generations`` list for specifying the lower bound of
    each generation of the history pack files. For example, list [
    '100MB', '1MB'] or ['1MB', '100MB'] will lead to three generations: [
    0, 1MB), [1MB, 100MB) and [100MB, infinity).

    ``remotefilelog.history.maxrepackpacks`` the maximum number of pack files to
    include in an incremental history repack.

    ``remotefilelog.history.repackmaxpacksize`` the maximum size of a pack file
    for it to be considered for an incremental history repack.

    ``remotefilelog.history.repacksizelimit`` the maximum total size of pack
    files to include in an incremental history repack.

    ``remotefilelog.dolfsprefetch`` means that fileserverclient's prefetch
    will also cause lfs prefetch to happen. This is True by default.

    ``remotefilelog.updatesharedcache`` is used to prevent writing data to the
    shared remotefilelog cache. This can be useful to prevent poisoning cache
    while using experimental remotefilelog store.

    ``remotefilelog.descendantrevfastpath`` controls whether to use the
    linkrev-fixup fastpath when creating a filectx from a descendant rev.
    The default is true, but this may make some operations cause many tree
    fetches when used in conjunction with treemanifest in treeonly mode.

    ``remotefilelog.cleanoldpacks`` controls whether repack will attempt to
    limit the size of its cache.

    ``remotefilelog.cachelimit`` limit the size of the hgcache to this size.
    Packfiles will be removed from oldest to newest during repack.

    ``remotefilelog.manifestlimit`` limit the size of the manifest cache to this size.
    Manifests will be removed from oldest to newest during repack.

    ``remotefilelog.getpackversion`` version of the "getpack" wire protocol.
    Starting with 2, LFS blobs are supported.

    ``format.userustmutablestore`` switches to using the rust mutable stores.

    ``treemanifest.blocksendflat`` causes an exception to be thrown if the
    current repository attempts to add flat manifests to a changegroup.

    ``treemanifest.forceallowflat`` lets a client tell the server that it
    requires flat manifests, despite blocksendflat being set. This is primarily
    used for mirroring infrastructure.

    ``remotefilelog.simplecacheserverstore`` use simplecache as cache implementation.

    ``remotefilelog.indexedlogdatastore`` use an IndexedLog content store.

    ``remotefilelog.indexedloghistorystore`` use an IndexedLog history store.

    ``remotefilelog.userustpackstore`` use the Rust PackStore.

    ``remotefilelog.cachekey`` cache key prefix to use.

    ``edenapi.url`` URL of the EdenAPI server.

    ``remotefilelog.http`` use HTTP (EdenAPI) instead of SSH to fetch data.
"""
from __future__ import absolute_import

import os
import time
from contextlib import contextmanager

from edenscm.mercurial import (
    archival,
    bundle2,
    changegroup,
    changelog2,
    cmdutil,
    commands,
    context,
    copies,
    error,
    exchange,
    extensions,
    git,
    hg,
    localrepo,
    match,
    merge,
    patch,
    pycompat,
    registrar,
    repair,
    revset,
    scmutil,
    smartset,
    store,
    templatekw,
    util,
)
from edenscm.mercurial.commands import debug as hgdebugcommands
from edenscm.mercurial.extensions import wrapfunction
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex
from edenscm.mercurial.pycompat import isint, sysplatform

from . import (
    debugcommands,
    fileserverclient,
    remotefilectx,
    remotefilelog,
    remotefilelogserver,
    repack as repackmod,
    shallowbundle,
    shallowrepo,
    shallowstore,
)


# ensures debug commands are registered
hgdebugcommands.command

try:
    from edenscm.mercurial import streamclone

    streamclone._walkstreamfiles
    hasstreamclone = True
except Exception:
    hasstreamclone = False

cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem("remotefilelog", "descendantrevfastpath", default=False)
configitem("remotefilelog", "localdatarepack", default=False)
configitem("remotefilelog", "updatesharedcache", default=True)
configitem("remotefilelog", "servercachepath", default=None)
configitem("remotefilelog", "simplecacheserverstore", default=False)
configitem("remotefilelog", "server", default=None)
configitem("remotefilelog", "getpackversion", default=1)
configitem("remotefilelog", "commitsperrepack", default=100)
configitem("remotefilelog", "http", default=True)
configitem("edenapi", "url", default=None)

testedwith = "ships-with-fb-hgext"

repoclass = localrepo.localrepository
if util.safehasattr(repoclass, "_basesupported"):
    repoclass._basesupported.add(shallowrepo.requirement)
else:
    # hg <= 2.7
    repoclass.supported.add(shallowrepo.requirement)


def uisetup(ui):
    """Wraps user facing Mercurial commands to swap them out with shallow
    versions.
    """
    hg.wirepeersetupfuncs.append(fileserverclient.peersetup)

    extensions.wrapcommand(commands.table, "clone", cloneshallow)
    extensions.wrapcommand(commands.table, "debugindex", debugcommands.debugindex)
    extensions.wrapcommand(commands.table, "debugindexdot", debugcommands.debugindexdot)
    extensions.wrapcommand(commands.table, "log", log)
    extensions.wrapcommand(commands.table, "pull", pull)
    extensions.wrapfunction(bundle2, "getrepocaps", getrepocaps)

    # Prevent 'hg manifest --all'
    def _manifest(orig, ui, repo, *args, **opts):
        if shallowrepo.requirement in repo.requirements and opts.get("all"):
            raise error.Abort(_("--all is not supported in a shallow repo"))

        return orig(ui, repo, *args, **opts)

    extensions.wrapcommand(commands.table, "manifest", _manifest)

    # Wrap remotefilelog with lfs code
    def _lfsloaded(loaded=False):
        lfsmod = None
        try:
            lfsmod = extensions.find("lfs")
        except KeyError:
            pass
        if lfsmod:
            lfsmod.wrapfilelog(ui, remotefilelog.remotefilelog)
            fileserverclient._lfsmod = lfsmod

    extensions.afterloaded("lfs", _lfsloaded)

    # debugdata needs remotefilelog.len to work
    extensions.wrapcommand(commands.table, "debugdata", debugdatashallow)

    wrappackers()


def getrepocaps(orig, repo, *args, **kwargs):
    caps = orig(repo, *args, **kwargs)
    if shallowrepo.requirement in repo.requirements:
        caps["remotefilelog"] = ("True",)
        if repo.ui.configbool("treemanifest", "forceallowflat"):
            caps["allowflatmanifest"] = ("True",)
    return caps


def wrappackers():
    # some users in core still call changegroup.cg1packer directly
    changegroup.cg1packer = shallowbundle.shallowcg1packer

    packermap = None
    if util.safehasattr(changegroup, "packermap"):
        packermap = changegroup.packermap
    elif util.safehasattr(changegroup, "_packermap"):
        packermap = changegroup._packermap

    if packermap:
        # Mercurial >= 3.3
        packermap01 = packermap["01"]
        packermap02 = packermap["02"]
        packermap03 = packermap["03"]
        packermap["01"] = (shallowbundle.shallowcg1packer, packermap01[1])
        packermap["02"] = (shallowbundle.shallowcg2packer, packermap02[1])
        packermap["03"] = (shallowbundle.shallowcg3packer, packermap03[1])


def cloneshallow(orig, ui, source, *args, **opts):
    # skip for (full) git repos
    if opts.get("git"):
        giturl = source
    else:
        giturl = git.maybegiturl(source)
    if opts.get("shallow") and giturl is None:
        repos = []

        def pull_shallow(orig, self, *args, **kwargs):
            repos.append(self)
            # set up the client hooks so the post-clone update works
            setupclient(self.ui, self)

            if shallowrepo.requirement not in self.requirements:
                self.requirements.add(shallowrepo.requirement)
                self._writerequirements()

            # Since setupclient hadn't been called, exchange.pull was not
            # wrapped. So we need to manually invoke our version of it.
            return exchangepull(orig, self, *args, **kwargs)

        wrapfunction(exchange, "pull", pull_shallow)

        if hasstreamclone:

            def canperformstreamclone(orig, *args, **kwargs):
                supported, requirements = orig(*args, **kwargs)
                if requirements is not None:
                    requirements.add(shallowrepo.requirement)
                return supported, requirements

            wrapfunction(streamclone, "canperformstreamclone", canperformstreamclone)
        else:

            def stream_in_shallow(orig, repo, remote, requirements):
                requirements.add(shallowrepo.requirement)
                return orig(repo, remote, requirements)

            wrapfunction(localrepo.localrepository, "stream_in", stream_in_shallow)

    return orig(ui, source, *args, **opts)


def debugdatashallow(orig, *args, **kwds):
    oldlen = remotefilelog.remotefilelog.__len__
    try:
        remotefilelog.remotefilelog.__len__ = lambda x: 1
        return orig(*args, **kwds)
    finally:
        remotefilelog.remotefilelog.__len__ = oldlen


def reposetup(ui, repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    isserverenabled = ui.configbool("remotefilelog", "server")
    isshallowclient = shallowrepo.requirement in repo.requirements

    if isserverenabled and isshallowclient:
        raise RuntimeError("Cannot be both a server and shallow client.")

    if isshallowclient:
        setupclient(ui, repo)

    if isserverenabled:
        remotefilelogserver.onetimesetup(ui)


def uploadblobs(repo, nodes):
    toupload = []
    for ctx in repo.set("%ln - public()", nodes):
        for f in ctx.files():
            if f not in ctx:
                continue

            fctx = ctx[f]
            toupload.append((fctx.path(), fctx.filenode()))
    repo.fileslog.contentstore.upload(toupload)


def prepush(pushop):
    uploadblobs(pushop.repo, pushop.outgoing.missing)


def setupclient(ui, repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    # Even clients get the server setup since they need to have the
    # wireprotocol endpoints registered.
    remotefilelogserver.onetimesetup(ui)
    onetimeclientsetup(ui)

    repo.prepushoutgoinghooks.add("remotefilelog", prepush)

    shallowrepo.wraprepo(repo)
    repo.store = shallowstore.wrapstore(repo.store)


clientonetime = False


def onetimeclientsetup(ui):
    global clientonetime
    if clientonetime:
        return
    clientonetime = True

    if util.safehasattr(changegroup, "_addchangegroupfiles"):
        fn = "_addchangegroupfiles"  # hg >= 3.6
    else:
        fn = "addchangegroupfiles"  # hg <= 3.5
    wrapfunction(changegroup, fn, shallowbundle.addchangegroupfiles)
    if util.safehasattr(changegroup, "getchangegroup"):
        wrapfunction(changegroup, "getchangegroup", shallowbundle.getchangegroup)
    else:
        wrapfunction(changegroup, "makechangegroup", shallowbundle.makechangegroup)

    def storewrapper(orig, requirements, path, vfstype, *args):
        s = orig(requirements, path, vfstype, *args)
        if shallowrepo.requirement in requirements:
            s = shallowstore.wrapstore(s)

        return s

    wrapfunction(store, "store", storewrapper)

    extensions.wrapfunction(exchange, "pull", exchangepull)

    # prefetch files before update
    def applyupdates(
        orig, repo, actions, wctx, mctx, overwrite, labels=None, ancestors=None
    ):
        if shallowrepo.requirement in repo.requirements:
            manifest = mctx.manifest()
            files = []
            for f, args, msg in actions["g"]:
                files.append((f, hex(manifest[f])))
            # batch fetch the needed files from the server
            repo.fileservice.prefetch(files, fetchhistory=False)
        return orig(
            repo, actions, wctx, mctx, overwrite, labels=labels, ancestors=ancestors
        )

    wrapfunction(merge, "applyupdates", applyupdates)

    # Prefetch merge checkunknownfiles
    def checkunknownfiles(orig, repo, wctx, mctx, force, actions, *args, **kwargs):
        if shallowrepo.requirement in repo.requirements:
            files = []
            sparsematch = repo.maybesparsematch(mctx.rev())
            for f, (m, actionargs, msg) in pycompat.iteritems(actions):
                if sparsematch and not sparsematch(f):
                    continue
                if m in ("c", "dc", "cm"):
                    files.append((f, hex(mctx.filenode(f))))
                elif m == "dg":
                    f2 = actionargs[0]
                    files.append((f2, hex(mctx.filenode(f2))))
            # We need history for the files so we can compute the sha(p1, p2,
            # text) for the files on disk. This will unfortunately fetch all the
            # history for the files, which is excessive. In the future we should
            # change this to fetch the sha256 and size, then we can avoid p1, p2
            # entirely.
            repo.fileservice.prefetch(files, fetchdata=False, fetchhistory=True)
        return orig(repo, wctx, mctx, force, actions, *args, **kwargs)

    wrapfunction(merge, "_checkunknownfiles", checkunknownfiles)

    # Prefetch the logic that compares added and removed files for renames
    def findrenames(orig, repo, matcher, added, removed, *args, **kwargs):
        if shallowrepo.requirement in repo.requirements:
            files = []
            parentctx = repo["."]
            m1 = parentctx.manifest()
            for f in removed:
                if f in m1:
                    files.append((f, hex(parentctx.filenode(f))))
            # batch fetch the needed files from the server
            repo.fileservice.prefetch(files)
        return orig(repo, matcher, added, removed, *args, **kwargs)

    wrapfunction(scmutil, "_findrenames", findrenames)

    # prefetch files before mergecopies check
    def computenonoverlap(orig, repo, c1, c2, *args, **kwargs):
        u1, u2 = orig(repo, c1, c2, *args, **kwargs)
        if shallowrepo.requirement in repo.requirements:
            m1 = c1.manifest()
            m2 = c2.manifest()
            files = []

            sparsematch1 = repo.maybesparsematch(c1.rev())
            if sparsematch1:
                sparseu1 = []
                for f in u1:
                    if sparsematch1(f):
                        files.append((f, hex(m1[f])))
                        sparseu1.append(f)
                u1 = sparseu1

            sparsematch2 = repo.maybesparsematch(c2.rev())
            if sparsematch2:
                sparseu2 = []
                for f in u2:
                    if sparsematch2(f):
                        files.append((f, hex(m2[f])))
                        sparseu2.append(f)
                u2 = sparseu2

            # batch fetch the needed files from the server
            repo.fileservice.prefetch(files)
        return u1, u2

    wrapfunction(copies, "_computenonoverlap", computenonoverlap)

    # prefetch files before pathcopies check
    def computeforwardmissing(orig, a, b, match=None):
        missing = list(orig(a, b, match=match))
        repo = a._repo
        if shallowrepo.requirement in repo.requirements:
            mb = b.manifest()

            files = []
            sparsematch = repo.maybesparsematch(b.rev())
            if sparsematch:
                sparsemissing = []
                for f in missing:
                    if sparsematch(f):
                        files.append((f, hex(mb[f])))
                        sparsemissing.append(f)
                missing = sparsemissing

            # batch fetch the needed files from the server
            repo.fileservice.prefetch(files)
        return missing

    wrapfunction(copies, "_computeforwardmissing", computeforwardmissing)

    # prefetch files before archiving
    def computefiles(orig, ctx, matchfn):
        files = orig(ctx, matchfn)

        repo = ctx._repo
        if shallowrepo.requirement in repo.requirements:
            # Don't run on memory commits, since they may contain files without
            # hashes, which can screw up prefetch.
            if ctx.node() is not None:
                mf = ctx.manifest()
                repo.fileservice.prefetch(list((f, hex(mf.get(f))) for f in files))

        return files

    wrapfunction(archival, "computefiles", computefiles)

    # disappointing hacks below
    templatekw.getrenamedfn = getrenamedfn
    wrapfunction(revset, "filelog", filelogrevset)
    revset.symbols["filelog"] = revset.filelog
    wrapfunction(cmdutil, "walkfilerevs", walkfilerevs)

    # prevent strip from stripping remotefilelogs
    def _collectbrokencsets(orig, repo, files, striprev):
        if shallowrepo.requirement in repo.requirements:
            files = list([f for f in files if not repo.shallowmatch(f)])
        return orig(repo, files, striprev)

    wrapfunction(repair, "_collectbrokencsets", _collectbrokencsets)

    # Don't commit filelogs until we know the commit hash, since the hash
    # is present in the filelog blob.
    # This violates Mercurial's filelog->manifest->changelog write order,
    # but is generally fine for client repos.
    pendingfilecommits = []

    def addrawrevision(
        orig,
        self,
        rawtext,
        transaction,
        link,
        p1,
        p2,
        node,
        flags,
        cachedelta=None,
        _metatuple=None,
    ):
        if isint(link):
            pendingfilecommits.append(
                (
                    self,
                    rawtext,
                    transaction,
                    link,
                    p1,
                    p2,
                    node,
                    flags,
                    cachedelta,
                    _metatuple,
                )
            )
            return node
        else:
            return orig(
                self,
                rawtext,
                transaction,
                link,
                p1,
                p2,
                node,
                flags,
                cachedelta,
                _metatuple=_metatuple,
            )

    wrapfunction(remotefilelog.remotefilelog, "addrawrevision", addrawrevision)

    def changelogadd(orig, self, *args):
        oldlen = len(self)
        node = orig(self, *args)
        newlen = len(self)
        if oldlen != newlen:
            linknode = node
            for oldargs in pendingfilecommits:
                log, rt, tr, _link, p1, p2, n, fl, c, m = oldargs
                log.addrawrevision(rt, tr, linknode, p1, p2, n, fl, c, m)
        else:
            # "link" is actually wrong here (it is set to len(changelog))
            # if changelog remains unchanged, skip writing file revisions
            # but still do a sanity check about pending multiple revisions
            if len(set(x[3] for x in pendingfilecommits)) > 1:
                raise error.ProgrammingError(
                    "pending multiple integer revisions are not supported"
                )
        del pendingfilecommits[:]
        return node

    wrapfunction(changelog2.changelog, "add", changelogadd)

    # changectx wrappers
    def filectx(orig, self, path, fileid=None, filelog=None):
        if fileid is None:
            fileid = self.filenode(path)
        if (
            shallowrepo.requirement in self._repo.requirements
            and self._repo.shallowmatch(path)
        ):
            return remotefilectx.remotefilectx(
                self._repo, path, fileid=fileid, changectx=self, filelog=filelog
            )
        return orig(self, path, fileid=fileid, filelog=filelog)

    wrapfunction(context.changectx, "filectx", filectx)

    def workingfilectx(orig, self, path, filelog=None):
        if (
            shallowrepo.requirement in self._repo.requirements
            and self._repo.shallowmatch(path)
        ):
            return remotefilectx.remoteworkingfilectx(
                self._repo, path, workingctx=self, filelog=filelog
            )
        return orig(self, path, filelog=filelog)

    wrapfunction(context.workingctx, "filectx", workingfilectx)

    # prefetch required revisions before a diff
    def trydiff(
        orig,
        repo,
        revs,
        ctx1,
        ctx2,
        modified,
        added,
        removed,
        copy,
        getfilectx,
        *args,
        **kwargs,
    ):
        if shallowrepo.requirement in repo.requirements:
            prefetch = []
            mf1 = ctx1.manifest()
            for fname in modified + added + removed:
                if fname in mf1:
                    fnode = getfilectx(fname, ctx1).filenode()
                    # fnode can be None if it's a edited working ctx file
                    if fnode:
                        prefetch.append((fname, hex(fnode)))
                if fname not in removed:
                    fnode = getfilectx(fname, ctx2).filenode()
                    if fnode:
                        prefetch.append((fname, hex(fnode)))

            repo.fileservice.prefetch(prefetch)

        return orig(
            repo,
            revs,
            ctx1,
            ctx2,
            modified,
            added,
            removed,
            copy,
            getfilectx,
            *args,
            **kwargs,
        )

    wrapfunction(patch, "trydiff", trydiff)

    if util.safehasattr(cmdutil, "_revertprefetch"):
        wrapfunction(cmdutil, "_revertprefetch", _revertprefetch)
    else:
        wrapfunction(cmdutil, "revert", revert)

    def writenewbundle(
        orig, ui, repo, source, filename, bundletype, outgoing, *args, **kwargs
    ):
        if shallowrepo.requirement in repo.requirements:
            uploadblobs(repo, outgoing.missing)
        return orig(ui, repo, source, filename, bundletype, outgoing, *args, **kwargs)

    # when writing a bundle via "hg bundle" command, upload related LFS blobs
    wrapfunction(bundle2, "writenewbundle", writenewbundle)

    if ui.configbool("remotefilelog", "lfs"):
        # Make bundle choose changegroup3 instead of changegroup2. This affects
        # "hg bundle" command. Note: it does not cover all bundle formats like
        # "packed1". Using "packed1" with lfs will likely cause trouble.
        names = [k for k, v in exchange._bundlespeccgversions.items() if v == "02"]
        for k in names:
            exchange._bundlespeccgversions[k] = "03"


def getrenamedfn(repo, endrev=None):
    rcache = {}

    def getrenamed(fn, rev):
        """looks up all renames for a file (up to endrev) the first
        time the file is given. It indexes on the changerev and only
        parses the manifest if linkrev != changerev.
        Returns rename info for fn at changerev rev."""
        if rev in rcache.setdefault(fn, {}):
            return rcache[fn][rev]

        try:
            fctx = repo[rev].filectx(fn)
            for ancestor in fctx.ancestors():
                if ancestor.path() == fn:
                    renamed = ancestor.renamed()
                    rcache[fn][ancestor.rev()] = renamed

            return fctx.renamed()
        except error.LookupError:
            return None

    return getrenamed


def walkfilerevs(orig, repo, match, follow, revs, fncache):
    if not shallowrepo.requirement in repo.requirements:
        return orig(repo, match, follow, revs, fncache)

    # remotefilelog's can't be walked in rev order, so throw.
    # The caller will see the exception and walk the commit tree instead.
    if not follow:
        raise cmdutil.FileWalkError("Cannot walk via filelog")

    wanted = set()
    minrev, maxrev = min(revs), max(revs)

    pctx = repo["."]
    for filename in match.files():
        if filename not in pctx:
            raise error.Abort(
                _("cannot follow file not in parent " 'revision: "%s"') % filename
            )
        fctx = pctx[filename]

        linkrev = fctx.linkrev()
        if linkrev >= minrev and linkrev <= maxrev:
            fncache.setdefault(linkrev, []).append(filename)
            wanted.add(linkrev)

        for ancestor in fctx.ancestors():
            linkrev = ancestor.linkrev()
            if linkrev >= minrev and linkrev <= maxrev:
                fncache.setdefault(linkrev, []).append(ancestor.path())
                wanted.add(linkrev)

    return wanted


def filelogrevset(orig, repo, subset, x):
    """``filelog(pattern)``
    Changesets connected to the specified filelog.

    For performance reasons, ``filelog()`` does not show every changeset
    that affects the requested file(s). See :hg:`help log` for details. For
    a slower, more accurate result, use ``file()``.
    """

    if not shallowrepo.requirement in repo.requirements:
        return orig(repo, subset, x)

    # i18n: "filelog" is a keyword
    pat = revset.getstring(x, _("filelog requires a pattern"))
    m = match.match(repo.root, repo.getcwd(), [pat], default="relpath", ctx=repo[None])
    s = set()

    if not match.patkind(pat):
        # slow
        for r in subset:
            ctx = repo[r]
            cfiles = ctx.files()
            for f in m.files():
                if f in cfiles:
                    s.add(ctx.rev())
                    break
    else:
        # partial
        files = (f for f in repo[None] if m(f))
        for f in files:
            fctx = repo[None].filectx(f)
            s.add(fctx.linkrev())
            for actx in fctx.ancestors():
                s.add(actx.linkrev())

    return smartset.baseset([r for r in subset if r in s], repo=repo)


@contextmanager
def openrepo(ui, repopath):
    repo = None
    try:
        repo = hg.peer(ui, {}, repopath)
        yield repo
    except error.RepoError:
        yield None
    finally:
        if repo:
            repo.close()


@command("gc", [], _("hg gc"), optionalrepo=True)
def gc(ui, repo, *args, **opts):
    """garbage collect the client caches"""
    ui.warn(_("hg gc is no longer supported."))

    if not sysplatform.startswith("win"):
        cachepath = ui.config("remotefilelog", "cachepath")

        if cachepath:
            command = "`rm -rf {}/*`".format(cachepath)

            ui.warn(
                _(
                    """
To reclaim space from the hgcache directory, run:

%s

NOTE: The hgcache should manage its size itself. You should only run the command
above if you are completely out of space and quickly need to reclaim some space
temporarily. This will affect other users if you run this command on a shared machine.
"""
                )
                % command
            )


def log(orig, ui, repo, *pats, **opts):
    if shallowrepo.requirement not in repo.requirements:
        return orig(ui, repo, *pats, **opts)

    follow = opts.get("follow")
    revs = opts.get("rev")
    if pats:
        # Force slowpath for non-follow patterns and follows that start from
        # non-working-copy-parent revs.
        if not follow or revs:
            # This forces the slowpath
            opts["removed"] = True

        # If this is a non-follow log without any revs specified, recommend that
        # the user add -f to speed it up.
        if not follow and not revs:
            match, pats = scmutil.matchandpats(repo["."], pats, opts)
            isfile = not match.anypats()
            if isfile:
                for file in match.files():
                    if not os.path.isfile(repo.wjoin(file)):
                        isfile = False
                        break

            if isfile:
                ui.warn(
                    _(
                        "warning: file log can be slow on large repos - "
                        + "use -f to speed it up\n"
                    )
                )

    return orig(ui, repo, *pats, **opts)


def revdatelimit(ui, revset):
    """Update revset so that only changesets no older than 'prefetchdays' days
    are included. The default value is set to 14 days. If 'prefetchdays' is set
    to zero or negative value then date restriction is not applied.
    """
    days = ui.configint("remotefilelog", "prefetchdays", 14)
    if days > 0:
        revset = "(%s) & date(-%s)" % (revset, days)
    return revset


def readytofetch(repo):
    """Check that enough time has passed since the last background prefetch.
    This only relates to prefetches after operations that change the working
    copy parent. Default delay between background prefetches is 2 minutes.
    """
    timeout = repo.ui.configint("remotefilelog", "prefetchdelay", 120)
    fname = repo.localvfs.join("lastprefetch")

    ready = False
    with util.posixfile(fname, "a"):
        # the with construct above is used to avoid race conditions
        modtime = os.path.getmtime(fname)
        if (time.time() - modtime) > timeout:
            os.utime(fname, None)
            ready = True

    return ready


def pull(orig, ui, repo, *pats, **opts):
    result = orig(ui, repo, *pats, **opts)

    if shallowrepo.requirement in repo.requirements:
        # prefetch if it's configured
        prefetchrevset = ui.config("remotefilelog", "pullprefetch", None)
        bgrepack = repo.ui.configbool("remotefilelog", "backgroundrepack", False)

        if prefetchrevset:
            ui.status(_("prefetching file contents\n"))
            revs = scmutil.revrange(repo, [prefetchrevset])
            base = repo["."].rev()
            repo.prefetch(revs, base=base)
            if bgrepack:
                repackmod.domaintenancerepack(repo)
        elif bgrepack:
            repackmod.domaintenancerepack(repo)

    return result


def exchangepull(orig, repo, remote, *args, **kwargs):
    # Hook into the callstream/getbundle to insert bundle capabilities
    # during a pull.
    def localgetbundle(
        orig, source, heads=None, common=None, bundlecaps=None, **kwargs
    ):
        if not bundlecaps:
            bundlecaps = set()
        bundlecaps.add("remotefilelog")
        return orig(source, heads=heads, common=common, bundlecaps=bundlecaps, **kwargs)

    if util.safehasattr(remote, "_callstream"):
        remote._localrepo = repo
    elif util.safehasattr(remote, "getbundle"):
        wrapfunction(remote, "getbundle", localgetbundle)

    return orig(repo, remote, *args, **kwargs)


def revert(orig, ui, repo, ctx, parents, *pats, **opts):
    # prefetch prior to reverting
    # used for old mercurial version
    if shallowrepo.requirement in repo.requirements:
        files = []
        m = scmutil.match(ctx, pats, opts)
        mf = ctx.manifest()
        m.bad = lambda x, y: False
        for path in ctx.walk(m):
            files.append((path, hex(mf[path])))
        repo.fileservice.prefetch(files)

    return orig(ui, repo, ctx, parents, *pats, **opts)


def _revertprefetch(orig, repo, ctx, *files):
    # prefetch data that needs to be reverted
    # used for new mercurial version
    if shallowrepo.requirement in repo.requirements:
        allfiles = []
        mf = ctx.manifest()
        sparsematch = repo.maybesparsematch(ctx.rev())
        for f in files:
            for path in f:
                if (not sparsematch or sparsematch(path)) and path in mf:
                    allfiles.append((path, hex(mf[path])))
        repo.fileservice.prefetch(allfiles)
    return orig(repo, ctx, *files)


@command(
    "debugremotefilelog",
    [("d", "decompress", None, _("decompress the filelog first"))],
    _("hg debugremotefilelog <path>"),
    norepo=True,
)
def debugremotefilelog(ui, path, **opts):
    return debugcommands.debugremotefilelog(ui, path, **opts)


@command(
    "verifyremotefilelog",
    [("d", "decompress", None, _("decompress the filelogs first"))],
    _("hg verifyremotefilelogs <directory>"),
    norepo=True,
)
def verifyremotefilelog(ui, path, **opts):
    return debugcommands.verifyremotefilelog(ui, path, **opts)


@command(
    "debugdatapack",
    [
        ("", "long", None, _("print the long hashes")),
        ("", "node", "", _("dump the contents of node"), "NODE"),
        ("", "node-delta", "", _("dump the delta chain info of node"), "NODE"),
    ],
    _("hg debugdatapack <paths>"),
    norepo=True,
)
def debugdatapack(ui, *paths, **opts):
    return debugcommands.debugdatapack(ui, *paths, **opts)


@command(
    "debugindexedlogdatastore",
    [
        ("", "long", None, _("print the long hashes")),
        ("", "node", "", _("dump the contents of node"), "NODE"),
        ("", "node-delta", "", _("dump the delta chain info of node"), "NODE"),
    ],
    _("hg debugindexedlogdatastore <paths>"),
    norepo=True,
)
def debugindexedlogdatastore(ui, *paths, **opts):
    return debugcommands.debugindexedlogdatastore(ui, *paths, **opts)


@command(
    "debughistorypack",
    [("", "long", None, _("print the long hashes"))],
    _("hg debughistorypack <path>"),
    norepo=True,
)
def debughistorypack(ui, *paths, **opts):
    return debugcommands.debughistorypack(ui, paths, **opts)


@command(
    "debugindexedloghistorystore",
    [("", "long", None, _("print the long hashes"))],
    _("hg debugindexedloghistorystore <path>"),
    norepo=True,
)
def debugindexedloghistorystore(ui, *paths, **opts):
    return debugcommands.debugindexedloghistorystore(ui, paths, **opts)


@command("debugwaitonrepack", [], _("hg debugwaitonrepack"))
def debugwaitonrepack(ui, repo, **opts):
    return debugcommands.debugwaitonrepack(repo)


@command("debugwaitonprefetch", [], _("hg debugwaitonprefetch"))
def debugwaitonprefetch(ui, repo, **opts):
    return debugcommands.debugwaitonprefetch(repo)


def resolveprefetchopts(ui, opts):
    if not opts.get("rev"):
        revset = [".", "draft()"]

        prefetchrevset = ui.config("remotefilelog", "pullprefetch", None)
        if prefetchrevset:
            revset.append("(%s)" % prefetchrevset)
        revset = "+".join(revset)

        # update a revset with a date limit
        revset = revdatelimit(ui, revset)

        opts["rev"] = [revset]

    if not opts.get("base"):
        opts["base"] = None

    return opts


@command(
    "prefetch",
    [
        ("r", "rev", [], _("prefetch the specified revisions"), _("REV")),
        ("", "repack", False, _("run repack after prefetch")),
        ("b", "base", "", _("rev that is assumed to already be local")),
    ]
    + commands.walkopts,
    _("hg prefetch [OPTIONS] [FILE...]"),
)
def prefetch(ui, repo, *pats, **opts):
    """prefetch file revisions from the server

    Prefetchs file revisions for the specified revs and stores them in the
    local remotefilelog cache.  If no rev is specified, the default rev is
    used which is the union of dot, draft, and pullprefetch.
    File names or patterns can be used to limit which files are downloaded.

    Return 0 on success.
    """
    if not shallowrepo.requirement in repo.requirements:
        raise error.Abort(_("repo is not shallow"))
    fullrepo = not (pats or opts.get("include") or opts.get("exclude"))
    if "eden" in repo.requirements and fullrepo:
        raise error.Abort(
            _("`hg prefetch` must be called with paths in an EdenFS repository!"),
            hint="Specify exact paths you want to fetch i.e. run `hg prefetch DIR/**`",
        )

    opts = resolveprefetchopts(ui, opts)
    matcher = scmutil.match(repo[None], pats, opts)
    revs = scmutil.revrange(repo, opts.get("rev"))
    repo.prefetch(revs, opts.get("base"), matcher=matcher)

    # Run repack in background
    if opts.get("repack"):
        repackmod.domaintenancerepack(repo)


@command(
    "repack",
    [
        ("", "background", None, _("run in a background process"), None),
        ("", "incremental", None, _("do an incremental repack"), None),
    ],
    _("hg repack [OPTIONS]"),
)
def repack(ui, repo, *pats, **opts):
    if opts.get("background"):
        repackmod.backgroundrepack(repo, incremental=opts.get("incremental"))
        return

    try:
        if opts.get("incremental"):
            repackmod.incrementalrepack(repo)
        else:
            repackmod.fullrepack(repo)
    except repackmod.RepackAlreadyRunning as ex:
        # Don't propogate the exception if the repack is already in
        # progress, since we want the command to exit 0.
        repo.ui.warn("%s\n" % ex)
