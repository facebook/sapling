# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
treemanifest extension is to aid in the transition from flat manifests to
treemanifests. It has a client portion that's used to construct trees during
client pulls and commits, and a server portion which is used to generate
tree manifests side-by-side normal flat manifests.

Configs:

    ``treemanifest.server`` is used to indicate that this repo can serve
    treemanifests

allows using and migrating to tree manifests

When autocreatetrees is enabled, you can limit which bookmarks are initially
converted to trees during pull by specifying `treemanifest.allowedtreeroots`.

::

    [treemanifest]
    allowedtreeroots = master,stable

Disabling `treemanifest.demanddownload` will prevent the extension from
automatically downloading trees from the server when they don't exist locally.

::

    [treemanifest]
    demanddownload = True

Disabling `treemanifest.demandgenerate` will prevent the extension from
automatically generating tree manifest from corresponding flat manifest when
it doesn't exist locally. Note that this setting is only relevant in treeonly
mode.

::

    [treemanifest]
    demandgenerate = True

Setting `treemanifest.pullprefetchcount` to an integer N will cause the latest N
commits' manifests to be downloaded (if they aren't already).

::

    [treemanifest]
    pullprefetchcount = 0

`treemanifest.pullprefetchrevs` specifies a revset of commits who's trees should
be prefetched after a pull. Defaults to None.

::

    [treemanifest]
    pullprefetchrevs = master + stable

Setting `treemanifest.repackstartrev` and `treemanifest.repackendrev` causes `hg
repack --incremental` to only repack the revlog entries in the given range. The
default values are 0 and len(changelog) - 1, respectively.

::

    [treemanifest]
    repackstartrev = 0
    repackendrev = 1000

Setting `treemanifest.treeonly` to True will force all manifest reads to use the
tree format. This is useful in the final stages of a migration to treemanifest
to prevent accesses of flat manifests.

::

    [treemanifest]
    treeonly = True

`treemanifest.simplecacheserverstore` causes the treemanifest server to store a cache
of treemanifest revisions in simplecache. This is a replacement for treemanifest.cacheserverstore
Simplecache can be configured to use memcache as a store or a local disk.

::

    [treemanifest]
    simplecacheserverstore = True

`treemanifest.cacheserverstore` causes the treemanifest server to store a cache
of treemanifest revisions in individual files. These improve lookup speed since
we don't have to open a revlog.

::

    [treemanifest]
    cacheserverstore = True

`treemanifest.servermaxcachesize` the maximum number of entries in the server
cache. Not used for treemanifest.simplecacheserverstore.

::

    [treemanifest]
    servermaxcachesize = 1000000

`treemanifest.servercacheevictionpercent` the percent of the cache to evict
when the maximum size is hit. Not used for treemanifest.simplecacheserverstore.

::

    [treemanifest]
    servercacheevictionpercent = 50

`treemanifest.fetchdepth` sets the default depth to fetch trees when fetching
trees from the server.

::

    [treemanifest]
    fetchdepth = 65536

`treemanifest.prefetchdraftparents` causes treemanifest to prefetch the parent
trees for new draft roots added to the repository.

::

    [treemanifest]
    prefetchdraftparents = True

`treemanifest.bfsprefetch` causes the client to perform a BFS over the
tree to be prefetched and manually request all missing nodes from the
server, rather than relying on the server to perform this computation.

::

    [treemanifest]
    bfsprefetch = True

`treemanifest.http` causes treemanifest to fetch tress over HTTP using EdenAPI.

::

    [treemanifest]
    http = True
