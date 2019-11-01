# Copyright (c) Facebook, Inc. and its affiliates.
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

    ``remotefilelog.bgprefetchrevs`` specifies revisions to fetch on commit and

    update, and on other commands that use them. Different from pullprefetch.

    ``remotefilelog.gcrepack`` does garbage collection during repack when True

    ``remotefilelog.nodettl`` specifies maximum TTL of a node in seconds before
    it is garbage collected

    ``remotefilelog.localdatarepack`` runs repack on local data loose files

    ``remotefilelog.getfilesstep`` the number of files per batch during fetching

    ``remotefilelog.repackonhggc`` runs repack on hg gc when True

    ``remotefilelog.prefetchdays`` specifies the maximum age of a commit in
    days after which it is no longer prefetched.

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

    ``remotefilelog.cacheprocess2`` name of the cache client.

    ``remotefilelog.cachekey`` cache key prefix to use.

Configs for Eden API (HTTP data fetching):

    ``edenapi.enabled`` specifies whether HTTP data fetching should be used.

    ``edenapi.debug`` enables debug output for HTTP data fetching.

    ``edenapi.url`` specifies the base URL of the API server.

    ``edenapi.databatchsize`` specifies the batch size for file data requests
    for backends that support batching.

    ``edenapi.historybatchsize`` specifies the batch size for history requests
    for backends that support batching.

    ``edenapi.maxretries`` specifies the maximum number of times the client
    should retry failed HTTP requests.

    ``edenapi.validate`` specifies that the client should recompute and
    validate the node hash for data fetched over HTTP.

    ``edenapi.streamdata`` specifies that the client should request a
    streaming response for data fetches

    ``edenapi.streamhistory`` specifies that the client should request a
    streaming response for history fetches

    ``edenapi.streamtrees`` specifies that the client should request a
    streaming response for tree fetches

    ``edenapi.authhelp`` specifies the error message that will be
    printed out if there is an issue with the user's configured TLS
    client certificate. It is often desirable to configure a custom
    error message that explains to the user how to obtain valid TLS
    client credentials.

    ``edenapi.tlshelp`` specifies the error message that will be
    printed out when HTTP data fetching encounters a TLS error.

Eden API TLS credentials are configured using the auth section:

    ``auth.edenapi.prefix``: base URL (without scheme) for which to set credentials.

    ``auth.edenapi.schemes``: URL scheme to match; should usually be "https".

    ``auth.edenapi.cert``: client certificate for TLS mutual authenticaton.

    ``auth.edenapi.key``: client key for TLS mutual authentication.
