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

`treemanifest.fetchdepth` sets the default depth to fetch trees when fetching
trees from the server.

::

    [treemanifest]
    fetchdepth = 65536

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

import hashlib
import os
import struct

from bindings import manifest as rustmanifest, revisionstore
from edenscm.mercurial import (
    bundle2,
    bundlerepo,
    changegroup,
    changelog2,
    commands,
    eagerepo,
    error,
    exchange,
    extensions,
    git,
    hg,
    localrepo,
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

from .. import clienttelemetry
from ..remotefilelog import (
    cmdtable as remotefilelogcmdtable,
    resolveprefetchopts,
    shallowbundle,
    shallowrepo,
    shallowutil,
    wirepack,
)
from ..remotefilelog.contentstore import unioncontentstore
from ..remotefilelog.datapack import memdatapack
from ..remotefilelog.historypack import memhistorypack
from ..remotefilelog.metadatastore import unionmetadatastore
from ..remotefilelog.repack import domaintenancerepack


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

configitem("treemanifest", "fetchdepth", default=TREE_DEPTH_MAX)
configitem("treemanifest", "stickypushpath", default=True)
configitem("treemanifest", "http", default=True)

PACK_CATEGORY = "manifests"

TREEGROUP_PARTTYPE = "b2x:treegroup"
# Temporary part type while we migrate the arguments
TREEGROUP_PARTTYPE2 = "b2x:treegroup2"
RECEIVEDNODE_RECORD = "receivednodes"

# When looking for a recent manifest to consider our base during tree
# prefetches, this constant defines how far back we should search.
BASENODESEARCHMAX = 25000


def treeenabled(ui):
    return (
        ui.config("extensions", "treemanifest") not in (None, "!")
        or "treemanifest" in extensions.DEFAULT_EXTENSIONS
        or "treemanifest" in extensions.ALWAYS_ON_EXTENSIONS
    )


def usehttpfetching(repo):
    """Returns True if HTTP (EdenApi) fetching should be used."""
    if repo.ui.config("ui", "ssh") == "false":
        # Cannot use SSH.
        return True
    return (
        repo.ui.configbool("treemanifest", "http") and repo.nullableedenapi is not None
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
        caps["treeonly"] = ("True",)
    return caps


def _collectmanifest(orig, repo, striprev):
    if treeenabled(repo.ui):
        return []
    return orig(repo, striprev)


def stripmanifest(orig, repo, striprev, tr, files):
    if treeenabled(repo.ui):
        repair.striptrees(repo, tr, striprev, files)
        return
    orig(repo, striprev, tr, files)


def reposetup(ui, repo):
    # Update "{manifest}" again since it might be rewritten by templatekw.init.
    templatekw.defaulttempl["manifest"] = "{node}"

    if not isinstance(repo, localrepo.localrepository):
        return

    repo.svfs.treemanifestserver = False
    clientreposetup(repo)

    wraprepo(repo)


def clientreposetup(repo):
    repo.name = repo.ui.config("remotefilelog", "reponame", "unknown")


def wraprepo(repo):
    class treerepository(repo.__class__):
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

            self.manifestlog.datastore.prefetch(list(("", node) for node in mfnodes))

        def resettreefetches(self):
            fetches = self._treefetches
            self._treefetches = 0
            return fetches

        def _restrictcapabilities(self, caps):
            caps = super(treerepository, self)._restrictcapabilities(caps)
            caps.add("designatednodes")
            caps.add("gettreepack")
            caps.add("treeonly")
            return caps

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
    if git.isgitstore(repo):
        mfl._isgit = True
        mfl._iseager = False
        mfl.datastore = git.openstore(repo)
    elif eagerepo.iseagerepo(repo):
        mfl._isgit = False
        mfl._iseager = True
        mfl.datastore = repo.fileslog.contentstore
    else:
        mfl._isgit = False
        mfl._iseager = False
        mfl.makeruststore()


class basetreemanifestlog(object):
    def __init__(self, repo):
        self.recentlinknode = None
        cachesize = 4
        self._treemanifestcache = util.lrucachedict(cachesize)
        self._isgit = False

    def add(
        self,
        ui,
        newtree,
        p1node,
        p2node,
        linknode,
        tr=None,
        linkrev=None,
    ):
        """Writes the given tree into the manifestlog."""
        assert (
            not self._isgit
        ), "do not use add() for git tree, use tree.flush() instead"
        return self._addtopack(
            ui,
            newtree,
            p1node,
            p2node,
            linknode,
            linkrev=linkrev,
        )

    def _getmutablelocalpacks(self):
        """Returns a tuple containing a data pack and a history pack."""
        return (self.datastore, self.historystore)

    def getmutablesharedpacks(self):
        return (
            self.datastore.getsharedmutable(),
            self.historystore.getsharedmutable(),
        )

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
        linkrev=None,
    ):
        newtreeiter = _finalize(self, newtree, p1node, p2node)

        if self._iseager:
            # eagerepo does not have history pack but requires p1node and
            # p2node to be part of the sha1 blob.
            store = self.datastore
            rootnode = None
            for nname, nnode, ntext, _np1text, np1, np2 in newtreeiter:
                rawtext = revlog.textwithheader(ntext, np1, np2)
                node = store.add_sha1_blob(rawtext)
                assert node == nnode, f"{node} == {nnode}"
                if rootnode is None and nname == "":
                    rootnode = node
            return rootnode

        dpack, hpack = self._getmutablelocalpacks()

        node = None
        for nname, nnode, ntext, _np1text, np1, np2 in newtreeiter:
            self._addtreeentry(
                dpack, hpack, nname, nnode, ntext, np1, np2, linknode, linkrev
            )
            if node is None and nname == "":
                node = nnode

        return node

    def commitsharedpacks(self):
        """Persist the dirty trees written to the shared packs."""
        if self._isgit or self._iseager:
            return

        self.datastore.markforrefresh()
        self.historystore.markforrefresh()
        self.datastore.flush()
        self.historystore.flush()

    def commitpending(self):
        self.commitsharedpacks()

    def abortpending(self):
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
        # git store does not have the Python `.get(path, node)` method.
        # it can only be accessed via the Rust treemanifest.
        # eager store does not require remote lookup.
        if node == nullid or self._isgit or self._iseager:
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
        edenapi = repo.nullableedenapi
        if usehttpfetching(repo) and edenapi:
            return edenapi.treestore()
        return None

    def makeruststore(self):
        assert not self._iseager
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
            if self._repo.ui.configbool("scmstore", "enableshim"):
                self.datastore = self.treescmstore
            else:
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

    def writegit(self):
        newtree = self._treemanifest
        return newtree.flush()

    def write(self, tr, linkrev, p1, p2, added, removed):
        mfl = self._manifestlog
        assert not mfl._isgit, "do not use write() for git tree, use writegit() instead"

        newtree = self._treemanifest

        # linknode=None because the linkrev is provided
        node = mfl.add(
            mfl.ui,
            newtree,
            p1,
            p2,
            None,
            tr=tr,
            linkrev=linkrev,
        )
        return node


def getmanifestlog(orig, self):
    if not treeenabled(self.ui):
        return orig(self)

    mfl = treeonlymanifestlog(self.svfs, self)
    setuptreestores(self, mfl)

    return mfl


def getbundlemanifestlog(orig, self):
    mfl = orig(self)
    if not treeenabled(self.ui):
        return mfl

    wrapmfl = mfl

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
            tr=None,
            linkrev=None,
        ):
            return self._addtopack(
                ui,
                newtree,
                p1node,
                p2node,
                linknode,
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


@command("debuggetroottree", [], "NODE")
def debuggetroottree(ui, repo, rootnode):
    with ui.configoverride({("treemanifest", "fetchdepth"): "1"}, "forcesinglefetch"):
        repo.prefetchtrees([bin(rootnode)])


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


def _unpackmanifestscg3(orig, self, repo, *args, **kwargs):
    if not treeenabled(repo.ui):
        return orig(self, repo, *args, **kwargs)

    self.manifestheader()
    for chunk in self.deltaiter():
        raise error.ProgrammingError(
            "manifest deltas are not supported in a changegroup"
        )
    # Handle sub-tree manifests
    for chunkdata in iter(self.filelogheader, {}):
        raise error.ProgrammingError("sub-trees are not supported in a changegroup")


def _unpackmanifestscg1(orig, self, repo, revmap, trp, numchanges):
    if not treeenabled(repo.ui):
        return orig(self, repo, revmap, trp, numchanges)

    self.manifestheader()
    for chunk in self.deltaiter():
        raise error.ProgrammingError(
            "manifest deltas are not supported in a changegroup"
        )


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

        mfl = repo.manifestlog

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
    if "default" not in repo.ui.paths:
        return
    if git.isgitstore(repo) or eagerepo.iseagerepo(repo):
        return

    ctxs = []
    mfstore = repo.manifestlog.datastore

    # prefetch if it's configured
    prefetchcount = ui.configint("treemanifest", "pullprefetchcount", None)
    if prefetchcount:
        # Calculate what recent manifests are we missing
        ctxs.extend(repo.set("limit(sort(::tip,-rev),%s) & public()", prefetchcount))

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
        wireproto.unescapearg(d) for d in args["directories"].split(",") if d != ""
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


class remotetreestore(object):
    def __init__(self, repo):
        self._repo = repo
        self.ui = repo.ui

    def _interactivedebug(self):
        """Returns True if this is an interactive command running in debug mode."""
        return self.ui.interactive() and self.ui.configbool("remotefilelog", "debug")

    def prefetch(self, datastore, historystore, keys):
        if usehttpfetching(self._repo):
            # http fetching is handled inside the content store, so raise a
            # KeyError here.
            raise shallowutil.MissingNodesError(keys)
        else:
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
        fallbackpath = getfallbackpath(self._repo)
        with self._repo.connectionpool.get(fallbackpath) as conn:
            if "designatednodes" not in conn.peer.capabilities():
                raise error.ProgrammingError("designatednodes must be supported")

            mfnodes = [node for path, node in keys]
            directories = [path for path, node in keys]

            if self.ui.configbool("remotefilelog", "debug") and len(keys) == 1:
                name = keys[0][0]
                node = keys[0][1]
                msg = _("fetching tree %r %s") % (name, hex(node))
                self.ui.warn(msg + "\n")

            start = util.timer()
            with self.ui.timesection("getdesignatednodes"):
                _gettrees(
                    self._repo,
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


def _addpartsfromopts(orig, ui, repo, bundler, source, outgoing, *args, **kwargs):
    orig(ui, repo, bundler, source, outgoing, *args, **kwargs)

    # Only add trees to bundles for tree enabled clients. Servers use revlogs
    # and therefore will use changegroup tree storage.
    if treeenabled(repo.ui):
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


def pullbundle2extraprepare(orig, pullop, kwargs):
    repo = pullop.repo
    if treeenabled(repo.ui):
        bundlecaps = kwargs.get("bundlecaps", set())
        bundlecaps.add("treeonly")