"""
from __future__ import absolute_import

import abc
import contextlib
import hashlib
import os
import random
import struct

from bindings import manifest as rustmanifest, revisionstore
from edenscm.hgext import clienttelemetry
from edenscm.hgext.remotefilelog import (
    cmdtable as remotefilelogcmdtable,
    mutablestores,
    resolveprefetchopts,
    shallowbundle,
    shallowrepo,
    shallowutil,
    wirepack,
)
from edenscm.hgext.remotefilelog.contentstore import (
    manifestrevlogstore,
    unioncontentstore,
)
from edenscm.hgext.remotefilelog.datapack import makedatapackstore, memdatapack
from edenscm.hgext.remotefilelog.historypack import makehistorypackstore, memhistorypack
from edenscm.hgext.remotefilelog.metadatastore import unionmetadatastore
from edenscm.hgext.remotefilelog.repack import domaintenancerepack
from edenscm.mercurial import (
    bundle2,
    bundlerepo,
    changegroup,
    changelog2,
    commands,
    error,
    exchange,
    extensions,
    hg,
    localrepo,
    manifest,
    mdiff,
    perftrace,
    phases,
    progress,
    pycompat,
    registrar,
    repair,
    revlog,
    revsetlang,
    scmutil,
    sshserver,
    templatekw,
    util,
    wireproto,
)
from edenscm.mercurial.commands import debug as debugcommands
from edenscm.mercurial.i18n import _, _n
from edenscm.mercurial.node import bin, hex, nullid, short
from edenscm.mercurial.pycompat import range


try:
    from itertools import zip_longest
except ImportError:
    # pyre-fixme[21]: Could not find name `izip_longest` in `itertools`.
    from itertools import izip_longest as zip_longest


cmdtable = {}
command = registrar.command(cmdtable)

# The default depth to fetch during tree fetches
TREE_DEPTH_MAX = 2**16

configtable = {}
configitem = registrar.configitem(configtable)

configitem("treemanifest", "sendtrees", default=False)
configitem("treemanifest", "server", default=False)
configitem("treemanifest", "simplecacheserverstore", default=False)
configitem("treemanifest", "cacheserverstore", default=True)
configitem("treemanifest", "servermaxcachesize", default=1000000)
configitem("treemanifest", "servercacheevictionpercent", default=50)
configitem("treemanifest", "fetchdepth", default=TREE_DEPTH_MAX)
configitem("treemanifest", "stickypushpath", default=True)
configitem("treemanifest", "treeonly", default=True)
configitem("treemanifest", "prefetchdraftparents", default=True)
configitem("treemanifest", "useruststore", default=True)
configitem("treemanifest", "http", default=False)

PACK_CATEGORY = "manifests"

TREEGROUP_PARTTYPE = "b2x:treegroup"
# Temporary part type while we migrate the arguments
TREEGROUP_PARTTYPE2 = "b2x:treegroup2"
RECEIVEDNODE_RECORD = "receivednodes"

# When looking for a recent manifest to consider our base during tree
# prefetches, this constant defines how far back we should search.
BASENODESEARCHMAX = 25000


def treeenabled(ui):
    for name in ["treemanifest", "treemanifestserver"]:
        if (
            ui.config("extensions", name) not in (None, "!")
            or name in extensions.DEFAULT_EXTENSIONS
            or name in extensions.ALWAYS_ON_EXTENSIONS
        ):
            return True
    return False


def useruststore(ui):
    return ui.configbool("treemanifest", "useruststore") and not ui.configbool(
        "treemanifest", "server"
    )


def usehttpfetching(repo):
    """Returns True if HTTP (EdenApi) fetching should be used."""
    if repo.ui.config("ui", "ssh") == "false":
        # Cannot use SSH.
        return True
    return (
        repo.ui.configbool("treemanifest", "http")
        and getattr(repo, "edenapi", None) is not None
    )


def hgupdate(orig, repo, node, quietempty=False, updatecheck=None):
    oldfallbackpath = getattr(repo, "fallbackpath", None)
    if util.safehasattr(repo, "stickypushpath"):
        repo.fallbackpath = repo.stickypushpath

    try:
        return orig(repo, node, quietempty, updatecheck)
    finally:
        repo.fallbackpath = oldfallbackpath


def expush(orig, repo, remote, *args, **kwargs):
    if repo.ui.configbool(
        "treemanifest", "stickypushpath", True
    ) and "gettreepack" in shallowutil.peercapabilities(remote):
        # In case of pushrebase using paths.default-push, the pushback bundle
        # part does not contain trees. The client might prefetch trees from
        # paths.default, which could be lagging and cause tree prefetch to
        # fail. Set fallbackpath explicitly so the client pulls from the same
        # server as it pushes to.
        #
        # This assumes the server supporting pushrebase also supports
        # treemanifest, which is true for now.
        repo.stickypushpath = remote.url()

    return orig(repo, remote, *args, **kwargs)


def uisetup(ui):
    extensions.wrapfunction(exchange, "push", expush)
    extensions.wrapfunction(hg, "update", hgupdate)

    extensions.wrapfunction(
        changegroup.cg1unpacker, "_unpackmanifests", _unpackmanifestscg1
    )
    extensions.wrapfunction(
        changegroup.cg3unpacker, "_unpackmanifests", _unpackmanifestscg3
    )
    extensions.wrapfunction(
        exchange, "_pullbundle2extraprepare", pullbundle2extraprepare
    )
    extensions.wrapfunction(revlog.revlog, "checkhash", _checkhash)

    extensions.wrapfilecache(localrepo.localrepository, "manifestlog", getmanifestlog)
    extensions.wrapfilecache(
        bundlerepo.bundlerepository, "manifestlog", getbundlemanifestlog
    )

    extensions.wrapfunction(manifest.memmanifestctx, "write", _writemanifestwrapper)

    extensions.wrapcommand(commands.table, "pull", pull)

    wireproto.commands["gettreepack"] = (servergettreepack, "*")
    wireproto.wirepeer.gettreepack = clientgettreepack
    localrepo.localpeer.gettreepack = localgettreepack

    extensions.wrapfunction(
        debugcommands, "_findtreemanifest", _debugcmdfindtreemanifest
    )
    extensions.wrapfunction(debugcommands, "_debugbundle2part", _debugbundle2part)
    extensions.wrapfunction(repair, "_collectfiles", collectfiles)
    extensions.wrapfunction(repair, "striptrees", striptrees)
    extensions.wrapfunction(repair, "_collectmanifest", _collectmanifest)
    extensions.wrapfunction(repair, "stripmanifest", stripmanifest)
    extensions.wrapfunction(bundle2, "_addpartsfromopts", _addpartsfromopts)
    extensions.wrapfunction(
        bundlerepo.bundlerepository, "_handlebundle2part", _handlebundle2part
    )
    extensions.wrapfunction(bundle2, "getrepocaps", getrepocaps)
    _registerbundle2parts()

    extensions.wrapfunction(templatekw, "showmanifest", showmanifest)
    templatekw.keywords["manifest"] = templatekw.showmanifest

    # Change manifest template output
    templatekw.defaulttempl["manifest"] = "{node}"

    def _wrapremotefilelog(loaded):
        if loaded:
            remotefilelogmod = extensions.find("remotefilelog")
            extensions.wrapcommand(
                remotefilelogmod.cmdtable, "prefetch", _prefetchwrapper
            )
        else:
            # There is no prefetch command to wrap around. In this case, we use
            # the command table entry for prefetch in the remotefilelog to
            # define the prefetch command, wrap it, and then override it
            # completely.  This ensures that the options to the prefetch command
            # are consistent.
            cmdtable["prefetch"] = remotefilelogcmdtable["prefetch"]
            extensions.wrapcommand(cmdtable, "prefetch", _overrideprefetch)

    extensions.afterloaded("remotefilelog", _wrapremotefilelog)

    # Work around the chicken-egg issue that a linkrev brings by delaying adding
    # data to the store until the changelog has been updated. This breaks the
    # assumption that manifests are written before the changelog, but unless
    # linknode are moved outside of the historypack entries, we have to solve
    # the dependency loop.
    # A similar hack is done in remotefilelog for the same reasons.
    pendingadd = []

    def addtreeentry(
        orig, self, dpack, hpack, nname, nnode, ntext, np1, np2, linknode, linkrev=None
    ):
        if linkrev is not None:
            pendingadd.append(
                (self, dpack, hpack, nname, nnode, ntext, np1, np2, linkrev)
            )
        else:
            orig(self, dpack, hpack, nname, nnode, ntext, np1, np2, linknode)

    extensions.wrapfunction(basetreemanifestlog, "_addtreeentry", addtreeentry)

    def changelogadd(orig, self, *args):
        oldlen = len(self)
        node = orig(self, *args)
        newlen = len(self)
        if oldlen != newlen:
            for oldargs in pendingadd:
                log, dpack, hpack, nname, nnode, ntext, np1, np2, linkrev = oldargs
                log._addtreeentry(dpack, hpack, nname, nnode, ntext, np1, np2, node)
        else:
            # Nothing was added to the changelog, let's make sure that we don't
            # have pending adds.
            if len(set(x[8] for x in pendingadd)) > 1:
                raise error.ProgrammingError(
                    "manifest entries were added, but no matching revisions were"
                )

        del pendingadd[:]
        return node

    extensions.wrapfunction(changelog2.changelog, "add", changelogadd)


def showmanifest(orig, **args):
    """Same implementation as the upstream showmanifest, but without the 'rev'
    field."""
    ctx, templ = args[r"ctx"], args[r"templ"]
    mnode = ctx.manifestnode()
    if mnode is None:
        # just avoid crash, we might want to use the 'ff...' hash in future
        return

    mhex = hex(mnode)
    args = args.copy()
    args.update({r"node": mhex})
    f = templ("manifest", **args)
    return templatekw._mappable(f, None, f, lambda x: {"node": mhex})


def getrepocaps(orig, repo, *args, **kwargs):
    caps = orig(repo, *args, **kwargs)
    if treeenabled(repo.ui):
        caps["treemanifest"] = ("True",)
        if repo.ui.configbool("treemanifest", "treeonly"):
            caps["treeonly"] = ("True",)
        if repo.svfs.treemanifestserver:
            caps["treemanifestserver"] = ("True",)
    return caps


def _collectmanifest(orig, repo, striprev):
    if treeenabled(repo.ui) and repo.ui.configbool("treemanifest", "treeonly"):
        return []
    return orig(repo, striprev)


def stripmanifest(orig, repo, striprev, tr, files):
    if treeenabled(repo.ui) and repo.ui.configbool("treemanifest", "treeonly"):
        repair.striptrees(repo, tr, striprev, files)
        return
    orig(repo, striprev, tr, files)


def reposetup(ui, repo):
    # Update "{manifest}" again since it might be rewritten by templatekw.init.
    templatekw.defaulttempl["manifest"] = "{node}"

    if not isinstance(repo, localrepo.localrepository):
        return

    repo.svfs.treemanifestserver = repo.ui.configbool("treemanifest", "server")
    if repo.svfs.treemanifestserver:
        serverreposetup(repo)
    else:
        clientreposetup(repo)

    wraprepo(repo)


def clientreposetup(repo):
    repo.name = repo.ui.config("remotefilelog", "reponame", "unknown")


def wraprepo(repo):
    class treerepository(repo.__class__):
        def transaction(self, *args, **kwargs):
            tr = super(treerepository, self).transaction(*args, **kwargs)
            tr.addpostclose("draftparenttreefetch", self._parenttreefetch)
            return tr

        def _shouldusehttp(self):
            """Returns True if HTTP fetching should be used."""
            return usehttpfetching(self)

        def _interactivedebug(self):
            """Returns True if this is an interactive command running in debug mode."""
            return self.ui.interactive() and self.ui.configbool(
                "remotefilelog", "debug"
            )

        def _parenttreefetch(self, tr):
            """Prefetches draft commit parents after draft commits are added to the
            repository. This is useful for avoiding expensive ondemand downloads when
            accessing a draft commit for which we have the draft trees but not the
            public trees."""
            if not self.ui.configbool("treemanifest", "prefetchdraftparents"):
                return

            # strip only logically removes commits, the added commits are
            # pointless and can have issues like invalidated revs. Do nothing
            # in this case.
            if self.ui.cmdname == "debugstrip" or "strip" in tr.desc:
                return

            nodes = tr.changes.get("nodes")
            if not nodes:
                return

            # If any draft commits were added, prefetch their public parents.
            # Note that shelve could've produced a hidden commit, so
            # we need an unfiltered repo to evaluate the revset
            try:
                revset = "parents(%ln & draft() - hidden()) & public()"
                draftparents = list(self.set(revset, nodes))

                if draftparents:
                    self.prefetchtrees([c.manifestnode() for c in draftparents])
            except Exception as ex:
                # Errors are not fatal.
                self.ui.warn(_("not prefetching trees for draft parents: %s\n") % ex)

        @perftrace.tracefunc("Prefetch Trees")
        def prefetchtrees(self, mfnodes, basemfnodes=None):
            if not treeenabled(self.ui):
                return

            mfnodes = list(mfnodes)
            perftrace.tracevalue("Keys", len(mfnodes))

            mfstore = self.manifestlog.datastore
            missingentries = mfstore.getmissing(("", n) for n in mfnodes)
            mfnodes = list(n for path, n in missingentries)
            perftrace.tracevalue("Missing", len(mfnodes))
            if not mfnodes:
                return

            if self.svfs.treemanifestserver:
                # The server has nowhere to fetch from, so this is an error and
                # we should throw. This can legitimately happen during the tree
                # transition if the server has trees for all of its commits but
                # it has to serve an infinitepush bundle that doesn't have trees.
                raise shallowutil.MissingNodesError(
                    (("", n) for n in mfnodes), "tree nodes missing on server"
                )

            if useruststore(self.ui):
                self.manifestlog.datastore.prefetch(
                    list(("", node) for node in mfnodes)
                )
            else:
                # If we have no base nodes, scan the changelog looking for a
                # semi-recent manifest node to treat as the base.
                if not basemfnodes:
                    basemfnodes = _findrecenttree(self, self["tip"].node(), mfnodes)

        def resettreefetches(self):
            fetches = self._treefetches
            self._treefetches = 0
            return fetches

        def _restrictcapabilities(self, caps):
            caps = super(treerepository, self)._restrictcapabilities(caps)
            caps = _addservercaps(self, caps)
            return caps

        def getdesignatednodes(self, keys):
            if self._interactivedebug():
                n = len(keys)
                (firstpath, firstnode) = keys[0]
                firstnode = hex(firstnode)
                self.ui.write_err(
                    _n(
                        "fetching tree for ('%(path)s', %(node)s)\n",
                        "fetching %(num)s trees\n",
                        n,
                    )
                    % {"path": firstpath, "node": firstnode, "num": n}
                )

            if self._shouldusehttp():
                return self._httpgetdesignatednodes(keys)
            else:
                return self._sshgetdesignatednodes(keys)

        @perftrace.tracefunc("SSH On-Demand Fetch Trees")
        def _sshgetdesignatednodes(self, keys):
            """
            Fetch the specified tree nodes over SSH.

            This method requires the server to support the "designatednodes"
            capability. This capability overloads the gettreepack wireprotocol
            command to allow the client to specify an exact set of tree nodes
            to fetch; the server will then provide only those nodes.

            Returns False if the server does not support "designatednodes",
            and True otherwise.
            """
            fallbackpath = getfallbackpath(self)
            with self.connectionpool.get(fallbackpath) as conn:
                if "designatednodes" not in conn.peer.capabilities():
                    raise error.ProgrammingError("designatednodes must be supported")

                mfnodes = [node for path, node in keys]
                directories = [path for path, node in keys]

                start = util.timer()
                with self.ui.timesection("getdesignatednodes"):
                    _gettrees(
                        self,
                        conn.peer,
                        "",
                        mfnodes,
                        [],
                        directories,
                        start,
                        depth=1,
                        ondemandfetch=True,
                    )

            return True

        @perftrace.tracefunc("HTTP On-Demand Fetch Trees")
        def _httpgetdesignatednodes(self, keys):
            dpack, _hpack = self.manifestlog.getmutablesharedpacks()
            self.edenapi.storetrees(dpack, keys)
            return True

        def forcebfsprefetch(self, rootdir, mfnodes, depth=None):
            self._bfsprefetch(rootdir, mfnodes, depth)

        @perftrace.tracefunc("BFS Prefetch")
        def _bfsprefetch(self, rootdir, mfnodes, depth=None):
            with progress.spinner(self.ui, "prefetching trees using BFS"):
                store = self.manifestlog.datastore
                for node in mfnodes:
                    if node != nullid:
                        rustmanifest.prefetch(store, node, rootdir, depth)

    repo.__class__ = treerepository
    repo._treefetches = 0


def _prunesharedpacks(repo, packpath):
    """Repack the packpath if it has too many packs in it"""
    try:
        numentries = len(os.listdir(packpath))
    except OSError:
        return

    # Note this is based on file count, not pack count.
    config = repo.ui.configint("packs", "maxpackfilecount")
    if config and numentries > config:
        domaintenancerepack(repo)


def setuptreestores(repo, mfl):
    ui = repo.ui
    if ui.configbool("treemanifest", "server"):
        packpath = repo.localvfs.join("cache/packs/%s" % PACK_CATEGORY)

        mutablelocalstore = mutablestores.mutabledatahistorystore(
            lambda: mfl._mutablelocalpacks
        )
        ondemandstore = ondemandtreedatastore(repo)

        # Data store
        datastore = makedatapackstore(ui, packpath, False)
        revlogstore = manifestrevlogstore(repo)
        mfl.revlogstore = revlogstore

        if ui.configbool("treemanifest", "cacheserverstore") and ui.configbool(
            "treemanifest", "simplecacheserverstore"
        ):
            raise error.Abort(
                "treemanifest.cacheserverstore and treemanifest.simplecacheserverstore can't be both enabled"
            )

        if ui.configbool("treemanifest", "cacheserverstore"):
            maxcachesize = ui.configint("treemanifest", "servermaxcachesize")
            evictionrate = ui.configint("treemanifest", "servercacheevictionpercent")
            revlogstore = vfscachestore(
                revlogstore, repo.cachevfs, maxcachesize, evictionrate
            )

        if ui.configbool("treemanifest", "simplecacheserverstore"):
            revlogstore = simplecachestore(ui, revlogstore)

        mfl.datastore = unioncontentstore(
            datastore, revlogstore, mutablelocalstore, ondemandstore
        )

        # History store
        historystore = makehistorypackstore(ui, packpath, False)
        mfl.historystore = unionmetadatastore(
            historystore, revlogstore, mutablelocalstore, ondemandstore
        )
        _prunesharedpacks(repo, packpath)
        ondemandstore.setshared(mfl.datastore, mfl.historystore)

        mfl.shareddatastores = [datastore, revlogstore]
        # Local stores are stores that contain data not on the main server
        mfl.localdatastores = []
        mfl.sharedhistorystores = [historystore, revlogstore]
        mfl.localhistorystores = []
        return

    if useruststore(ui):
        mfl.makeruststore()
        return

    if not util.safehasattr(repo, "name"):
        repo.name = ui.config("remotefilelog", "reponame", "unknown")
    packpath = shallowutil.getcachepackpath(repo, PACK_CATEGORY)
    _prunesharedpacks(repo, packpath)

    localpackpath = shallowutil.getlocalpackpath(repo.svfs.vfs.base, PACK_CATEGORY)

    demanddownload = ui.configbool("treemanifest", "demanddownload", True)
    demandgenerate = (
        ui.configbool("treemanifest", "treeonly")
        or ui.configbool("treemanifest", "sendtrees")
    ) and ui.configbool("treemanifest", "demandgenerate", True)
    remotestore = remotetreestore(repo)
    ondemandstore = ondemandtreedatastore(repo)

    mutablelocalstore = mutablestores.mutabledatahistorystore(
        lambda: mfl._mutablelocalpacks
    )
    mutablesharedstore = mutablestores.mutabledatahistorystore(
        lambda: mfl._mutablesharedpacks
    )

    # Data store
    datastore = makedatapackstore(ui, packpath, True, deletecorruptpacks=True)
    localdatastore = makedatapackstore(ui, localpackpath, False)
    datastores = [datastore, localdatastore, mutablelocalstore, mutablesharedstore]
    if demanddownload:
        datastores.append(remotestore)

    if demandgenerate:
        datastores.append(ondemandstore)

    mfl.datastore = unioncontentstore(*datastores)

    mfl.shareddatastores = [datastore]
    # Local stores are stores that contain data not on the main server
    mfl.localdatastores = [localdatastore]

    # History store
    sharedhistorystore = makehistorypackstore(
        ui, packpath, True, deletecorruptpacks=True
    )
    localhistorystore = makehistorypackstore(ui, localpackpath, False)
    mfl.sharedhistorystores = [sharedhistorystore]
    mfl.localhistorystores = [localhistorystore]

    histstores = [
        sharedhistorystore,
        localhistorystore,
        mutablelocalstore,
        mutablesharedstore,
    ]
    if demanddownload:
        histstores.append(remotestore)

    if demandgenerate:
        histstores.append(ondemandstore)

    mfl.historystore = unionmetadatastore(*histstores)
    shallowutil.reportpackmetrics(ui, "treestore", mfl.datastore, mfl.historystore)

    remotestore.setshared(mfl.datastore, mfl.historystore)
    ondemandstore.setshared(mfl.datastore, mfl.historystore)


class basetreemanifestlog(object):
    def __init__(self, repo):
        if not useruststore(self._repo.ui):
            self._mutablelocalpacks = mutablestores.pendingmutablepack(
                repo,
                lambda: shallowutil.getlocalpackpath(
                    self._opener.vfs.base, "manifests"
                ),
            )
            self._mutablesharedpacks = mutablestores.pendingmutablepack(
                repo, lambda: shallowutil.getcachepackpath(self._repo, PACK_CATEGORY)
            )
        self.recentlinknode = None
        cachesize = 4
        self._treemanifestcache = util.lrucachedict(cachesize)

    def add(
        self,
        ui,
        newtree,
        p1node,
        p2node,
        linknode,
        overridenode=None,
        overridep1node=None,
        tr=None,
        linkrev=None,
    ):
        """Writes the given tree into the manifestlog. If `overridenode` is
        specified, the tree root is written with that node instead of its actual
        node. If `overridep1node` is specified, the the p1 node for the root
        tree is also overridden.
        """
        if ui.configbool("treemanifest", "server"):
            return self._addtorevlog(
                ui,
                newtree,
                p1node,
                p2node,
                linknode,
                overridenode=overridenode,
                overridep1node=overridep1node,
                tr=tr,
                linkrev=linkrev,
            )
        else:
            return self._addtopack(
                ui,
                newtree,
                p1node,
                p2node,
                linknode,
                overridenode=overridenode,
                overridep1node=overridep1node,
                linkrev=linkrev,
            )

    def _getmutablelocalpacks(self):
        """Returns a tuple containing a data pack and a history pack."""
        if useruststore(self._repo.ui):
            return (self.datastore, self.historystore)
        else:
            return self._mutablelocalpacks.getmutablepack()

    def getmutablesharedpacks(self):
        if useruststore(self._repo.ui):
            return (
                self.datastore.getsharedmutable(),
                self.historystore.getsharedmutable(),
            )

        return self._mutablesharedpacks.getmutablepack()

    def _addtreeentry(
        self, dpack, hpack, nname, nnode, ntext, np1, np2, linknode, linkrev=None
    ):
        if linkrev is not None:
            raise error.ProgrammingError("linkrev cannot be added")
        # Not using deltas, since there aren't any other trees in
        # this pack it could delta against.
        dpack.add(nname, nnode, revlog.nullid, ntext)
        hpack.add(nname, nnode, np1, np2, linknode, "")

    def _addtopack(
        self,
        ui,
        newtree,
        p1node,
        p2node,
        linknode,
        overridenode=None,
        overridep1node=None,
        linkrev=None,
    ):
        dpack, hpack = self._getmutablelocalpacks()

        newtreeiter = _finalize(self, newtree, p1node, p2node)

        if overridenode is not None:
            dpack = InterceptedMutableDataPack(dpack, overridenode, overridep1node)
            hpack = InterceptedMutableHistoryPack(hpack, overridenode, overridep1node)

        node = overridenode
        for nname, nnode, ntext, _np1text, np1, np2 in newtreeiter:
            self._addtreeentry(
                dpack, hpack, nname, nnode, ntext, np1, np2, linknode, linkrev
            )
            if node is None and nname == "":
                node = nnode

        return node

    def _addtorevlog(
        self,
        ui,
        newtree,
        p1node,
        p2node,
        linknode,
        overridenode=None,
        overridep1node=None,
        tr=None,
        linkrev=None,
    ):
        if tr is None:
            raise error.ProgrammingError("missing transaction")
        if linkrev is None and linknode is None:
            raise error.ProgrammingError("missing linkrev or linknode")
        if overridep1node is not None and p1node != overridep1node:
            raise error.ProgrammingError(
                "overridep1node is not supported for " "revlogs"
            )

        if linkrev is None:
            linkrev = self._maplinknode(linknode)

        revlogstore = self.revlogstore
        node = overridenode
        newtreeiter = _finalize(self, newtree, p1node, p2node)
        for nname, nnode, ntext, _np1text, np1, np2 in newtreeiter:
            revlog = revlogstore._revlog(nname)
            override = None
            if nname == "":
                override = overridenode
            resultnode = revlog.addrevision(ntext, tr, linkrev, np1, np2, node=override)
            if node is None and nname == "":
                node = resultnode
            if (overridenode is None or nname != "") and resultnode != nnode:
                raise error.ProgrammingError(
                    "tree node mismatch - "
                    "Expected=%s ; Actual=%s" % (hex(nnode), hex(resultnode))
                )
        return node

    def commitsharedpacks(self):
        """Persist the dirty trees written to the shared packs."""

        self.datastore.markforrefresh()
        self.historystore.markforrefresh()
        if useruststore(self.ui):
            self.datastore.flush()
            self.historystore.flush()
        else:
            self._mutablesharedpacks.commit()

    def commitpending(self):
        if not useruststore(self._repo.ui):
            self._mutablelocalpacks.commit()
        self.commitsharedpacks()

    def abortpending(self):
        if not useruststore(self._repo.ui):
            self._mutablelocalpacks.abort()
        self.commitsharedpacks()

    def __nonzero__(self):
        return True

    __bool__ = __nonzero__

    def __getitem__(self, node):
        return self.get("", node)

    def get(self, dir, node, verify=True):
        if dir != "":
            raise RuntimeError(
                "native tree manifestlog doesn't support "
                "subdir reads: (%s, %s)" % (dir, hex(node))
            )
        if node == nullid:
            return treemanifestctx(self, dir, node)
        if node in self._treemanifestcache:
            return self._treemanifestcache[node]

        store = self.datastore

        try:
            store.get(dir, node)
        except KeyError:
            raise shallowutil.MissingNodesError([(dir, node)])

        m = treemanifestctx(self, dir, node)
        self._treemanifestcache[node] = m
        return m

    def edenapistore(self, repo):
        if usehttpfetching(repo):
            return repo.edenapi.treestore()
        return None

    def makeruststore(self):
        remotestore = revisionstore.pyremotestore(remotetreestore(self._repo))
        correlator = clienttelemetry.correlator(self._repo.ui)
        edenapistore = self.edenapistore(self._repo)

        mask = os.umask(0o002)
        try:
            # TODO(meyer): Is there a reason no edenapi is used here?
            # Do all reads fall back through _httpgetdesignatednodes instead
            # Of the normal contentstore fallback path?
            self.treescmstore = revisionstore.treescmstore(
                self._repo.svfs.vfs.base,
                self.ui._rcfg,
                remotestore,
                None,
                edenapistore,
                "manifests",
                correlator=correlator,
            )
            self.datastore = self.treescmstore.get_contentstore()
            self.historystore = revisionstore.metadatastore(
                self._repo.svfs.vfs.base,
                self.ui._rcfg,
                remotestore,
                None,
                None,
                "manifests",
            )
        finally:
            os.umask(mask)


class treemanifestlog(basetreemanifestlog, manifest.manifestlog):
    def __init__(self, opener, repo, treemanifest=False):
        self._repo = repo
        basetreemanifestlog.__init__(self, self._repo)
        assert treemanifest is False
        cachesize = 4

        self.ui = repo.ui

        opts = getattr(opener, "options", None)
        if opts is not None:
            cachesize = opts.get("manifestcachesize", cachesize)
        self._treeinmem = True

        self._opener = opener

        # A cache of the manifestctx or treemanifestctx for each directory
        self._dirmancache = {}
        self._dirmancache[""] = util.lrucachedict(cachesize)

        self.cachesize = cachesize

    @util.propertycache
    def _revlog(self):
        return self.revlogstore._revlog("")


class treeonlymanifestlog(basetreemanifestlog):
    def __init__(self, opener, repo):
        self._repo = repo
        super(treeonlymanifestlog, self).__init__(self._repo)
        self._opener = opener
        self.ui = repo.ui

    def clearcaches(self):
        pass

    def _maplinknode(self, linknode):
        """Turns a linknode into a linkrev. Only needed for revlog backed
        manifestlogs."""
        return self._repo.changelog.rev(linknode)

    def _maplinkrev(self, linkrev):
        """Turns a linkrev into a linknode. Only needed for revlog backed
        manifestlogs."""
        return self._repo.changelog.node(linkrev)


class hybridmanifestlog(manifest.manifestlog):
    def __init__(self, opener, repo):
        super(hybridmanifestlog, self).__init__(opener, repo)

        self._opener = opener
        self.ui = repo.ui

        self.treemanifestlog = treemanifestlog(opener, repo)
        setuptreestores(repo, self.treemanifestlog)
        if useruststore(self.ui):
            self.treescmstore = self.treemanifestlog.treescmstore
        self.datastore = self.treemanifestlog.datastore
        self.historystore = self.treemanifestlog.historystore

        if util.safehasattr(self.treemanifestlog, "shareddatastores"):
            self.shareddatastores = self.treemanifestlog.shareddatastores
            self.localdatastores = self.treemanifestlog.localdatastores
            self.sharedhistorystores = self.treemanifestlog.sharedhistorystores
            self.localhistorystores = self.treemanifestlog.localhistorystores

    def commitpending(self):
        super(hybridmanifestlog, self).commitpending()
        self.treemanifestlog.commitpending()

    def abortpending(self):
        super(hybridmanifestlog, self).abortpending()
        self.treemanifestlog.abortpending()


def _buildtree(manifestlog, node=None):
    # this code seems to belong in manifestlog but I have no idea how
    # manifestlog objects work
    store = manifestlog.datastore
    initfn = rustmanifest.treemanifest
    if node is not None and node != nullid:
        return initfn(store, node)
    else:
        return initfn(store)


def _finalize(manifestlog, tree, p1node=None, p2node=None):
    parents = []
    p1tree = _getparenttree(manifestlog, p1node)
    if p1tree is not None:
        parents.append(p1tree)
    p2tree = _getparenttree(manifestlog, p2node)
    if p2tree is not None:
        parents.append(p2tree)
    return tree.finalize(*parents)


def _getparenttree(manifestlog, node=None):
    if node is None or node == nullid:
        return None
    tree = manifestlog[node].read()
    if util.safehasattr(tree, "_treemanifest"):
        # Detect hybrid manifests and unwrap them
        tree = tree._treemanifest()
    return tree


class treemanifestctx(object):
    def __init__(self, manifestlog, dir, node):
        self._manifestlog = manifestlog
        self._dir = dir
        self._node = node
        self._tree = None

    def read(self):
        if self._tree is None:
            self._tree = _buildtree(self._manifestlog, self._node)
        return self._tree

    def node(self):
        return self._node

    def new(self, dir=""):
        if dir != "":
            raise RuntimeError(
                "native tree manifestlog doesn't support " "subdir creation: '%s'" % dir
            )
        return _buildtree(self._manifestlog)

    def copy(self):
        memmf = memtreemanifestctx(self._manifestlog, dir=self._dir)
        memmf._treemanifest = self.read().copy()
        return memmf

    @util.propertycache
    def parents(self):
        store = self._manifestlog.historystore
        p1, p2, linkrev, copyfrom = store.getnodeinfo(self._dir, self._node)
        if copyfrom:
            p1 = nullid
        return p1, p2

    def readnew(self, shallow=False):
        """Returns a manifest containing just the entries that are present
        in this manifest, but not in its p1 manifest. This is efficient to read
        if the revlog delta is already p1.

        If `shallow` is True, this will read the delta for this directory,
        without recursively reading subdirectory manifests. Instead, any
        subdirectory entry will be reported as it appears in the manifest, i.e.
        the subdirectory will be reported among files and distinguished only by
        its 't' flag.
        """
        p1, p2 = self.parents
        mf = self.read()
        parentmf = _buildtree(self._manifestlog, p1)

        if shallow:
            # This appears to only be used for changegroup creation in
            # upstream changegroup.py. Since we use pack files for all native
            # tree exchanges, we shouldn't need to implement this.
            raise NotImplemented("native trees don't support shallow " "readdelta yet")
        else:
            md = _buildtree(self._manifestlog)
            for f, ((n1, fl1), (n2, fl2)) in pycompat.iteritems(parentmf.diff(mf)):
                if n2:
                    md[f] = n2
                    if fl2:
                        md.setflag(f, fl2)
            return md

    def find(self, key):
        return self.read().find(key)


class memtreemanifestctx(object):
    def __init__(self, manifestlog, dir=""):
        self._manifestlog = manifestlog
        self._dir = dir
        self._treemanifest = _buildtree(manifestlog)

    def new(self, dir=""):
        return memtreemanifestctx(self._manifestlog, dir=dir)

    def copy(self):
        memmf = memtreemanifestctx(self._manifestlog, dir=self._dir)
        memmf._treemanifest = self._treemanifest.copy()
        return memmf

    def read(self):
        return self._treemanifest

    def write(self, tr, linkrev, p1, p2, added, removed):
        mfl = self._manifestlog

        newtree = self._treemanifest

        overridenode = None
        overridep1node = None
        # linknode=None because the linkrev is provided
        node = mfl.add(
            mfl.ui,
            newtree,
            p1,
            p2,
            None,
            overridenode=overridenode,
            overridep1node=overridep1node,
            tr=tr,
            linkrev=linkrev,
        )
        return node


def _addservercaps(repo, caps):
    caps = set(caps)
    caps.add("gettreepack")
    caps.add("designatednodes")
    if repo.ui.configbool("treemanifest", "treeonly"):
        caps.add("treeonly")
    # other code expects caps to be a list, not a set
    return list(caps)


def serverreposetup(repo):
    def _capabilities(orig, repo, proto):
        caps = orig(repo, proto)
        caps = _addservercaps(repo, caps)
        return caps

    if util.safehasattr(wireproto, "_capabilities"):
        extensions.wrapfunction(wireproto, "_capabilities", _capabilities)
    else:
        extensions.wrapfunction(wireproto, "capabilities", _capabilities)

    repo.ui.setconfig("hooks", "pretxnclose.checkmanifest", verifymanifesthook)


def verifymanifesthook(ui, repo, **kwargs):
    """pretxnclose hook that verifies that every newly added commit has a
    corresponding root manifest."""
    node = kwargs.get("node")
    if node is None:
        return

    newctxs = list(repo.set("%s:", node))
    mfnodes = set(ctx.manifestnode() for ctx in newctxs)

    mfdatastore = repo.manifestlog.datastore
    missing = mfdatastore.getmissing(("", mfnode) for mfnode in mfnodes)

    if missing:
        missingmfnodes = set(hex(key[1]) for key in missing)
        hexnodes = list(
            ctx.hex() for ctx in newctxs if hex(ctx.manifestnode()) in missingmfnodes
        )
        raise error.Abort(
            _(
                "attempting to close transaction which includes commits (%s) without "
                "manifests (%s)"
            )
            % (", ".join(hexnodes), ", ".join(missingmfnodes))
        )


def getmanifestlog(orig, self):
    if not treeenabled(self.ui):
        return orig(self)

    if self.ui.configbool("treemanifest", "treeonly"):
        if self.ui.configbool("treemanifest", "server"):
            mfl = treemanifestlog(self.svfs, self)
        else:
            mfl = treeonlymanifestlog(self.svfs, self)
        setuptreestores(self, mfl)
    else:
        mfl = hybridmanifestlog(self.svfs, self)

    return mfl


def getbundlemanifestlog(orig, self):
    mfl = orig(self)
    if not treeenabled(self.ui):
        return mfl

    wrapmfl = mfl
    if isinstance(mfl, hybridmanifestlog):
        wrapmfl = mfl.treemanifestlog

    class pendingmempack(object):
        def __init__(self):
            self._mutabledpack = None
            self._mutablehpack = None

        def getmutabledpack(self, read=False):
            if self._mutabledpack is None and not read:
                self._mutabledpack = memdatapack()
            return self._mutabledpack

        def getmutablehpack(self, read=False):
            if self._mutablehpack is None and not read:
                self._mutablehpack = memhistorypack()
            return self._mutablehpack

        def getmutablepack(self):
            dpack = self.getmutabledpack()
            hpack = self.getmutablehpack()

            return dpack, hpack

    class bundlemanifestlog(wrapmfl.__class__):
        def add(
            self,
            ui,
            newtree,
            p1node,
            p2node,
            linknode,
            overridenode=None,
            overridep1node=None,
            tr=None,
            linkrev=None,
        ):
            return self._addtopack(
                ui,
                newtree,
                p1node,
                p2node,
                linknode,
                overridenode=overridenode,
                overridep1node=overridep1node,
                linkrev=linkrev,
            )

        def commitpending(self):
            pass

        def abortpending(self):
            self._mutabelocalpacks = None
            self._mutablesharedpacks = None

    wrapmfl.__class__ = bundlemanifestlog
    wrapmfl._mutablelocalpacks = pendingmempack()
    wrapmfl._mutablesharedpacks = pendingmempack()
    return mfl


def _writemanifestwrapper(orig, self, tr, link, p1, p2, added, removed):
    n = orig(self, tr, link, p1, p2, added, removed)

    mfl = self._manifestlog
    if (
        util.safehasattr(mfl._revlog.opener, "treemanifestserver")
        and mfl._revlog.opener.treemanifestserver
    ):
        # Since we're adding the root flat manifest, let's add the corresponding
        # root tree manifest.
        tmfl = mfl.treemanifestlog
        _converttotree(tr, mfl, tmfl, self, linkrev=link, torevlog=True)

    return n


@command("debuggetroottree", [], "NODE")
def debuggetroottree(ui, repo, rootnode):
    with ui.configoverride({("treemanifest", "fetchdepth"): "1"}, "forcesinglefetch"):
        repo.prefetchtrees([bin(rootnode)])


@command(
    "debuggentrees",
    [
        (
            "s",
            "skip-allowed-roots",
            None,
            _("skips the check for only generating on allowed roots"),
        ),
        ("", "verify", None, _("verify consistency of tree data")),
    ],
    _("hg debuggentrees FIRSTREV LASTREV"),
)
def debuggentrees(ui, repo, rev1, rev2, *args, **opts):
    rev1 = repo.revs(rev1).first()
    rev2 = repo.revs(rev2).last()

    mfrevlog = repo.manifestlog._revlog
    mfrev1 = mfrevlog.rev(repo[rev1].manifestnode())
    mfrev2 = mfrevlog.rev(repo[rev2].manifestnode()) + 1

    packpath = shallowutil.getcachepackpath(repo, PACK_CATEGORY)
    if opts.get("skip_allowed_roots", False):
        ui.setconfig("treemanifest", "allowedtreeroots", None)
    with mutablestores.mutabledatastore(repo, packpath) as dpack:
        with mutablestores.mutablehistorystore(repo, packpath) as hpack:
            recordmanifest(
                dpack, hpack, repo, mfrev1, mfrev2, verify=opts.get("verify", False)
            )


@command("backfillmanifestrevlog", [], _("hg backfillmanifestrevlog"))
def backfillmanifestrevlog(ui, repo, *args, **opts):
    """Download any missing manifest revlog entries. This is useful when
    transitioning back from a treeonly repo to a flat+tree hybrid repo."""
    fallbackpath = getfallbackpath(repo)
    with repo.connectionpool.get(fallbackpath) as conn:
        remote = conn.peer

        # _localrepo is needed for remotefilelog to work
        if util.safehasattr(remote, "_callstream"):
            remote._localrepo = repo

        cl = repo.changelog
        mfrevlog = repo.manifestlog._revlog

        # We need to download any manifests the server has that we don't. We
        # calculate that by saying we need all the public heads, and that we
        # have some of them already. This might result in extra downloading but
        # they become no-ops when attempting to be added to the revlog.
        publicheads = list(repo.revs("heads(public())"))
        clnode = cl.node
        heads = [clnode(r) for r in publicheads]

        # Only request heads the server knows about
        knownheads = list(remote.known(heads))
        heads = [n for i, n in enumerate(heads) if knownheads[i]]
        common = [
            clnode(r)
            for i, r in enumerate(publicheads)
            if knownheads[i] and cl.changelogrevision(r).manifest in mfrevlog.nodemap
        ]
        with repo.wlock(), repo.lock(), (repo.transaction("backfillmanifest")) as tr:
            bundlecaps = exchange.caps20to10(repo)
            cg = remote.getbundle(
                "pull", bundlecaps=bundlecaps, common=common, heads=heads
            )
            bundle2.applybundle(repo, cg, tr, "pull", remote.url())


@command(
    "backfilltree", [("l", "limit", "10000000", _(""))], _("hg backfilltree [OPTIONS]")
)
def backfilltree(ui, repo, *args, **opts):
    if isinstance(repo.manifestlog, treemanifestlog):
        repo.ui.warn(_("backfilltree is not supported on a tree-only repo\n"))
        return

    with repo.wlock(), repo.lock(), repo.transaction("backfilltree") as tr:
        start, end = _getbackfillrange(repo, int(opts.get("limit")))
        if start <= end:
            mfl = repo.manifestlog
            tmfl = mfl.treemanifestlog
            revs = range(start, end)
            _backfilltree(tr, repo, mfl, tmfl, revs)


def _getbackfillrange(repo, limit):
    treerevlog = repo.manifestlog.treemanifestlog._revlog
    maxrev = len(treerevlog) - 1
    start = treerevlog.linkrev(maxrev) + 1

    numclrevs = len(repo.changelog)
    end = min(numclrevs, start + limit)
    return (start, end)


def _backfilltree(tr, repo, mfl, tmfl, revs):
    with progress.bar(
        repo.ui, _("converting flat manifest to tree manifest"), total=len(revs)
    ) as prog:
        for rev in revs:
            prog.value += 1
            _converttotree(tr, mfl, tmfl, repo[rev].manifestctx(), torevlog=True)


def _converttotree(tr, mfl, tmfl, mfctx, linkrev=None, torevlog=False):
    # A manifest can be the nullid if the first commit in the repo is an empty
    # commit.
    if mfctx.node() == nullid:
        return

    p1node, p2node = mfctx.parents
    if p1node != nullid:
        try:
            parenttree = tmfl[p1node].read()
            # Just read p2node to verify it's actually present
            tmfl[p2node].read()
        except KeyError:
            raise error.Abort(
                _("unable to find tree parent nodes %s %s") % (hex(p1node), hex(p2node))
            )
    else:
        parenttree = rustmanifest.treemanifest(tmfl.datastore)

    added, removed = _getflatdiff(mfl, mfctx)
    newtree = _getnewtree(parenttree, added, removed)

    # Let's use the provided ctx's linknode. We exclude memmanifestctx's because
    # they haven't been committed yet and don't actually have a linknode yet.
    linknode = None
    if not isinstance(mfctx, manifest.memmanifestctx):
        linknode = mfctx.linknode
        if linkrev is not None:
            if linknode != tmfl._maplinkrev(linkrev):
                raise error.ProgrammingError(
                    (
                        "linknode '%s' doesn't match "
                        "linkrev '%s:%s' during tree conversion"
                    )
                    % (hex(linknode), linkrev, hex(tmfl._maplinkrev(linkrev)))
                )
        # Since we have a linknode, let's not use the linkrev.
        linkrev = None
    tmfl.add(
        mfl.ui,
        newtree,
        p1node,
        p2node,
        linknode,
        overridenode=mfctx.node(),
        overridep1node=p1node,
        tr=tr,
        linkrev=linkrev,
    )


def _difftoaddremove(diff):
    added = []
    removed = []
    for filename, (old, new) in pycompat.iteritems(diff):
        if new is not None and new[0] is not None:
            added.append((filename, new[0], new[1]))
        else:
            removed.append(filename)
    return added, removed


def _getnewtree(parenttree, added, removed):
    newtree = parenttree.copy()
    for fname in removed:
        del newtree[fname]

    for fname, fnode, fflags in added:
        newtree.set(fname, fnode, fflags)

    return newtree


def _getflatdiff(mfl, mfctx):
    mfrevlog = mfl._revlog
    rev = mfrevlog.rev(mfctx.node())
    p1, p2 = mfrevlog.parentrevs(rev)
    p1node = mfrevlog.node(p1)
    p2node = mfrevlog.node(p2)
    linkrev = mfrevlog.linkrev(rev)

    # We have to fall back to the slow path for merge commits and for commits
    # that are currently being made, since they haven't written their changelog
    # data yet and it is necessary for the fastpath.
    if p2node != nullid or linkrev >= len(mfl._repo.changelog):
        diff = mfl[p1node].read().diff(mfctx.read())
        deletes = []
        adds = []
        for filename, ((anode, aflag), (bnode, bflag)) in pycompat.iteritems(diff):
            if bnode is None:
                deletes.append(filename)
            else:
                adds.append((filename, bnode, bflag))
    else:
        # This will generally be very quick, since p1 == deltabase
        delta = mfrevlog.revdiff(p1, rev)

        deletes = []
        adds = []

        # Inspect the delta and read the added files from it
        current = 0
        end = len(delta)
        while current < end:
            try:
                block = b""
                # Deltas are of the form:
                #   <start><end><datalen><data>
                # Where start and end say what bytes to delete, and data
                # says what bytes to insert in their place. So we can
                # just read <data> to figure out all the added files.
                byte1, byte2, blocklen = struct.unpack(
                    ">lll", delta[current : current + 12]
                )
                current += 12
                if blocklen:
                    block = delta[current : current + blocklen]
                    current += blocklen
            except struct.error:
                raise RuntimeError("patch cannot be decoded")

            # An individual delta block may contain multiple newline
            # delimited entries.
            for line in block.split(b"\n"):
                if not line:
                    continue
                fname, rest = pycompat.decodeutf8(line).split("\0")
                fnode = rest[:40]
                fflag = rest[40:]
                adds.append((fname, bin(fnode), fflag))

        allfiles = set(mfl._repo.changelog.readfiles(linkrev))
        deletes = allfiles.difference(fname for fname, fnode, fflag in adds)
    return adds, deletes


def _unpackmanifestscg3(orig, self, repo, *args, **kwargs):
    if not treeenabled(repo.ui):
        return orig(self, repo, *args, **kwargs)

    if repo.ui.configbool("treemanifest", "treeonly"):
        self.manifestheader()
        _convertdeltastotrees(repo, self.deltaiter())
        # Handle sub-tree manifests
        for chunkdata in iter(self.filelogheader, {}):
            raise error.ProgrammingError(
                "sub-trees are not supported in a " "changegroup"
            )
        return
    return orig(self, repo, *args, **kwargs)


def _unpackmanifestscg1(orig, self, repo, revmap, trp, numchanges):
    if not treeenabled(repo.ui):
        return orig(self, repo, revmap, trp, numchanges)

    if repo.ui.configbool("treemanifest", "treeonly"):
        self.manifestheader()
        if repo.svfs.treemanifestserver:
            for chunkdata in self.deltaiter():
                raise error.Abort(_("treeonly server cannot receive flat " "manifests"))
        else:
            _convertdeltastotrees(repo, self.deltaiter())
        return

    mfrevlog = repo.manifestlog._revlog
    oldtip = len(mfrevlog)

    mfnodes = orig(self, repo, revmap, trp, numchanges)

    if repo.svfs.treemanifestserver:
        mfl = repo.manifestlog
        tmfl = repo.manifestlog.treemanifestlog
        for mfnode in mfnodes:
            linkrev = mfrevlog.linkrev(mfrevlog.rev(mfnode))
            _converttotree(trp, mfl, tmfl, mfl[mfnode], linkrev=linkrev, torevlog=True)

    if util.safehasattr(repo.manifestlog, "datastore") and repo.ui.configbool(
        "treemanifest", "autocreatetrees"
    ):

        # TODO: only put in cache if pulling from main server
        packpath = shallowutil.getcachepackpath(repo, PACK_CATEGORY)
        with mutablestores.mutabledatastore(repo, packpath) as dpack:
            with mutablestores.mutablehistorystore(repo, packpath) as hpack:
                recordmanifest(dpack, hpack, repo, oldtip, len(mfrevlog))

        # Alert the store that there may be new packs
        repo.manifestlog.datastore.markforrefresh()


def _convertdeltastotrees(repo, deltas):
    lrucache = util.lrucachedict(10)
    first = False
    with progress.spinner(repo.ui, _("converting manifests to trees")):
        for chunkdata in deltas:
            if not first:
                first = True
                repo.ui.debug("converting flat manifests to treemanifests\n")
            _convertdeltatotree(repo, lrucache, *chunkdata)


def _convertdeltatotree(
    repo, lrucache, node, p1, p2, linknode, deltabase, delta, flags
):
    """Converts the given flat manifest delta into a tree. This may be extremely
    slow since it may need to rebuild a flat manifest full text from a tree."""
    mfl = repo.manifestlog

    def gettext(tree, node):
        text = lrucache.get(node)
        if text is not None:
            return text
        text = tree.text()
        lrucache[node] = text
        return text

    # Get flat base mf text
    parenttree = mfl[p1].read()
    parenttext = gettext(parenttree, p1)
    if p1 == deltabase:
        deltabasetext = parenttext
    else:
        deltabasetree = mfl[deltabase].read()
        deltabasetext = gettext(deltabasetree, deltabase)

    # Get flat manifests
    parentflat = manifest.manifestdict(parenttext)

    newflattext = bytes(mdiff.patch(deltabasetext, delta))
    lrucache[node] = newflattext
    newflat = manifest.manifestdict(newflattext)

    # Diff old and new flat text to get new tree
    added, removed = _difftoaddremove(parentflat.diff(newflat))
    newtree = _getnewtree(parenttree, added, removed)

    # Save new tree
    mfl.add(mfl.ui, newtree, p1, p2, linknode, overridenode=node, overridep1node=p1)


class InterceptedMutableDataPack(object):
    """This classes intercepts data pack writes and replaces the node for the
    root with the provided node. This is useful for forcing a tree manifest to
    be referencable via its flat hash.
    """

    def __init__(self, pack, node, p1node):
        self._pack = pack
        self._node = node
        self._p1node = p1node

    def add(self, name, node, deltabasenode, delta):
        # For the root node, provide the flat manifest as the key
        if name == "":
            node = self._node
            if deltabasenode != nullid:
                deltabasenode = self._p1node
        return self._pack.add(name, node, deltabasenode, delta)


class InterceptedMutableHistoryPack(object):
    """This classes intercepts history pack writes and replaces the node for the
    root with the provided node. This is useful for forcing a tree manifest to
    be referencable via its flat hash.
    """

    def __init__(self, pack, node, p1node):
        self._pack = pack
        self._node = node
        self._p1node = p1node
        self.entries = []

    def add(self, filename, node, p1, *args, **kwargs):
        # For the root node, provide the flat manifest as the key
        if filename == "":
            node = self._node
            if p1 != nullid:
                p1 = self._p1node
        self._pack.add(filename, node, p1, *args, **kwargs)


def recordmanifest(datapack, historypack, repo, oldtip, newtip, verify=False):
    cl = repo.changelog
    mfl = repo.manifestlog
    mfrevlog = mfl._revlog
    total = newtip - oldtip
    ui = repo.ui
    builttrees = {}

    refcount = {}
    for rev in range(oldtip, newtip):
        p1 = mfrevlog.parentrevs(rev)[0]
        p1node = mfrevlog.node(p1)
        refcount[p1node] = refcount.get(p1node, 0) + 1

    allowedtreeroots = set()
    for name in repo.ui.configlist("treemanifest", "allowedtreeroots"):
        if name in repo:
            allowedtreeroots.add(repo[name].manifestnode())

    includedentries = set()
    with progress.bar(ui, _("priming tree cache"), total=total) as prog:
        for rev in range(oldtip, newtip):
            prog.value = rev - oldtip
            node = mfrevlog.node(rev)
            p1, p2 = mfrevlog.parentrevs(rev)
            p1node = mfrevlog.node(p1)
            linkrev = mfrevlog.linkrev(rev)
            linknode = cl.node(linkrev)

            if p1node == nullid:
                origtree = _buildtree(mfl)
            elif p1node in builttrees:
                origtree = builttrees[p1node]
            else:
                origtree = mfl[p1node].read()._treemanifest()

            if origtree is None:
                if allowedtreeroots and p1node not in allowedtreeroots:
                    continue

                p1mf = mfl[p1node].read()
                p1linknode = cl.node(mfrevlog.linkrev(p1))
                origtree = _buildtree(mfl)
                for filename, fnode, flag in p1mf.iterentries():
                    origtree.set(filename, fnode, flag)

                tempdatapack = InterceptedMutableDataPack(datapack, p1node, nullid)
                temphistorypack = InterceptedMutableHistoryPack(
                    historypack, p1node, nullid
                )
                for nname, nnode, ntext, _np1text, np1, np2 in origtree.finalize():
                    # No need to compute a delta, since we know the parent isn't
                    # already a tree.
                    tempdatapack.add(nname, nnode, nullid, ntext)
                    temphistorypack.add(nname, nnode, np1, np2, p1linknode, "")
                    includedentries.add((nname, nnode))

                builttrees[p1node] = origtree

            # Remove the tree from the cache once we've processed its final use.
            # Otherwise memory explodes
            p1refcount = refcount[p1node] - 1
            if p1refcount == 0:
                builttrees.pop(p1node, None)
            refcount[p1node] = p1refcount

            adds, deletes = _getflatdiff(mfl, mfl[node])

            # Apply the changes on top of the parent tree
            newtree = _getnewtree(origtree, adds, deletes)

            tempdatapack = InterceptedMutableDataPack(
                datapack, mfrevlog.node(rev), p1node
            )
            temphistorypack = InterceptedMutableHistoryPack(
                historypack, mfrevlog.node(rev), p1node
            )
            mfdatastore = mfl.datastore
            newtreeiter = newtree.finalize(origtree if p1node != nullid else None)
            for nname, nnode, ntext, _np1text, np1, np2 in newtreeiter:
                if verify:
                    # Verify all children of the tree already exist in the store
                    # somewhere.
                    lines = ntext.split("\n")
                    for line in lines:
                        if not line:
                            continue
                        childname, nodeflag = line.split("\0")
                        childpath = os.path.join(nname, childname)
                        cnode = nodeflag[:40]
                        cflag = nodeflag[40:]
                        if (
                            cflag == "t"
                            and (childpath + "/", bin(cnode)) not in includedentries
                            and mfdatastore.getmissing([(childpath, bin(cnode))])
                        ):
                            import pdb

                            pdb.set_trace()

                tempdatapack.add(nname, nnode, nullid, ntext)
                temphistorypack.add(nname, nnode, np1, np2, linknode, "")
                includedentries.add((nname, nnode))

            if ui.configbool("treemanifest", "verifyautocreate", False):
                diff = newtree.diff(origtree)
                for fname in deletes:
                    fdiff = diff.get(fname)
                    if fdiff is None:
                        import pdb

                        pdb.set_trace()
                    else:
                        l, r = fdiff
                        if l != (None, ""):
                            import pdb

                            pdb.set_trace()

                for fname, fnode, fflags in adds:
                    fdiff = diff.get(fname)
                    if fdiff is None:
                        # Sometimes adds are no-ops, so they don't show up in
                        # the diff.
                        if origtree.get(fname) != newtree.get(fname):
                            import pdb

                            pdb.set_trace()
                    else:
                        l, r = fdiff
                        if l != (fnode, fflags):
                            import pdb

                            pdb.set_trace()
            builttrees[mfrevlog.node(rev)] = newtree

            mfnode = mfrevlog.node(rev)
            if refcount.get(mfnode) > 0:
                builttrees[mfnode] = newtree


def _checkhash(orig, self, *args, **kwargs):
    # Don't validate root hashes during the transition to treemanifest
    if self.indexfile.endswith("00manifesttree.i"):
        return
    return orig(self, *args, **kwargs)


# Wrapper around the 'prefetch' command which also allows for prefetching the
# trees along with the files.
def _prefetchwrapper(orig, ui, repo, *pats, **opts):
    # The wrapper will take care of the repacking.
    repackrequested = opts.pop("repack")

    _prefetchonlytrees(repo, opts)
    _prefetchonlyfiles(orig, ui, repo, *pats, **opts)

    if repackrequested:
        domaintenancerepack(repo)


# Wrapper around the 'prefetch' command which overrides the command completely
# and only allows for prefetching trees. This is only required when the
# 'prefetch' command is not available because the remotefilelog extension is not
# loaded and we want to be able to at least prefetch trees. The wrapping just
# ensures that we get a consistent interface to the 'prefetch' command.
def _overrideprefetch(orig, ui, repo, *pats, **opts):
    if opts.get("repack"):
        raise error.Abort(_("repack requires remotefilelog extension"))

    _prefetchonlytrees(repo, opts)


def _prefetchonlyfiles(orig, ui, repo, *pats, **opts):
    if shallowrepo.requirement in repo.requirements:
        orig(ui, repo, *pats, **opts)


def _prefetchonlytrees(repo, opts):
    opts = resolveprefetchopts(repo.ui, opts)
    revs = scmutil.revrange(repo, opts.get("rev"))

    # No trees need to be downloaded for the non-public commits.
    spec = revsetlang.formatspec("%ld & public()", revs)
    mfnodes = set(ctx.manifestnode() for ctx in repo.set(spec))

    basemfnode = set()
    base = opts.get("base")
    if base is not None:
        basemfnode.add(repo[base].manifestnode())

    repo.prefetchtrees(mfnodes, basemfnodes=basemfnode)


def _gettrees(
    repo,
    remote,
    rootdir,
    mfnodes,
    basemfnodes,
    directories,
    start,
    depth,
    ondemandfetch=False,
):
    if "gettreepack" not in shallowutil.peercapabilities(remote):
        raise error.Abort(_("missing gettreepack capability on remote"))
    bundle = remote.gettreepack(rootdir, mfnodes, basemfnodes, directories, depth)

    try:
        op = bundle2.processbundle(repo, bundle, None)

        receivednodes = op.records[RECEIVEDNODE_RECORD]
        count = 0
        missingnodes = set(mfnodes)

        for reply in receivednodes:
            # If we're doing on-demand tree fetching, this means that we are not
            # trying to fetch complete trees. Consequently, the set of mfnodes
            # passed in are not all different versions of the same root
            # directory -- instead they correspond to individual subdirectories
            # within a single tree, which we are explicitly not downloading in
            # its entirety. This means we should not check the directory path
            # when checking for missing nodes in the response.
            if ondemandfetch:
                missingnodes.difference_update(n for d, n in reply)
            else:
                missingnodes.difference_update(n for d, n in reply if d == rootdir)
            count += len(reply)
        perftrace.tracevalue("Fetched", count)
        if op.repo.ui.configbool("remotefilelog", "debug"):
            duration = util.timer() - start
            op.repo.ui.warn(_("%s trees fetched over %0.2fs\n") % (count, duration))

        if missingnodes:
            raise shallowutil.MissingNodesError(
                (("", n) for n in missingnodes),
                "tree nodes missing from server response",
            )
    except bundle2.AbortFromPart as exc:
        repo.ui.debug("remote: abort: %s\n" % exc)
        # Give stderr some time to reach the client, so we can read it into the
        # currently pushed ui buffer, instead of it randomly showing up in a
        # future ui read.
        raise shallowutil.MissingNodesError((("", n) for n in mfnodes), hint=exc.hint)
    except error.BundleValueError as exc:
        raise error.Abort(_("missing support for %s") % exc)


def _registerbundle2parts():
    @bundle2.parthandler(TREEGROUP_PARTTYPE2, ("version", "cache", "category"))
    def treeparthandler2(op, part):
        """Handles received tree packs. If `cache` is True, the received
        data goes in to the shared pack cache. Otherwise, the received data
        goes into the permanent repo local data.
        """
        repo = op.repo

        versionstr = part.params.get("version")
        try:
            version = int(versionstr)
        except ValueError:
            version = 0

        if version < 1 or version > 2:
            raise error.Abort(
                _("unknown treegroup bundle2 part version: %s") % versionstr
            )

        category = part.params.get("category", "")
        if category != PACK_CATEGORY:
            raise error.Abort(_("invalid treegroup pack category: %s") % category)

        cl = repo.changelog
        mfl = repo.manifestlog
        if isinstance(mfl, hybridmanifestlog):
            mfl = repo.manifestlog.treemanifestlog

        # Treemanifest servers don't accept trees directly. They must either go
        # through pushrebase, or be processed manually.
        if repo.svfs.treemanifestserver:
            if not repo.ui.configbool("treemanifest", "treeonly"):
                # We can't accept non-pushrebase treeonly pushes to a hybrid
                # server because the hashes will be wrong and we have no way of
                # returning the new commits to the server.
                raise error.Abort(
                    _("cannot push only trees to a hybrid server " "without pushrebase")
                )
            data = part.read()
            wirepackstore = wirepack.wirepackstore(data, version=version)
            datastore = unioncontentstore(wirepackstore, mfl.datastore)
            tr = op.gettransaction()

            # Sort the trees so they are added in the same order as the commits.
            # This requires that the changegroup be processed first so we can
            # compare the linkrevs.
            rootnodes = (node for name, node in wirepackstore if name == "")
            rootnodes = sorted(
                rootnodes, key=lambda n: cl.rev(wirepackstore.getnodeinfo("", n)[2])
            )

            for node in rootnodes:
                p1, p2, linknode, copyfrom = wirepackstore.getnodeinfo("", node)
                newtree = rustmanifest.treemanifest(datastore, node)

                mfl.add(mfl.ui, newtree, p1, p2, linknode, tr=tr, overridenode=node)
            return

        if part.params.get("cache", "False") == "True":
            dpack, hpack = mfl.getmutablesharedpacks()
        else:
            dpack, hpack = mfl._getmutablelocalpacks()

        receivedhistory, receiveddata = wirepack.receivepack(
            repo.ui, part, dpack, hpack, version=version
        )

        op.records.add(RECEIVEDNODE_RECORD, receiveddata)

    @bundle2.parthandler(TREEGROUP_PARTTYPE, ("version", "treecache"))
    def treeparthandler(op, part):
        treecache = part.params.pop("treecache")
        part.params["cache"] = treecache
        part.params["category"] = PACK_CATEGORY
        return treeparthandler2(op, part)

    @exchange.b2partsgenerator(TREEGROUP_PARTTYPE)
    def gettreepackpart(pushop, bundler):
        # We no longer generate old tree groups
        pass

    @exchange.b2partsgenerator(TREEGROUP_PARTTYPE2)
    @perftrace.tracefunc("gettreepackpart2")
    def gettreepackpart2(pushop, bundler):
        """add parts containing trees being pushed"""
        if "treepack" in pushop.stepsdone or not treeenabled(pushop.repo.ui):
            return
        pushop.stepsdone.add("treepack")

        # Only add trees if we have them
        sendtrees = shallowbundle.cansendtrees(
            pushop.repo, pushop.outgoing.missing, b2caps=bundler.capabilities
        )
        if sendtrees != shallowbundle.NoTrees and pushop.outgoing.missing:
            part = createtreepackpart(
                pushop.repo, pushop.outgoing, TREEGROUP_PARTTYPE2, sendtrees=sendtrees
            )
            bundler.addpart(part)

    @exchange.getbundle2partsgenerator(TREEGROUP_PARTTYPE2)
    def _getbundlechangegrouppart(
        bundler,
        repo,
        source,
        bundlecaps=None,
        b2caps=None,
        heads=None,
        common=None,
        **kwargs,
    ):
        """add parts containing trees being pulled"""
        if (
            "True" not in b2caps.get("treemanifest", [])
            or not treeenabled(repo.ui)
            or repo.svfs.treemanifestserver
            or not kwargs.get("cg", True)
        ):
            return

        outgoing = exchange._computeoutgoing(repo, heads, common)
        sendtrees = shallowbundle.cansendtrees(
            repo, outgoing.missing, bundlecaps=bundlecaps, b2caps=b2caps
        )
        if sendtrees != shallowbundle.NoTrees:
            try:
                part = createtreepackpart(
                    repo, outgoing, TREEGROUP_PARTTYPE2, sendtrees=sendtrees
                )
                bundler.addpart(part)
            except BaseException as ex:
                bundler.addpart(bundle2.createerrorpart(str(ex)))


def createtreepackpart(repo, outgoing, partname, sendtrees=shallowbundle.AllTrees):
    if sendtrees == shallowbundle.NoTrees:
        raise error.ProgrammingError("calling createtreepackpart with NoTrees")

    rootdir = ""
    mfnodes = []
    basemfnodes = []
    directories = []

    linknodemap = {}
    for node in outgoing.missing:
        ctx = repo[node]
        if sendtrees == shallowbundle.AllTrees or ctx.phase() != phases.public:
            mfnode = ctx.manifestnode()
            mfnodes.append(mfnode)
            linknodemap.setdefault(mfnode, node)

    basectxs = repo.set("parents(%ln) - %ln", outgoing.missing, outgoing.missing)
    for basectx in basectxs:
        basemfnodes.append(basectx.manifestnode())
    linknodefixup = (set(outgoing.missing), linknodemap)

    # createtreepackpart is used to form bundles for normal pushes and pulls, so
    # we always pass depth=max here.
    packstream = generatepackstream(
        repo,
        rootdir,
        mfnodes,
        basemfnodes,
        directories,
        TREE_DEPTH_MAX,
        linknodefixup,
        version=1,
    )
    part = bundle2.bundlepart(partname, data=packstream)
    part.addparam("version", "1")
    part.addparam("cache", "False")
    part.addparam("category", PACK_CATEGORY)

    return part


def getfallbackpath(repo):
    if util.safehasattr(repo, "fallbackpath"):
        return repo.fallbackpath
    else:
        path = repo.ui.config("paths", "default")
        if not path:
            raise error.Abort("no remote server configured to fetch trees from")
        return path


def pull(orig, ui, repo, *pats, **opts):
    # If we're not in treeonly mode, and we're missing public commits from the
    # revlog, backfill them.
    if treeenabled(ui) and not ui.configbool("treemanifest", "treeonly"):
        tippublicrevs = repo.revs("last(public())")
        if tippublicrevs:
            ctx = repo[tippublicrevs.first()]
            mfnode = ctx.manifestnode()
            mfrevlog = repo.manifestlog._revlog
            if mfnode not in mfrevlog.nodemap:
                ui.status(_("backfilling missing flat manifests\n"))
                backfillmanifestrevlog(ui, repo)

    result = orig(ui, repo, *pats, **opts)
    if treeenabled(repo.ui):
        try:
            _postpullprefetch(ui, repo)
        except Exception as ex:
            # Errors are not fatal.
            ui.warn(_("failed to prefetch trees after pull: %s\n") % ex)
            ui.log(
                "exceptions",
                exception_type=type(ex).__name__,
                exception_msg=str(ex),
                fatal="false",
            )
    return result


def _postpullprefetch(ui, repo):

    ctxs = []
    mfstore = repo.manifestlog.datastore

    # prefetch if it's configured
    prefetchcount = ui.configint("treemanifest", "pullprefetchcount", None)
    if prefetchcount:
        # Calculate what recent manifests are we missing
        firstrev = max(0, repo["tip"].rev() - prefetchcount + 1)
        ctxs.extend(repo.set("%s: & public()", firstrev))

    # Prefetch specific commits
    prefetchrevs = ui.config("treemanifest", "pullprefetchrevs", None)
    if prefetchrevs:
        ctxs.extend(repo.set(prefetchrevs))

    mfnodes = None
    if ctxs:
        mfnodes = list(c.manifestnode() for c in ctxs)

    if mfnodes:
        if len(mfnodes) == 1:
            ui.status(_("prefetching tree for %s\n") % short(ctxs[0].node()))
        else:
            ui.status(_("prefetching trees for %d commits\n") % len(mfnodes))
        # Calculate which parents we already have
        ctxnodes = list(ctx.node() for ctx in ctxs)
        parentctxs = repo.set("parents(%ln) - %ln", ctxnodes, ctxnodes)
        basemfnodes = set(ctx.manifestnode() for ctx in parentctxs)
        missingbases = list(mfstore.getmissing(("", n) for n in basemfnodes))
        basemfnodes.difference_update(n for k, n in missingbases)

        repo.prefetchtrees(mfnodes, basemfnodes=basemfnodes)


@util.timefunction("findrecenttrees")
def _findrecenttree(repo, startnode, targetmfnodes):
    cl = repo.changelog
    mfstore = repo.manifestlog.datastore
    targetmfnodes = set(targetmfnodes)

    with progress.spinner(repo.ui, _("finding nearest trees")):
        # Look up and down from the given rev
        ancestors = iter(
            repo.revs(
                "limit(reverse(ancestors(%n)), %z) & public()",
                startnode,
                BASENODESEARCHMAX,
            )
        )

        descendantquery = "limit(descendants(%n), %z) & public()"
        if extensions.enabled().get("remotenames", False):
            descendantquery += " & ::remotenames()"
        descendants = iter(repo.revs(descendantquery, startnode, BASENODESEARCHMAX))

        revs = []

        # Zip's the iterators together, using the fillvalue when the shorter
        # iterator runs out of values.
        candidates = zip_longest(ancestors, descendants, fillvalue=None)
        for revs in candidates:
            for rev in revs:
                if rev is None:
                    continue

                mfnode = cl.changelogrevision(rev).manifest

                # In theory none of the target mfnodes should be in the store at
                # all, since that's why we're trying to prefetch them now, but
                # we've seen cases where getmissing claims they are in the
                # store, and therefore we return the target mfnode as the recent
                # tree, which is an invalid request to the server. Let's prevent
                # this while we track down the root cause.
                if mfnode in targetmfnodes:
                    continue

                missing = mfstore.getmissing([("", mfnode)])
                if not missing:
                    return [mfnode]

    return []


def clientgettreepack(remote, rootdir, mfnodes, basemfnodes, directories, depth):
    opts = {}
    opts["rootdir"] = rootdir
    opts["mfnodes"] = wireproto.encodelist(mfnodes)
    opts["basemfnodes"] = wireproto.encodelist(basemfnodes)
    # Serialize directories with a trailing , so we can differentiate the empty
    # directory from the end of the list!
    opts["directories"] = "".join(
        [wireproto.escapestringarg(d) + "," for d in directories]
    )
    opts["depth"] = str(depth)

    ui = remote.ui
    ui.metrics.gauge("ssh_gettreepack_basemfnodes", len(basemfnodes))
    ui.metrics.gauge("ssh_gettreepack_mfnodes", len(mfnodes))
    ui.metrics.gauge("ssh_gettreepack_calls", 1)

    f = remote._callcompressable("gettreepack", **opts)
    return bundle2.getunbundler(remote.ui, f)


def localgettreepack(remote, rootdir, mfnodes, basemfnodes, directories, depth):
    bundler = _gettreepack(
        remote._repo, rootdir, mfnodes, basemfnodes, directories, depth
    )
    chunks = bundler.getchunks()
    cb = util.chunkbuffer(chunks)
    return bundle2.getunbundler(remote._repo.ui, cb)


def servergettreepack(repo, proto, args):
    """A server api for requesting a pack of tree information."""
    if shallowrepo.requirement in repo.requirements:
        raise error.Abort(_("cannot fetch remote files from shallow repo"))
    if not isinstance(proto, sshserver.sshserver):
        raise error.Abort(_("cannot fetch remote files over non-ssh protocol"))

    rootdir = args["rootdir"]
    depth = int(args.get("depth", str(2**16)))

    mfnodes = wireproto.decodelist(args["mfnodes"])
    basemfnodes = wireproto.decodelist(args["basemfnodes"])
    directories = list(
        wireproto.unescapestringarg(d)
        for d in args["directories"].split(",")
        if d != ""
    )

    bundler = _gettreepack(repo, rootdir, mfnodes, basemfnodes, directories, depth)
    return wireproto.streamres(gen=bundler.getchunks(), v1compressible=True)


def _gettreepack(repo, rootdir, mfnodes, basemfnodes, directories, depth):
    try:
        bundler = bundle2.bundle20(repo.ui)
        packstream = generatepackstream(
            repo, rootdir, mfnodes, basemfnodes, directories, depth, version=1
        )
        part = bundler.newpart(TREEGROUP_PARTTYPE2, data=packstream)
        part.addparam("version", "1")
        part.addparam("cache", "True")
        part.addparam("category", PACK_CATEGORY)

    except error.Abort as exc:
        # cleanly forward Abort error to the client
        bundler = bundle2.bundle20(repo.ui)
        bundler.addpart(bundle2.createerrorpart(str(exc), hint=exc.hint))
    except BaseException as exc:
        bundler = bundle2.bundle20(repo.ui)
        bundler.addpart(bundle2.createerrorpart(str(exc)))

    return bundler


def generatepackstream(
    repo,
    rootdir,
    mfnodes,
    basemfnodes,
    directories,
    depth,
    linknodefixup=None,
    version=1,
):
    """
    All size/len/counts are network order unsigned ints.

    Request args:

    `rootdir` - The directory of the tree to send (including its children)
    `mfnodes` - The manifest nodes of the specified root directory to send.
    `basemfnodes` - The manifest nodes of the specified root directory that are
    already on the client.
    `directories` - The fullpath (not relative path) of directories underneath
    the rootdir that should be sent.
    `depth` - The depth from the root that should be sent.
    `linknodefixup` - If not None, this is a pair of (validset, mapping) where
    validset is a set of nodes that are valid linknodes, and mapping is a map
    from root manifest node to the linknode that should be used for all new
    trees in that manifest which don't have a valid linknode.

    Response format:

    [<fileresponse>,...]<10 null bytes>
    fileresponse = <filename len: 2 byte><filename><history><deltas>
    history = <count: 4 byte>[<history entry>,...]
    historyentry = <node: 20 byte><p1: 20 byte><p2: 20 byte>
                   <linknode: 20 byte><copyfrom len: 2 byte><copyfrom>
    deltas = <count: 4 byte>[<delta entry>,...]
    deltaentry = <node: 20 byte><deltabase: 20 byte>
                 <delta len: 8 byte><delta>
    """
    datastore = repo.manifestlog.datastore

    # Throw an exception early if the requested nodes aren't present. It's
    # important that we throw it now and not later during the pack stream
    # generation because at that point we've already added the part to the
    # stream and it's difficult to switch to an error then.
    missing = []
    if len(directories) > 0:
        iterator = zip(mfnodes, directories)
        assert depth == 1
    else:
        iterator = list((n, rootdir) for n in mfnodes)
    for n, dir in iterator:
        try:
            datastore.get(dir, n)
        except shallowutil.MissingNodesError:
            missing.append((dir, n))
    if missing:
        raise shallowutil.MissingNodesError(missing, "tree nodes missing on server")

    return _generatepackstream(
        repo, rootdir, mfnodes, basemfnodes, directories, depth, linknodefixup, version
    )


def _existonserver(repo, mfnode):
    """Check if the root manifest exists on server-side.

    Return True if the server has the mfnode, False otherwise.
    """
    stream, _stats = repo.edenapi.trees(
        [("", mfnode)], {"parents": False, "manifest_blob": False}
    )
    try:
        list(stream)
        return True
    except Exception:
        # The error type story isn't great for now.
        return False


def _generatepackstream(
    repo, rootdir, mfnodes, basemfnodes, directories, depth, linknodefixup, version
):
    """A simple helper function for generatepackstream. This helper is a
    generator, while the main function is not, so we can execute the
    validation logic in the main function immediately without waiting for the
    first iteration.
    """
    historystore = repo.manifestlog.historystore
    datastore = repo.manifestlog.datastore

    # getdesignatednodes
    if len(directories) > 0:
        for mfnode, dir in zip(mfnodes, directories):
            text = datastore.get(dir, mfnode)
            p1node, p2node = historystore.getnodeinfo(dir, mfnode)[:2]

            data = [(mfnode, nullid, text, 0)]

            histdata = historystore.getnodeinfo(dir, mfnode)
            p1node, p2node, linknode, copyfrom = histdata
            history = [(mfnode, p1node, p2node, linknode, copyfrom)]

            for chunk in wirepack.sendpackpart(dir, history, data, version=version):
                yield chunk
        yield wirepack.closepart()
        return

    mfnodeset = set(mfnodes)
    basemfnodeset = set(basemfnodes)

    # Helper function for filtering out non-existent base manifests that were
    # passed. This can happen if the remote client passes a base manifest that
    # the server doesn't know about yet.
    def treeexists(mfnode):
        return bool(not datastore.getmissing([(rootdir, mfnode)]))

    prevmfnode = None
    for node in mfnodes:
        try:
            p1node, p2node = historystore.getnodeinfo(rootdir, node)[:2]
        except KeyError:
            if _existonserver(repo, node):
                # No need to bundle the tree. It is already present on the
                # server-side. Similar to what we do for public commits
                # (createtreepackpart checking ctx.phase() != phases.public)
                continue
            else:
                raise
        # If p1 is being sent or is already on the client, chances are
        # that's the best thing for us to delta against.
        if p1node != nullid and (p1node in mfnodeset or p1node in basemfnodeset):
            basetrees = [(rootdir, p1node)]
        elif basemfnodes and any(treeexists(mfnode) for mfnode in basemfnodes):
            basetrees = [
                (rootdir, basenode) for basenode in basemfnodes if treeexists(basenode)
            ]
        elif prevmfnode:
            # If there are no base nodes and the parent isn't one of the
            # requested mfnodes, then pick another mfnode as a base.
            basetrees = [(rootdir, prevmfnode)]
        else:
            basetrees = []
        prevmfnode = node

        if p2node != nullid and (p2node in mfnodeset or p2node in basemfnodeset):
            basetrees.append((rootdir, p2node))

        # Only use the first two base trees, since the current tree
        # implementation cannot handle more yet.
        basenodes = [mybasenode for (_path, mybasenode) in basetrees]
        subtrees = rustmanifest.subdirdiff(datastore, rootdir, node, basenodes, depth)
        rootlinknode = None
        if linknodefixup is not None:
            validlinknodes, linknodemap = linknodefixup
            rootlinknode = linknodemap.get(node)
        for subname, subnode, subtext, _x, _x in subtrees:
            # Append data
            data = [(subnode, nullid, subtext, 0)]

            # Append history
            # Only append first history for now, since the entire manifest
            # history is very long.
            histdata = historystore.getnodeinfo(subname, subnode)
            p1node, p2node, linknode, copyfrom = histdata
            if rootlinknode is not None and linknode not in validlinknodes:
                linknode = rootlinknode
            history = [(subnode, p1node, p2node, linknode, copyfrom)]

            for chunk in wirepack.sendpackpart(subname, history, data, version=version):
                yield chunk

    yield wirepack.closepart()


class generatingdatastore(pycompat.ABC):
    """Abstract base class representing stores which generate trees on the
    fly and write them to the shared store. Thereafter, the stores replay the
    lookup operation on the shared store expecting it to succeed."""

    def __init__(self, repo):
        self._repo = repo
        self._shareddata = None
        self._beinggenerated = set()

    def setshared(self, shareddata, sharedhistory):
        self._shareddata = shareddata
        self._sharedhistory = sharedhistory

    @abc.abstractmethod
    def _generatetrees(self, name, node):
        pass

    @contextlib.contextmanager
    def _generating(self, name, node):
        key = (name, node)
        if key in self._beinggenerated:
            # This key is already being generated, so we've reentered the
            # generator and hit infinite recurssion.
            raise KeyError((name, hex(node)))

        self._beinggenerated.add(key)
        yield
        self._beinggenerated.remove(key)

    def get(self, name, node):
        with self._generating(name, node):
            self._generatetrees(name, node)
            return self._shareddata.get(name, node)

    def getdeltachain(self, name, node):
        with self._generating(name, node):
            self._generatetrees(name, node)
            return self._shareddata.getdeltachain(name, node)

    def add(self, name, node, data):
        raise RuntimeError("cannot add to a generating store")

    def getmissing(self, keys):
        return keys

    def getmetrics(self):
        return {}

    def getnodeinfo(self, name, node):
        with self._generating(name, node):
            self._generatetrees(name, node)
            return self._sharedhistory.getnodeinfo(name, node)


class remotetreestore(generatingdatastore):
    def __init__(self, repo):
        super(remotetreestore, self).__init__(repo)

        if useruststore(self._repo.ui):
            self.prefetch = self._rustprefetch
        else:
            self.prefetch = self._pythonprefetch

    def _generatetrees(self, name, node):
        self._prefetchtrees([(name, node)])

    def _prefetchtrees(self, keys):
        # If on-demand fetching is enabled, we should attempt to
        # fetch the single tree node over SSH via gettreepack.
        # If the server does not support calling gettreepack in
        # this way, we must resort to a full fetch.
        self._repo.getdesignatednodes(keys)

    def _pythonprefetch(self, keys):
        # Filter out keys for nodes already present locally.
        keys = self._repo.manifestlog.datastore.getmissing(keys)
        if not keys:
            return

        if len(keys) == 1 and list(keys) == [("", nullid)]:
            return

        self._repo.getdesignatednodes(keys)

    def _rustprefetch(self, *args):
        # len() == 3 means it's a call from the Rust store layer, with the
        # mutable stores. len() == 1 means it's a call from Python, which
        # we'll then route to the Rust store layer, which will then come
        # back to here.
        if len(args) == 3:
            keys = args[2]
            self._prefetchtrees(keys)
        else:
            keys = args[0]
            self.datastore.prefetch(keys)


class ondemandtreedatastore(generatingdatastore):
    def _generatetrees(self, name, node):
        repo = self._repo

        def convert(tr):
            if isinstance(repo.manifestlog, hybridmanifestlog):
                mfl = repo.manifestlog
                tmfl = mfl.treemanifestlog
            else:
                # TODO: treeonly bundlerepo's won't work here since the manifest
                # bundle entries aren't being overlayed on the manifestrevlog.
                mfl = manifest.manifestlog(repo.svfs, repo)
                tmfl = repo.manifestlog
            mfctx = manifest.manifestctx(mfl, node)
            _converttotree(tr, mfl, tmfl, mfctx)

        if isinstance(repo, bundlerepo.bundlerepository):
            # bundlerepos do an entirely inmemory conversion. No transaction
            # necessary. This is used for converting flat-only infinitepush
            # bundles to have trees.
            convert(None)
        else:
            if repo.svfs.treemanifestserver:
                # tree servers shouldn't be trying to build non-bundle
                # treemanifests on the fly, so let's abort early.
                # When using hgsql, the transaction below causes an exception
                # during readonly mode. So aborting early prevents that.
                raise shallowutil.MissingNodesError([(name, node)])

            with repo.wlock(), repo.lock():
                with repo.transaction("demandtreegen") as tr:
                    convert(tr)


def _debugcmdfindtreemanifest(orig, ctx):
    manifest = ctx.manifest()
    # Check if the manifest we have is a treemanifest.
    if isinstance(manifest, rustmanifest.treemanifest):
        return manifest
    try:
        # Look up the treemanifest in the treemanifestlog.  There might not be
        # one, so ignore any failures.
        return ctx.repo().manifestlog.treemanifestlog.get("", ctx.manifestnode()).read()
    except Exception:
        pass
    return orig(ctx)


def _debugbundle2part(orig, ui, part, all, **opts):
    if part.type == TREEGROUP_PARTTYPE2:
        indent_string = "    "
        tempstore = wirepack.wirepackstore(part.read())

        ui.write(indent_string)
        ui.write("%s\n" % tempstore.debugstats())
        for key in sorted(tempstore):
            ui.write(indent_string)
            ui.write("%s %s\n" % (hex(key[1]), key[0]))

    orig(ui, part, all, **opts)


def collectfiles(orig, repo, striprev):
    """find out the filelogs affected by the strip"""
    if not treeenabled(repo.ui):
        return orig(repo, striprev)

    files = set()

    for x in range(striprev, len(repo)):
        ctx = repo[x]
        parents = ctx.parents()
        if len(parents) > 1:
            # Merge commits may not list all the files that are different
            # between the two sides. This is fine for stripping filelogs (since
            # any filelog that was changed will be listed), but is not fine for
            # directories, which may have changes despite filelogs not changing
            # (imagine a directory where two different files were added on
            # different sides of the merge. No filelogs change in the merge, but
            # the directory does).
            for parent in parents:
                diff = ctx.manifest().diff(parent.manifest())
                files.update(diff)
        else:
            files.update(repo[x].files())

    return sorted(files)


def striptrees(orig, repo, tr, striprev, files):
    if not treeenabled(repo.ui):
        return orig(repo, tr, striprev, files)

    if repo.ui.configbool("treemanifest", "server"):
        mfl = repo.manifestlog
        if isinstance(mfl, hybridmanifestlog):
            treemfl = repo.manifestlog.treemanifestlog
        elif isinstance(mfl, treemanifestlog):
            treemfl = mfl
        else:
            raise RuntimeError("cannot strip trees from %s type manifestlog" % mfl)

        treerevlog = treemfl._revlog
        for dir in util.dirs(files):
            # If the revlog doesn't exist, this returns an empty revlog and is a
            # no-op.
            rl = treerevlog.dirlog(dir)
            rl.strip(striprev, tr)

        treerevlog.strip(striprev, tr)


def _addpartsfromopts(orig, ui, repo, bundler, source, outgoing, *args, **kwargs):
    orig(ui, repo, bundler, source, outgoing, *args, **kwargs)

    # Only add trees to bundles for tree enabled clients. Servers use revlogs
    # and therefore will use changegroup tree storage.
    if treeenabled(repo.ui) and not repo.svfs.treemanifestserver:
        # Only add trees if we have them
        sendtrees = shallowbundle.cansendtrees(
            repo, outgoing.missing, b2caps=bundler.capabilities
        )
        if sendtrees != shallowbundle.NoTrees:
            part = createtreepackpart(
                repo, outgoing, TREEGROUP_PARTTYPE2, sendtrees=sendtrees
            )
            bundler.addpart(part)


def _handlebundle2part(orig, self, bundle, part):
    if part.type == TREEGROUP_PARTTYPE2:
        tempstore = wirepack.wirepackstore(part.read())

        # Point the bundle repo at the temp stores
        mfl = self.manifestlog
        mfl.datastore = unioncontentstore(tempstore, mfl.datastore)
        mfl.historystore = unionmetadatastore(tempstore, mfl.historystore)

        if isinstance(mfl, hybridmanifestlog):
            tmfl = mfl.treemanifestlog
            tmfl.datastore = mfl.datastore
            tmfl.historystore = mfl.historystore
    else:
        orig(self, bundle, part)


NODEINFOFORMAT = "!20s20s20sI"
NODEINFOLEN = struct.calcsize(NODEINFOFORMAT)


class nodeinfoserializer(object):
    """Serializer for node info"""

    @staticmethod
    def serialize(value):
        p1, p2, linknode, copyfrom = value
        copyfrom = pycompat.encodeutf8(copyfrom if copyfrom else "")
        return struct.pack(NODEINFOFORMAT, p1, p2, linknode, len(copyfrom)) + copyfrom

    @staticmethod
    def deserialize(raw):
        p1, p2, linknode, copyfromlen = struct.unpack_from(NODEINFOFORMAT, raw, 0)
        if len(raw) != NODEINFOLEN + copyfromlen:
            raise IOError(
                "invalid nodeinfo serialization: %s %s %s %s %s"
                % (hex(p1), hex(p2), hex(linknode), str(copyfromlen), raw[NODEINFOLEN:])
            )
        return (
            p1,
            p2,
            linknode,
            pycompat.decodeutf8(raw[NODEINFOLEN : NODEINFOLEN + copyfromlen]),
        )


class cachestoreserializer(object):
    """Simple serializer that attaches key and sha1 to the content"""

    def __init__(self, key):
        self.key = pycompat.encodeutf8(key)

    def serialize(self, value):
        sha = hashlib.sha1(value).digest()
        return sha + struct.pack("!I", len(self.key)) + self.key + value

    def deserialize(self, raw):
        key = self.key
        if not raw:
            raise IOError("missing content for the key in the cache: %s" % key)
        sha = raw[:20]
        keylen = struct.unpack_from("!I", raw, 20)[0]
        storedkey = raw[24 : 24 + keylen]
        if storedkey != key:
            raise IOError(
                "cache value has key '%s' but '%s' expected" % (storedkey, key)
            )
        value = raw[24 + keylen :]
        realsha = hashlib.sha1(value).digest()
        if sha != realsha:
            raise IOError("invalid content for the key in the cache: %s" % key)
        return value


class cachestorecommon(pycompat.ABC):
    def __init__(self, store, version):
        self.store = store
        self.version = version

    ########### HELPERS ########################################

    def _key(self, name, node, category):
        shakey = hex(hashlib.sha1(pycompat.encodeutf8(name) + node).digest())
        return os.path.join(
            "trees", "v" + str(self.version), category, shakey[:2], shakey[2:]
        )

    ########### APIS ###########################################

    def getnodeinfo(self, name, node):
        key = self._key(name, node, "nodeinfo")
        try:
            value = self._read(key)
            if value:
                return nodeinfoserializer.deserialize(value)
        except (IOError, OSError):
            pass
        # Failover to the underlying storage and update the cache
        nodeinfo = self.store.getnodeinfo(name, node)
        self._write(key, nodeinfoserializer.serialize(nodeinfo))
        return nodeinfo

    def get(self, name, node):
        if node == nullid:
            return ""
        key = self._key(name, node, "get")
        try:
            data = self._read(key)
            if data:
                return data
        except (IOError, OSError):
            pass
        # Failover to the underlying storage and update the cache
        data = self.store.get(name, node)
        self._write(key, data)
        return data

    def getdelta(self, name, node):
        revision = self.get(name, node)
        return (revision, name, nullid, self.getmeta(name, node))

    def getdeltachain(self, name, node):
        revision = self.get(name, node)
        return [(name, node, None, nullid, revision)]

    def getmeta(self, name, node):
        # TODO: We should probably cache getmeta as well
        return self.store.getmeta(name, node)

    def getmissing(self, keys):
        missing = [
            (name, node)
            for name, node in keys
            if not self._exists(self._key(name, node, "get"))
        ]
        return self.store.getmissing(missing)

    ################## Overrides ################################

    @abc.abstractmethod
    def _read(self, key):
        """Read from the cache"""

    @abc.abstractmethod
    def _write(self, key, value):
        """Write to the cache"""

    @abc.abstractmethod
    def _exists(self, key):
        """Check in the cache"""


class simplecachestore(cachestorecommon):
    def __init__(self, ui, store):
        super(simplecachestore, self).__init__(store, version=2)
        self.ui = ui
        try:
            self.simplecache = extensions.find("simplecache")
        except KeyError:
            raise error.Abort("simplecache extension must be enabled")

    def _read(self, key):
        return self.simplecache.cacheget(key, cachestoreserializer(key), self.ui)

    def _write(self, key, value):
        self.simplecache.cacheset(key, value, cachestoreserializer(key), self.ui)

    def _exists(self, key):
        # _exists is not yet implemented in simplecache
        # on server side this is only used by hooks
        return False


class vfscachestore(cachestorecommon):
    def __init__(self, store, vfs, maxcachesize, evictionrate):
        super(vfscachestore, self).__init__(store, version=2)
        self.vfs = vfs
        self.maxcachesize = maxcachesize
        self.evictionrate = evictionrate

    def _cachedirectory(self, key):
        # The given key is of the format:
        #   trees/v1/category/XX/XXXX...{38 character hash}
        # So the directory is key[:-39] which is equivalent to
        #   trees/v1/category/XX
        return key[:-39]

    def _read(self, key):
        with self.vfs(key) as f:
            return cachestoreserializer(key).deserialize(f.read())

    def _write(self, key, value):
        # Prevent the cache from getting 10% bigger than the max, by checking at
        # least once every 10% of the max size.
        checkfreq = int(self.maxcachesize * 0.1)
        checkcache = random.randint(0, checkfreq)
        if checkcache == 0:
            # Expire cache if it's too large
            try:
                cachedir = self._cachedirectory(key)
                if self.vfs.exists(cachedir):
                    entries = os.listdir(self.vfs.join(cachedir))
                    maxdirsize = self.maxcachesize / 256
                    if len(entries) > maxdirsize:
                        random.shuffle(entries)
                        evictionpercent = self.evictionrate / 100.0
                        unlink = self.vfs.tryunlink
                        for i in range(0, int(len(entries) * evictionpercent)):
                            unlink(os.path.join(cachedir, entries[i]))
            except Exception:
                pass

        with self.vfs(key, "w+", atomictemp=True) as f:
            f.write(cachestoreserializer(key).serialize(value))

    def _exists(self, key):
        return self.vfs.exists(key)


def pullbundle2extraprepare(orig, pullop, kwargs):
    repo = pullop.repo
    if treeenabled(repo.ui) and repo.ui.configbool("treemanifest", "treeonly"):
        bundlecaps = kwargs.get("bundlecaps", set())
        bundlecaps.add("treeonly")