"""
from __future__ import absolute_import

import os
import time
import traceback
from contextlib import contextmanager

from edenscm.mercurial import (
    archival,
    bundle2,
    changegroup,
    changelog,
    cmdutil,
    commands,
    context,
    copies,
    dirstate,
    dispatch,
    error,
    exchange,
    extensions,
    hg,
    localrepo,
    match,
    merge,
    patch,
    progress,
    registrar,
    repair,
    repoview,
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
from edenscm.mercurial.node import hex, nullrev

from . import (
    debugcommands,
    edenapi,
    fileserverclient,
    remotefilectx,
    remotefilelog,
    remotefilelogserver,
    repack as repackmod,
    shallowbundle,
    shallowrepo,
    shallowstore,
    shallowutil,
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

# Config items for HTTP data fetching.
configitem("edenapi", "enabled", default=False)
configitem("edenapi", "debug", default=False)
configitem("edenapi", "url", default=None)
configitem("edenapi", "databatchsize", default=None)
configitem("edenapi", "historybatchsize", default=None)
configitem("edenapi", "maxretries", default=3)
configitem("edenapi", "validate", default=True)
configitem("edenapi", "streamdata", default=False)
configitem("edenapi", "streamhistory", default=False)
configitem("edenapi", "streamtrees", default=False)
configitem("edenapi", "authhelp", default=None)
configitem("edenapi", "tlshelp", default=None)

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

    entry = extensions.wrapcommand(commands.table, "clone", cloneshallow)
    entry[1].append(
        (
            "",
            "shallow",
            None,
            _("create a shallow clone which uses remote file " "history"),
        )
    )

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
            lfsmod.wrapfilelog(remotefilelog.remotefilelog)
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


def cloneshallow(orig, ui, repo, *args, **opts):
    if opts.get("shallow"):
        repos = []

        def pull_shallow(orig, self, *args, **kwargs):
            if shallowrepo.requirement not in self.requirements:
                repos.append(self.unfiltered())
                # set up the client hooks so the post-clone update works
                setupclient(self.ui, self.unfiltered())

                # setupclient fixed the class on the repo itself
                # but we also need to fix it on the repoview
                if isinstance(self, repoview.repoview):
                    self.__class__.__bases__ = (
                        self.__class__.__bases__[0],
                        self.unfiltered().__class__,
                    )
                self.requirements.add(shallowrepo.requirement)
                self._writerequirements()

                # Since setupclient hadn't been called, exchange.pull was not
                # wrapped. So we need to manually invoke our version of it.
                return exchangepull(orig, self, *args, **kwargs)
            else:
                return orig(self, *args, **kwargs)

        wrapfunction(exchange, "pull", pull_shallow)

        # Wrap the stream logic to add requirements and to pass include/exclude
        # patterns around.
        def setup_streamout(repo, remote):
            # Replace remote.stream_out with a version that sends file
            # patterns.
            def stream_out_shallow(orig):
                caps = shallowutil.peercapabilities(remote)
                if shallowrepo.requirement in caps:
                    opts = {}
                    if repo.includepattern:
                        opts["includepattern"] = "\0".join(repo.includepattern)
                    if repo.excludepattern:
                        opts["excludepattern"] = "\0".join(repo.excludepattern)
                    if repo.ui.configbool("treemanifest", "treeonly"):
                        opts["noflatmanifest"] = "True"
                    return remote._callstream("stream_out_shallow", **opts)
                else:
                    return orig()

            wrapfunction(remote, "stream_out", stream_out_shallow)

        if hasstreamclone:

            def stream_wrap(orig, op):
                setup_streamout(op.repo, op.remote)
                return orig(op)

            wrapfunction(streamclone, "maybeperformlegacystreamclone", stream_wrap)

            def canperformstreamclone(orig, *args, **kwargs):
                supported, requirements = orig(*args, **kwargs)
                if requirements is not None:
                    requirements.add(shallowrepo.requirement)
                return supported, requirements

            wrapfunction(streamclone, "canperformstreamclone", canperformstreamclone)
        else:

            def stream_in_shallow(orig, repo, remote, requirements):
                setup_streamout(repo, remote)
                requirements.add(shallowrepo.requirement)
                return orig(repo, remote, requirements)

            wrapfunction(localrepo.localrepository, "stream_in", stream_in_shallow)

    orig(ui, repo, *args, **opts)


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

    # put here intentionally bc doesnt work in uisetup
    ui.setconfig("hooks", "update.prefetch", wcpprefetch)
    ui.setconfig("hooks", "commit.prefetch", wcpprefetch)

    isserverenabled = ui.configbool("remotefilelog", "server")
    isshallowclient = shallowrepo.requirement in repo.requirements

    if isserverenabled and isshallowclient:
        raise RuntimeError("Cannot be both a server and shallow client.")

    if isshallowclient:
        setupclient(ui, repo)

    if isserverenabled:
        remotefilelogserver.onetimesetup(ui)


def setupclient(ui, repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    # Even clients get the server setup since they need to have the
    # wireprotocol endpoints registered.
    remotefilelogserver.onetimesetup(ui)
    onetimeclientsetup(ui)

    shallowrepo.wraprepo(repo)
    repo.store = shallowstore.wrapstore(repo.store)

    if edenapi.enabled(ui):
        try:
            repo.edenapi = edenapi.pyclient(ui, repo)
        except Exception as e:
            ui.warn(_("failed to initialize Eden API client;"))
            ui.warn(_(" disabling HTTPS data fetching\n"))
            edenapi.logexception(ui, e)
            edenapi._disabled = True


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

    def storewrapper(orig, requirements, path, vfstype):
        s = orig(requirements, path, vfstype)
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
            repo.fileservice.prefetch(files)
        return orig(
            repo, actions, wctx, mctx, overwrite, labels=labels, ancestors=ancestors
        )

    wrapfunction(merge, "applyupdates", applyupdates)

    # Prefetch merge checkunknownfiles
    def checkunknownfiles(orig, repo, wctx, mctx, force, actions, *args, **kwargs):
        if shallowrepo.requirement in repo.requirements:
            files = []
            sparsematch = repo.maybesparsematch(mctx.rev())
            for f, (m, actionargs, msg) in actions.iteritems():
                if sparsematch and not sparsematch(f):
                    continue
                if m in ("c", "dc", "cm"):
                    files.append((f, hex(mctx.filenode(f))))
                elif m == "dg":
                    f2 = actionargs[0]
                    files.append((f2, hex(mctx.filenode(f2))))
            # batch fetch the needed files from the server
            repo.fileservice.prefetch(files)
        return orig(repo, wctx, mctx, force, actions, *args, **kwargs)

    wrapfunction(merge, "_checkunknownfiles", checkunknownfiles)

    # Prefetch files before status attempts to look at their size and contents
    def checklookup(orig, self, wctx, files):
        repo = self._repo
        if shallowrepo.requirement in repo.requirements:
            prefetchfiles = []
            for parent in wctx._parents:
                for f in files:
                    if f in parent:
                        prefetchfiles.append((f, hex(parent.filenode(f))))
            # batch fetch the needed files from the server
            repo.fileservice.prefetch(prefetchfiles)
        return orig(self, wctx, files)

    wrapfunction(dirstate.dirstate, "_checklookup", checklookup)

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
        if isinstance(link, int):
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
            for oldargs in pendingfilecommits:
                log, rt, tr, link, p1, p2, n, fl, c, m = oldargs
                linknode = self.node(link)
                if linknode == node:
                    log.addrawrevision(rt, tr, linknode, p1, p2, n, fl, c, m)
                else:
                    raise error.ProgrammingError(
                        "pending multiple integer revisions are not supported"
                    )
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

    wrapfunction(changelog.changelog, "add", changelogadd)

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
        **kwargs
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
            **kwargs
        )

    wrapfunction(patch, "trydiff", trydiff)

    if util.safehasattr(cmdutil, "_revertprefetch"):
        wrapfunction(cmdutil, "_revertprefetch", _revertprefetch)
    else:
        wrapfunction(cmdutil, "revert", revert)


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

    return smartset.baseset([r for r in subset if r in s])


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
    """garbage collect the client caches
    """

    if not repo:
        ui.warn(_("hg gc needs to be called in a repo\n"))
    else:
        repackmod.incrementalrepack(repo)


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


def wcpprefetch(ui, repo, **kwargs):
    """Prefetches in background revisions specified by bgprefetchrevs revset.
    Does background repack if backgroundrepack flag is set in config.
    """
    shallow = shallowrepo.requirement in repo.requirements
    bgprefetchrevs = ui.config("remotefilelog", "bgprefetchrevs", None)
    isready = readytofetch(repo)

    if not (shallow and bgprefetchrevs and isready):
        return

    bgrepack = repo.ui.configbool("remotefilelog", "backgroundrepack", False)
    # update a revset with a date limit
    bgprefetchrevs = revdatelimit(ui, bgprefetchrevs)

    def anon():
        if util.safehasattr(repo, "ranprefetch") and repo.ranprefetch:
            return
        repo.ranprefetch = True
        repo.backgroundprefetch(bgprefetchrevs, repack=bgrepack)

    repo._afterlock(anon)


def pull(orig, ui, repo, *pats, **opts):
    result = orig(ui, repo, *pats, **opts)

    if shallowrepo.requirement in repo.requirements:
        # prefetch if it's configured
        prefetchrevset = ui.config("remotefilelog", "pullprefetch", None)
        bgrepack = repo.ui.configbool("remotefilelog", "backgroundrepack", False)
        bgprefetch = repo.ui.configbool("remotefilelog", "backgroundprefetch", False)

        if prefetchrevset:
            ui.status(_("prefetching file contents\n"))
            revs = scmutil.revrange(repo, [prefetchrevset])
            base = repo["."].rev()
            if bgprefetch:
                basestr = None if base == nullrev else str(base)
                repo.backgroundprefetch(prefetchrevset, base=basestr, repack=bgrepack)
            else:
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


@command("debuggetfiles|debuggetfile", [], _("hg debuggetfiles"))
def debuggetfiles(ui, repo, **opts):
    """download file content from the API server
    Read filenode/path pairs from stdin, fetch the content of each file
    from the API server, and write the results to a datapack.
    """
    return debugcommands.debuggetfiles(ui, repo, **opts)


@command("debugserialgetfiles", [], _("hg debugserialgetfiles"))
def debugserialgetfiles(ui, repo, **opts):
    """serially download file content from the API server
    Read filenode/path pairs from stdin, fetch the content of each file
    from the API server one at a time. This is useful for testing the
    performance of serial fetching, which should ordinarily be avoided.
    """
    return debugcommands.debugserialgetfiles(ui, repo, **opts)


@command(
    "debuggethistory",
    [("d", "depth", 0, _("how many history entries to fetch per file"), _("DEPTH"))],
    _("hg debuggethistory"),
)
def debuggethistory(ui, repo, **opts):
    """download file history from the API server
    Read filenode/path pairs from stdin, fetch the history of each file
    from the API server, and write the results to a historypack.
    """
    return debugcommands.debuggethistory(ui, repo, **opts)


@command("debuggettrees", [], _("hg debuggettrees"))
def debuggettrees(ui, repo, **opts):
    """download tree content from the API server
    Read manifestnode/path pairs from stdin, fetch the content of each tree
    from the API server, and write the results to a datapack.
    """
    return debugcommands.debuggettrees(ui, repo, **opts)


def resolveprefetchopts(ui, opts):
    if not opts.get("rev"):
        revset = [".", "draft()"]

        prefetchrevset = ui.config("remotefilelog", "pullprefetch", None)
        if prefetchrevset:
            revset.append("(%s)" % prefetchrevset)
        bgprefetchrevs = ui.config("remotefilelog", "bgprefetchrevs", None)
        if bgprefetchrevs:
            revset.append("(%s)" % bgprefetchrevs)
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
    used which is the union of dot, draft, pullprefetch and bgprefetchrev.
    File names or patterns can be used to limit which files are downloaded.

    Return 0 on success.
    """
    if not shallowrepo.requirement in repo.requirements:
        raise error.Abort(_("repo is not shallow"))

    opts = resolveprefetchopts(ui, opts)
    revs = scmutil.revrange(repo, opts.get("rev"))
    repo.prefetch(revs, opts.get("base"), pats, opts)

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
