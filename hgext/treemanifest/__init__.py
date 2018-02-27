# __init__.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
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

    [treemanifest]
    allowedtreeroots = master,stable

Enabling `treemanifest.usecunionstore` will cause the extension to use the
native implementation of the datapack stores.

    [treemanifest]
    usecunionstore = True

Disabling `treemanifest.demanddownload` will prevent the extension from
automatically downloading trees from the server when they don't exist locally.

    [treemanifest]
    demanddownload = True

Setting `treemanifest.pullprefetchcount` to an integer N will cause the latest N
commits' manifests to be downloaded (if they aren't already).

    [treemanifest]
    pullprefetchcount = 0

`treemanifest.pullprefetchrevs` specifies a revset of commits who's trees should
be prefetched after a pull. Defaults to None.

   [treemanifest]
   pullprefetchrevs = master + stable

Setting `treemanifest.repackstartrev` and `treemanifest.repackendrev` causes `hg
repack --incremental` to only repack the revlog entries in the given range. The
default values are 0 and len(changelog) - 1, respectively.

   [treemanifest]
   repackstartrev = 0
   repackendrev = 1000

Setting `treemanifest.treeonly` to True will force all manifest reads to use the
tree format. This is useful in the final stages of a migration to treemanifest
to prevent accesses of flat manifests.

  [treemanifest]
  treeonly = True

`treemanifest.cacheserverstore` causes the treemanifest server to store a cache
of treemanifest revisions in individual files. These improve lookup speed since
we don't have to open a revlog.

  [treemanifest]
  cacheserverstore = True

`treemanifest.servermaxcachesize` the maximum number of entries in the server
cache.

  [treemanifest]
  servermaxcachesize = 1000000

`treemanifest.servercacheevictionpercent` the percent of the cache to evict
when the maximum size is hit.

  [treemanifest]
  servercacheevictionpercent = 50
"""
from __future__ import absolute_import

import abc
import hashlib
import os
import random
import shutil
import struct
import time

from mercurial.i18n import _
from mercurial.node import bin, hex, nullid
from mercurial import (
    bundle2,
    bundlerepo,
    changegroup,
    commands,
    error,
    exchange,
    extensions,
    localrepo,
    manifest,
    mdiff,
    phases,
    policy,
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

from ..extlib import cstore
from ..remotefilelog import (
    cmdtable as remotefilelogcmdtable,
    connectionpool,
    resolveprefetchopts,
    shallowrepo,
    shallowutil,
    wirepack,
)
from ..remotefilelog.contentstore import (
    manifestrevlogstore,
    unioncontentstore,
)
from ..remotefilelog.metadatastore import (
    unionmetadatastore,
)
from ..remotefilelog.datapack import (
    datapack,
    datapackstore,
    mutabledatapack,
)
from ..remotefilelog.historypack import (
    historypack,
    historypackstore,
    mutablehistorypack,
)
from ..remotefilelog.repack import (
    _computeincrementaldatapack,
    _computeincrementalhistorypack,
    _runrepack,
    _topacks,
    backgroundrepack,
)

osutil = policy.importmod(r'osutil')

cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem('treemanifest', 'sendtrees', default=False)
configitem('treemanifest', 'server', default=False)
configitem('treemanifest', 'cacheserverstore', default=True)
configitem('treemanifest', 'servermaxcachesize', default=1000000)
configitem('treemanifest', 'servercacheevictionpercent', default=50)

PACK_CATEGORY='manifests'

TREEGROUP_PARTTYPE = 'b2x:treegroup'
# Temporary part type while we migrate the arguments
TREEGROUP_PARTTYPE2 = 'b2x:treegroup2'
RECEIVEDNODE_RECORD = 'receivednodes'

# When looking for a recent manifest to consider our base during tree
# prefetches, this constant defines how far back we should search.
BASENODESEARCHMAX = 25000

try:
    xrange(0)
except NameError:
    xrange = range

def treeenabled(ui):
    return ui.config('extensions', 'treemanifest') not in (None, '!')

def uisetup(ui):
    extensions.wrapfunction(changegroup.cg1unpacker, '_unpackmanifests',
                            _unpackmanifestscg1)
    extensions.wrapfunction(changegroup.cg3unpacker, '_unpackmanifests',
                            _unpackmanifestscg3)
    extensions.wrapfunction(revlog.revlog, 'checkhash', _checkhash)

    wrappropertycache(localrepo.localrepository, 'manifestlog', getmanifestlog)

    extensions.wrapfunction(
        manifest.memmanifestctx, 'write', _writemanifestwrapper)

    extensions.wrapcommand(commands.table, 'pull', pull)

    wireproto.commands['gettreepack'] = (servergettreepack, '*')
    wireproto.wirepeer.gettreepack = clientgettreepack
    localrepo.localpeer.gettreepack = localgettreepack

    extensions.wrapfunction(repair, 'striptrees', striptrees)
    extensions.wrapfunction(repair, '_collectmanifest', _collectmanifest)
    extensions.wrapfunction(repair, 'stripmanifest', stripmanifest)
    extensions.wrapfunction(bundle2, '_addpartsfromopts', _addpartsfromopts)
    extensions.wrapfunction(bundlerepo.bundlerepository, '_handlebundle2part',
                            _handlebundle2part)
    extensions.wrapfunction(bundle2, 'getrepocaps', getrepocaps)
    _registerbundle2parts()

    extensions.wrapfunction(templatekw, 'showmanifest', showmanifest)
    templatekw.keywords['manifest'] = templatekw.showmanifest

    # Change manifest template output
    templatekw.defaulttempl['manifest'] = '{node}'

    def _wrapremotefilelog(loaded):
        if loaded:
            remotefilelogmod = extensions.find('remotefilelog')
            extensions.wrapcommand(
                remotefilelogmod.cmdtable, 'prefetch', _prefetchwrapper)
        else:
            # There is no prefetch command to wrap around. In this case, we use
            # the command table entry for prefetch in the remotefilelog to
            # define the prefetch command, wrap it, and then override it
            # completely.  This ensures that the options to the prefetch command
            # are consistent.
            cmdtable['prefetch'] = remotefilelogcmdtable['prefetch']
            extensions.wrapcommand(cmdtable, 'prefetch', _overrideprefetch)

    extensions.afterloaded('remotefilelog', _wrapremotefilelog)

def showmanifest(orig, **args):
    """Same implementation as the upstream showmanifest, but without the 'rev'
    field."""
    ctx, templ = args[r'ctx'], args[r'templ']
    mnode = ctx.manifestnode()
    if mnode is None:
        # just avoid crash, we might want to use the 'ff...' hash in future
        return

    mhex = hex(mnode)
    args = args.copy()
    args.update({r'node': mhex})
    f = templ('manifest', **args)
    return templatekw._mappable(f, None, f, lambda x: { 'node': mhex})

def getrepocaps(orig, repo, *args, **kwargs):
    caps = orig(repo, *args, **kwargs)
    if treeenabled(repo.ui):
        caps['treemanifest'] = ('True',)
    return caps

def _collectmanifest(orig, repo, striprev):
    if repo.ui.configbool("treemanifest", "treeonly"):
        return []
    return orig(repo, striprev)

def stripmanifest(orig, repo, striprev, tr, files):
    if repo.ui.configbool("treemanifest", "treeonly"):
        return
    orig(repo, striprev, tr, files)

def reposetup(ui, repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    repo.svfs.treemanifestserver = repo.ui.configbool('treemanifest', 'server')
    if repo.svfs.treemanifestserver:
        serverreposetup(repo)
    else:
        clientreposetup(repo)

    wraprepo(repo)

def clientreposetup(repo):
    repo.name = repo.ui.config('remotefilelog', 'reponame')
    if not repo.name:
        raise error.Abort(_("remotefilelog.reponame must be configured"))

    if not repo.ui.configbool('treemanifest', 'treeonly'):
        # If we're not a pure-tree repo, we must be using fastmanifest to
        # provide the hybrid manifest implementation.
        try:
            extensions.find('fastmanifest')
        except KeyError:
            raise error.Abort(_("cannot use treemanifest without fastmanifest"))

    if not util.safehasattr(repo, 'connectionpool'):
        repo.connectionpool = connectionpool.connectionpool(repo)

def wraprepo(repo):
    class treerepository(repo.__class__):
        def prefetchtrees(self, mfnodes, basemfnodes=None):
            if not treeenabled(self.ui):
                return

            mfstore = self.manifestlog.datastore
            missingentries = mfstore.getmissing(('', n) for n in mfnodes)
            mfnodes = list(n for path, n in missingentries)
            if not mfnodes:
                return

            # If we have no base nodes, scan the changelog looking for a
            # semi-recent manifest node to treat as the base.
            if not basemfnodes:
                changeloglen = len(repo.changelog) - 1
                basemfnodes = _findrecenttree(repo, changeloglen)

            self._prefetchtrees('', mfnodes, basemfnodes, [])

        def _prefetchtrees(self, rootdir, mfnodes, basemfnodes, directories):
            # If possible, use remotefilelog's more expressive fallbackpath
            fallbackpath = getfallbackpath(self)

            start = time.time()
            with self.connectionpool.get(fallbackpath) as conn:
                remote = conn.peer
                _gettrees(self, remote, rootdir, mfnodes, basemfnodes,
                          directories, start)

        def _restrictcapabilities(self, caps):
            caps = super(treerepository, self)._restrictcapabilities(caps)
            if repo.svfs.treemanifestserver:
                caps = set(caps)
                caps.add('gettreepack')
            return caps

    repo.__class__ = treerepository

def _prunesharedpacks(repo, packpath):
    """Wipe the packpath if it has too many packs in it"""
    try:
        numentries = len(os.listdir(packpath))
        # Note this is based on file count, not pack count.
        config = repo.ui.configint("packs", "maxpackfilecount")
        if config and numentries > config:
            repo.ui.warn(("purging shared treemanifest pack cache (%d entries) "
                         "-- too many files\n" % numentries))
            shutil.rmtree(packpath, True)
    except OSError:
        pass

def setuptreestores(repo, mfl):
    ui = repo.ui
    if ui.configbool('treemanifest', 'server'):
        packpath = repo.vfs.join('cache/packs/%s' % PACK_CATEGORY)

        # Data store
        datastore = cstore.datapackstore(packpath)
        revlogstore = manifestrevlogstore(repo)
        if ui.configbool("treemanifest", "cacheserverstore"):
            maxcachesize = ui.configint('treemanifest', 'servermaxcachesize')
            evictionrate = ui.configint(
                'treemanifest', 'servercacheevictionpercent')
            revlogstore = cachestore(
                revlogstore, repo.cachevfs, maxcachesize, evictionrate)

        mfl.datastore = unioncontentstore(datastore, revlogstore)

        # History store
        historystore = historypackstore(ui, packpath)
        mfl.historystore = unionmetadatastore(
            historystore,
            revlogstore,
        )
        _prunesharedpacks(repo, packpath)
        return

    usecdatapack = ui.configbool('remotefilelog', 'fastdatapack')

    if not util.safehasattr(repo, 'name'):
        repo.name = ui.config('remotefilelog', 'reponame')
    packpath = shallowutil.getcachepackpath(repo, PACK_CATEGORY)
    _prunesharedpacks(repo, packpath)

    localpackpath = shallowutil.getlocalpackpath(repo.svfs.vfs.base,
                                                 PACK_CATEGORY)

    demanddownload = ui.configbool('treemanifest', 'demanddownload', True)
    remotestore = remotetreestore(repo)
    # Data store
    if ui.configbool('treemanifest', 'usecunionstore'):
        datastore = cstore.datapackstore(packpath)
        localdatastore = cstore.datapackstore(localpackpath)
        # TODO: can't use remotedatastore with cunionstore yet
        # TODO make reportmetrics work with cstore
        mfl.datastore = cstore.uniondatapackstore([localdatastore, datastore])
    else:
        datastore = datapackstore(ui, packpath, usecdatapack=usecdatapack)
        localdatastore = datapackstore(ui, localpackpath,
                                       usecdatapack=usecdatapack)
        datastores = [datastore, localdatastore]
        if demanddownload:
            datastores.append(remotestore)

        mfl.datastore = unioncontentstore(*datastores,
                                          writestore=localdatastore)

    mfl.shareddatastores = [datastore]
    mfl.localdatastores = [localdatastore]
    mfl.ui = ui

    # History store
    sharedhistorystore = historypackstore(ui, packpath)
    localhistorystore = historypackstore(ui, localpackpath)
    mfl.sharedhistorystores = [
        sharedhistorystore
    ]
    mfl.localhistorystores = [
        localhistorystore,
    ]

    histstores = [sharedhistorystore, localhistorystore]
    if demanddownload:
        histstores.append(remotestore)

    mfl.historystore = unionmetadatastore(
        *histstores,
        writestore=localhistorystore)
    shallowutil.reportpackmetrics(ui, 'treestore', mfl.datastore,
        mfl.historystore)

    remotestore.setshared(mfl.datastore, mfl.historystore)

class basetreemanifestlog(object):
    def __init__(self):
        self._mutabledatapack = None
        self._mutablehistorypack = None

    def add(self, ui, newtree, p1tree, overridenode=None, overridep1node=None):
        """Writes the given tree into the manifestlog. If `overridenode` is
        specified, the tree root is written with that node instead of its actual
        node. If `overridep1node` is specified, the the p1 node for the root
        tree is also overriden.
        """
        if self._mutabledatapack is None:
            packpath = shallowutil.getlocalpackpath(
                    self._opener.vfs.base,
                    'manifests')
            self._mutabledatapack = mutabledatapack(ui, packpath)
            self._mutablehistorypack = mutablehistorypack(ui, packpath)

        newtreeiter = newtree.finalize(p1tree)

        dpack = self._mutabledatapack
        hpack = self._mutablehistorypack
        if overridenode is not None:
            dpack = InterceptedMutableDataPack(
                    dpack, overridenode, overridep1node)
            hpack = InterceptedMutableHistoryPack(
                    hpack, overridenode, overridep1node)

        node = overridenode
        for nname, nnode, ntext, np1text, np1, np2 in newtreeiter:
            # Not using deltas, since there aren't any other trees in
            # this pack it could delta against.
            dpack.add(nname, nnode, revlog.nullid, ntext)
            hpack.add(nname, nnode, np1, np2, revlog.nullid, '')
            if node is None and nname == "":
                node = nnode

        return node

    def commitpending(self):
        if self._mutabledatapack is not None:
            dpack = self._mutabledatapack
            hpack = self._mutablehistorypack

            dpack.close()
            hpack.close()

            self._mutabledatapack = None
            self._mutablehistorypack = None

            self.datastore.markforrefresh()
            self.historystore.markforrefresh()

    def abortpending(self):
        if self._mutabledatapack is not None:
            dpack = self._mutabledatapack
            hpack = self._mutablehistorypack

            dpack.abort()
            hpack.abort()

            self._mutabledatapack = None
            self._mutablehistorypack = None

class treemanifestlog(basetreemanifestlog, manifest.manifestlog):
    def __init__(self, opener, repo, treemanifest=False):
        basetreemanifestlog.__init__(self)
        assert treemanifest is False
        cachesize = 4

        opts = getattr(opener, 'options', None)
        if opts is not None:
            cachesize = opts.get('manifestcachesize', cachesize)
        self._treeinmem = True

        self._changelog = repo.unfiltered().changelog

        self._opener = opener
        self._revlog = manifest.manifestrevlog(opener,
                                               indexfile='00manifesttree.i',
                                               treemanifest=True)

        # A cache of the manifestctx or treemanifestctx for each directory
        self._dirmancache = {}
        self._dirmancache[''] = util.lrucachedict(cachesize)

        self.cachesize = cachesize

class treeonlymanifestlog(basetreemanifestlog):
    def __init__(self, opener, repo):
        super(treeonlymanifestlog, self).__init__()
        self._opener = opener
        self._memtrees = {}
        self._changelog = repo.unfiltered().changelog

    def __getitem__(self, node):
        return self.get('', node)

    def get(self, dir, node, verify=True):
        if dir != '':
            raise RuntimeError("native tree manifestlog doesn't support "
                               "subdir reads: (%s, %s)" % (dir, hex(node)))
        if node == nullid:
            return treemanifestctx(self, dir, node)

        memtree = self._memtrees.get((dir, node))
        if memtree is not None:
            return memtree

        store = self.datastore

        try:
            store.get(dir, node)
        except KeyError:
            raise shallowutil.MissingNodesError([(dir, node)])

        return treemanifestctx(self, dir, node)

    def addmemtree(self, node, tree, p1, p2):
        ctx = treemanifestctx(self, '', node)
        ctx._data = tree
        ctx.parents = (p1, p2)
        self._memtrees[('', node)] = ctx

    def clearcaches(self):
        self._memtrees.clear()

    def _maplinknode(self, linknode):
        """Turns a linknode into a linkrev. Only needed for revlog backed
        manifestlogs."""
        return self._changelog.rev(linknode)

    def _maplinkrev(self, linkrev):
        """Turns a linkrev into a linknode. Only needed for revlog backed
        manifestlogs."""
        return self._changelog.node(linkrev)

class hybridmanifestlog(manifest.manifestlog):
    def __init__(self, opener, repo):
        super(hybridmanifestlog, self).__init__(opener, repo)

        self._opener = opener
        self.ui = repo.ui

        self.treemanifestlog = treemanifestlog(opener, repo)
        setuptreestores(repo, self.treemanifestlog)
        self.datastore = self.treemanifestlog.datastore
        self.historystore = self.treemanifestlog.historystore

        if util.safehasattr(self.treemanifestlog, 'shareddatastores'):
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

class treemanifestctx(object):
    def __init__(self, manifestlog, dir, node):
        self._manifestlog = manifestlog
        self._dir = dir
        self._node = node
        self._data = None

    def read(self):
        if self._data is None:
            store = self._manifestlog.datastore
            if self._node != nullid:
                self._data = cstore.treemanifest(store, self._node)
            else:
                self._data = cstore.treemanifest(store)
        return self._data

    def node(self):
        return self._node

    def new(self, dir=''):
        if dir != '':
            raise RuntimeError("native tree manifestlog doesn't support "
                               "subdir creation: '%s'" % dir)

        store = self._manifestlog.datastore
        return cstore.treemanifest(store)

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

    def readdelta(self, shallow=False):
        '''Returns a manifest containing just the entries that are present
        in this manifest, but not in its p1 manifest. This is efficient to read
        if the revlog delta is already p1.

        If `shallow` is True, this will read the delta for this directory,
        without recursively reading subdirectory manifests. Instead, any
        subdirectory entry will be reported as it appears in the manifest, i.e.
        the subdirectory will be reported among files and distinguished only by
        its 't' flag.
        '''
        store = self._manifestlog.datastore
        p1, p2 = self.parents
        mf = self.read()
        if p1 == nullid:
            parentmf = cstore.treemanifest(store)
        else:
            parentmf = cstore.treemanifest(store, p1)

        if shallow:
            # This appears to only be used for changegroup creation in
            # upstream changegroup.py. Since we use pack files for all native
            # tree exchanges, we shouldn't need to implement this.
            raise NotImplemented("native trees don't support shallow "
                                 "readdelta yet")
        else:
            md = cstore.treemanifest(store)
            for f, ((n1, fl1), (n2, fl2)) in parentmf.diff(mf).iteritems():
                if n2:
                    md[f] = n2
                    if fl2:
                        md.setflag(f, fl2)
            return md

    def readfast(self, shallow=False):
        '''Calls either readdelta or read, based on which would be less work.
        readdelta is called if the delta is against the p1, and therefore can be
        read quickly.

        If `shallow` is True, it only returns the entries from this manifest,
        and not any submanifests.
        '''
        return self.readdelta(shallow=shallow)

    def find(self, key):
        return self.read().find(key)

class memtreemanifestctx(object):
    def __init__(self, manifestlog, dir=''):
        self._manifestlog = manifestlog
        self._dir = dir
        store = self._manifestlog.datastore
        self._treemanifest = cstore.treemanifest(store)

    def new(self, dir=''):
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
        p1tree = mfl[p1].read()

        node = mfl.add(mfl.ui, newtree, p1tree)
        if node is not None and util.safehasattr(mfl, 'addmemtree'):
            mfl.addmemtree(node, newtree, p1, p2)
        return node

def serverreposetup(repo):
    extensions.wrapfunction(manifest.manifestrevlog, 'addgroup',
                            _addmanifestgroup)

    def _capabilities(orig, repo, proto):
        caps = orig(repo, proto)
        caps.append('gettreepack')
        return caps

    if util.safehasattr(wireproto, '_capabilities'):
        extensions.wrapfunction(wireproto, '_capabilities', _capabilities)
    else:
        extensions.wrapfunction(wireproto, 'capabilities', _capabilities)

def _addmanifestgroup(orig, revlog, *args, **kwargs):
    isserver = False
    opts = getattr(revlog.opener, 'options', None)
    if opts is not None:
        isserver = opts.get('treemanifest-server', False)
    if isserver:
        raise error.Abort(_("cannot push commits to a treemanifest transition "
                            "server without pushrebase"))

    return orig(revlog, *args, **kwargs)

def getmanifestlog(orig, self):
    if not treeenabled(self.ui):
        return orig(self)

    if self.ui.configbool('treemanifest', 'treeonly'):
        mfl = treeonlymanifestlog(self.svfs, self)
        setuptreestores(self, mfl)
    else:
        mfl = hybridmanifestlog(self.svfs, self)

    return mfl

def _writemanifestwrapper(orig, self, tr, link, p1, p2, added, removed):
    n = orig(self, tr, link, p1, p2, added, removed)

    mfl = self._manifestlog
    if (util.safehasattr(mfl._revlog.opener, 'treemanifestserver') and
        mfl._revlog.opener.treemanifestserver):
        # Since we're adding the root flat manifest, let's add the corresponding
        # root tree manifest.
        tmfl = mfl.treemanifestlog
        _converttotree(tr, mfl, tmfl, self, link, torevlog=True)

    return n

@command('debuggentrees', [
    ('s', 'skip-allowed-roots', None,
     _('skips the check for only generating on allowed roots')),
    ('', 'verify', None,
     _('verify consistency of tree data')),
    ], _('hg debuggentrees FIRSTREV LASTREV'))
def debuggentrees(ui, repo, rev1, rev2, *args, **opts):
    rev1 = repo.revs(rev1).first()
    rev2 = repo.revs(rev2).last()

    mfrevlog = repo.manifestlog._revlog
    mfrev1 = mfrevlog.rev(repo[rev1].manifestnode())
    mfrev2 = mfrevlog.rev(repo[rev2].manifestnode()) + 1

    packpath = shallowutil.getcachepackpath(repo, PACK_CATEGORY)
    if opts.get('skip_allowed_roots', False):
        ui.setconfig('treemanifest', 'allowedtreeroots', None)
    with mutabledatapack(repo.ui, packpath) as dpack:
        with mutablehistorypack(repo.ui, packpath) as hpack:
            recordmanifest(dpack, hpack, repo, mfrev1, mfrev2,
                           verify=opts.get('verify', False))

@command('backfillmanifestrevlog', [
    ], _('hg backfillmanifestrevlog'))
def backfillmanifestrevlog(ui, repo, *args, **opts):
    """Download any missing manifest revlog entries. This is useful when
    transitioning back from a treeonly repo to a flat+tree hybrid repo."""
    fallbackpath = getfallbackpath(repo)
    with repo.connectionpool.get(fallbackpath) as conn:
        remote = conn.peer

        # _localrepo is needed for remotefilelog to work
        if util.safehasattr(remote, '_callstream'):
            remote._localrepo = repo

        cl = repo.changelog
        mfrevlog = repo.manifestlog._revlog

        # We need to download any manifests the server has that we don't. We
        # calculate that by saying we need all the public heads, and that we
        # have some of them already. This might result in extra downloading but
        # they become no-ops when attempting to be added to the revlog.
        publicheads = repo.revs('heads(public())')
        clnode = cl.node
        heads = [clnode(r) for r in publicheads]
        common = [clnode(r) for r in publicheads if
                  cl.changelogrevision(r).manifest in mfrevlog.nodemap]
        with repo.wlock(), repo.lock(), (
             repo.transaction("backfillmanifest")) as tr:
            cg = remote.getbundle('pull', common=common, heads=heads)
            bundle2.applybundle(repo, cg, tr, 'pull', remote.url())

@command('backfilltree', [
    ('l', 'limit', '10000000', _(''))
    ], _('hg backfilltree [OPTIONS]'))
def backfilltree(ui, repo, *args, **opts):
    with repo.wlock(), repo.lock(), repo.transaction('backfilltree') as tr:
        start, end = _getbackfillrange(repo, int(opts.get('limit')))
        if start <= end:
            mfl = repo.manifestlog
            tmfl = mfl.treemanifestlog
            revs = xrange(start, end)
            _backfilltree(tr, repo, mfl, tmfl, revs)

def _getbackfillrange(repo, limit):
    treerevlog = repo.manifestlog.treemanifestlog._revlog
    maxrev = len(treerevlog) - 1
    start = treerevlog.linkrev(maxrev) + 1

    numclrevs = len(repo.changelog)
    end = min(numclrevs, start + limit)
    return (start, end)

def _backfilltree(tr, repo, mfl, tmfl, revs):
    ui = repo.ui
    converting = _("converting flat manifest to tree manifest")
    ui.progress(converting, 0, total=len(revs))
    count = 0
    for rev in revs:
        ui.progress(converting, count, total=len(revs))
        count += 1

        _converttotree(tr, mfl, tmfl, repo[rev].manifestctx(),
                       torevlog=True)

    ui.progress(converting, None)

def _converttotree(tr, mfl, tmfl, mfctx, linkrev=None, torevlog=False):
    p1node, p2node = mfctx.parents
    newflat = mfctx.read()
    if p1node != nullid:
        try:
            parentflat = mfl[p1node].read()
            parenttree = tmfl[p1node].read()
            # Just read p2node to verify it's actually present
            tmfl[p2node].read()
        except KeyError:
            raise error.Abort(_("unable to find parent nodes %s %s") %
                              (hex(p1node), hex(p2node)))
    else:
        parentflat = manifest.manifestdict()

        if torevlog:
            parenttree = manifest.treemanifest()
        else:
            parenttree = cstore.treemanifest(tmfl.datastore)

    newtree, added, removed = _getnewtree(newflat, parenttree, parentflat)

    linknode = mfctx.linknode
    if torevlog:
        # manifests that haven't been added to the changelog yet (and therefore
        # maplinknode returns -1) should've had their linkrev provided as an
        # argument.
        if linkrev is None:
            linkrev = mfl._maplinknode(linknode)
        assert linkrev != -1, "attempting to create manifest with null linkrev"
        _addtotreerevlog(newtree, tr, tmfl, linkrev, mfctx, added, removed)
    else:
        node = tmfl.add(mfl.ui, newtree, parenttree,
                        overridenode=mfctx.node(),
                        overridep1node=p1node)
        if node is not None and util.safehasattr(tmfl, 'addmemtree'):
            tmfl.addmemtree(node, newtree, p1node, p2node)

def _getnewtree(newflat, parenttree, parentflat):
    diff = parentflat.diff(newflat)

    newtree = parenttree.copy()
    added = []
    removed = []
    for filename, (old, new) in diff.iteritems():
        if new is not None and new[0] is not None:
            added.append(filename)
            newtree[filename] = new[0]
            newtree.setflag(filename, new[1])
        else:
            removed.append(filename)
            del newtree[filename]

    return (newtree, added, removed)

def _addtotreerevlog(newtree, tr, tmfl, linkrev, mfctx, added, removed):
    try:
        p1node, p2node = mfctx.parents
        treerevlog = tmfl._revlog
        oldaddrevision = treerevlog.addrevision
        def addusingnode(*args, **kwargs):
            newkwargs = kwargs.copy()
            newkwargs['node'] = mfctx.node()
            return oldaddrevision(*args, **newkwargs)
        treerevlog.addrevision = addusingnode
        def readtree(dir, node):
            return tmfl.get(dir, node).read()
        treerevlog.add(newtree, tr, linkrev, p1node, p2node, added, removed,
                       readtree=readtree)
    finally:
        del treerevlog.__dict__['addrevision']

def _unpackmanifestscg3(orig, self, repo, *args, **kwargs):
    if not treeenabled(repo.ui):
        return orig(self, repo, *args, **kwargs)

    if repo.ui.configbool('treemanifest', 'treeonly'):
        self.manifestheader()
        for delta in self.deltaiter():
            pass
        # Handle sub-tree manifests
        for chunkdata in iter(self.filelogheader, {}):
            for delta in self.deltaiter():
                pass
        return
    return orig(self, repo, *args, **kwargs)

def _unpackmanifestscg1(orig, self, repo, *args, **kwargs):
    if not treeenabled(repo.ui):
        return orig(self, repo, *args, **kwargs)

    if repo.ui.configbool('treemanifest', 'treeonly'):
        self.manifestheader()
        for chunkdata in self.deltaiter():
            pass
        return

    mfrevlog = repo.manifestlog._revlog
    oldtip = len(mfrevlog)

    orig(self, repo, *args, **kwargs)

    if (util.safehasattr(repo.manifestlog, "datastore") and
        repo.ui.configbool('treemanifest', 'autocreatetrees')):

        # TODO: only put in cache if pulling from main server
        packpath = shallowutil.getcachepackpath(repo, PACK_CATEGORY)
        with mutabledatapack(repo.ui, packpath) as dpack:
            with mutablehistorypack(repo.ui, packpath) as hpack:
                recordmanifest(dpack, hpack, repo, oldtip, len(mfrevlog))

        # Alert the store that there may be new packs
        repo.manifestlog.datastore.markforrefresh()

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

    def add(self, filename, node, p1, p2, linknode, copyfrom):
        # For the root node, provide the flat manifest as the key
        if filename == "":
            node = self._node
            if p1 != nullid:
                p1 = self._p1node
        self._pack.add(filename, node, p1, p2, linknode, copyfrom)

def recordmanifest(datapack, historypack, repo, oldtip, newtip, verify=False):
    cl = repo.changelog
    mfl = repo.manifestlog
    mfrevlog = mfl._revlog
    total = newtip - oldtip
    ui = repo.ui
    builttrees = {}
    message = _('priming tree cache')
    ui.progress(message, 0, total=total)

    refcount = {}
    for rev in xrange(oldtip, newtip):
        p1 = mfrevlog.parentrevs(rev)[0]
        p1node = mfrevlog.node(p1)
        refcount[p1node] = refcount.get(p1node, 0) + 1

    allowedtreeroots = set()
    for name in repo.ui.configlist('treemanifest', 'allowedtreeroots'):
        if name in repo:
            allowedtreeroots.add(repo[name].manifestnode())

    includedentries = set()
    for rev in xrange(oldtip, newtip):
        ui.progress(message, rev - oldtip, total=total)
        p1, p2 = mfrevlog.parentrevs(rev)
        p1node = mfrevlog.node(p1)
        p2node = mfrevlog.node(p2)
        linkrev = mfrevlog.linkrev(rev)
        linknode = cl.node(linkrev)

        if p1node == nullid:
            origtree = cstore.treemanifest(mfl.datastore)
        elif p1node in builttrees:
            origtree = builttrees[p1node]
        else:
            origtree = mfl[p1node].read()._treemanifest()

        if origtree is None:
            if allowedtreeroots and p1node not in allowedtreeroots:
                continue

            p1mf = mfl[p1node].read()
            p1linknode = cl.node(mfrevlog.linkrev(p1))
            origtree = cstore.treemanifest(mfl.datastore)
            for filename, node, flag in p1mf.iterentries():
                origtree.set(filename, node, flag)

            tempdatapack = InterceptedMutableDataPack(datapack, p1node, nullid)
            temphistorypack = InterceptedMutableHistoryPack(historypack, p1node,
                                                            nullid)
            for nname, nnode, ntext, np1text, np1, np2 in origtree.finalize():
                # No need to compute a delta, since we know the parent isn't
                # already a tree.
                tempdatapack.add(nname, nnode, nullid, ntext)
                temphistorypack.add(nname, nnode, np1, np2, p1linknode, '')
                includedentries.add((nname, nnode))

            builttrees[p1node] = origtree

        # Remove the tree from the cache once we've processed its final use.
        # Otherwise memory explodes
        p1refcount = refcount[p1node] - 1
        if p1refcount == 0:
            builttrees.pop(p1node, None)
        refcount[p1node] = p1refcount

        if p2node != nullid:
            node = mfrevlog.node(rev)
            diff = mfl[p1node].read().diff(mfl[node].read())
            deletes = []
            adds = []
            for filename, ((anode, aflag), (bnode, bflag)) in diff.iteritems():
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
                    block = ''
                    # Deltas are of the form:
                    #   <start><end><datalen><data>
                    # Where start and end say what bytes to delete, and data
                    # says what bytes to insert in their place. So we can just
                    # read <data> to figure out all the added files.
                    byte1, byte2, blocklen = struct.unpack(">lll",
                            delta[current:current + 12])
                    current += 12
                    if blocklen:
                        block = delta[current:current + blocklen]
                        current += blocklen
                except struct.error:
                    raise RuntimeError("patch cannot be decoded")

                # An individual delta block may contain multiple newline
                # delimited entries.
                for line in block.split('\n'):
                    if not line:
                        continue
                    fname, rest = line.split('\0')
                    fnode = rest[:40]
                    fflag = rest[40:]
                    adds.append((fname, bin(fnode), fflag))

            allfiles = set(repo.changelog.readfiles(linkrev))
            deletes = allfiles.difference(fname for fname, fnode, fflag in adds)

        # Apply the changes on top of the parent tree
        newtree = origtree.copy()
        for fname in deletes:
            newtree.set(fname, None, None)

        for fname, fnode, fflags in adds:
            newtree.set(fname, fnode, fflags)

        tempdatapack = InterceptedMutableDataPack(datapack, mfrevlog.node(rev),
                                                  p1node)
        temphistorypack = InterceptedMutableHistoryPack(historypack,
                                                        mfrevlog.node(rev),
                                                        p1node)
        mfdatastore = mfl.datastore
        newtreeiter = newtree.finalize(origtree if p1node != nullid else None)
        for nname, nnode, ntext, np1text, np1, np2 in newtreeiter:
            if verify:
                # Verify all children of the tree already exist in the store
                # somewhere.
                lines = ntext.split('\n')
                for line in lines:
                    if not line:
                        continue
                    childname, nodeflag = line.split('\0')
                    childpath = os.path.join(nname, childname)
                    cnode = nodeflag[:40]
                    cflag = nodeflag[40:]
                    if (cflag == 't' and
                        (childpath + '/', bin(cnode)) not in includedentries and
                        mfdatastore.getmissing([(childpath, bin(cnode))])):
                        import pdb
                        pdb.set_trace()

            # Only use deltas if the delta base is in this same pack file
            if np1 != nullid and (nname, np1) in includedentries:
                delta = mdiff.textdiff(np1text, ntext)
                deltabase = np1
            else:
                delta = ntext
                deltabase = nullid
            tempdatapack.add(nname, nnode, deltabase, delta)
            temphistorypack.add(nname, nnode, np1, np2, linknode, '')
            includedentries.add((nname, nnode))

        if ui.configbool('treemanifest', 'verifyautocreate', False):
            diff = newtree.diff(origtree)
            for fname in deletes:
                fdiff = diff.get(fname)
                if fdiff is None:
                    import pdb
                    pdb.set_trace()
                else:
                    l, r = fdiff
                    if l != (None, ''):
                        import pdb
                        pdb.set_trace()

            for fname, fnode, fflags in adds:
                fdiff = diff.get(fname)
                if fdiff is None:
                    # Sometimes adds are no-ops, so they don't show up in the
                    # diff.
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

    ui.progress(message, None)

def _checkhash(orig, self, *args, **kwargs):
    # Don't validate root hashes during the transition to treemanifest
    if self.indexfile.endswith('00manifesttree.i'):
        return
    return orig(self, *args, **kwargs)

def wrappropertycache(cls, propname, wrapper):
    """Wraps a filecache property. These can't be wrapped using the normal
    wrapfunction. This should eventually go into upstream Mercurial.
    """
    assert callable(wrapper)
    for currcls in cls.__mro__:
        if propname in currcls.__dict__:
            origfn = currcls.__dict__[propname].func
            assert callable(origfn)
            def wrap(*args, **kwargs):
                return wrapper(origfn, *args, **kwargs)
            currcls.__dict__[propname].func = wrap
            break

    if currcls is object:
        raise AttributeError(_("%s has no property '%s'") %
                             (type(currcls), propname))

# Wrapper around the 'prefetch' command which also allows for prefetching the
# trees along with the files.
def _prefetchwrapper(orig, ui, repo, *pats, **opts):
    # The wrapper will take care of the repacking.
    repackrequested = opts.pop('repack')

    _prefetchonlytrees(repo, opts)
    _prefetchonlyfiles(orig, ui, repo, *pats, **opts)

    if repackrequested:
        backgroundrepack(repo, incremental=True)

# Wrapper around the 'prefetch' command which overrides the command completely
# and only allows for prefetching trees. This is only required when the
# 'prefetch' command is not available because the remotefilelog extension is not
# loaded and we want to be able to at least prefetch trees. The wrapping just
# ensures that we get a consistent interface to the 'prefetch' command.
def _overrideprefetch(orig, ui, repo, *pats, **opts):
    if opts.get('repack'):
        raise error.Abort(_('repack requires remotefilelog extension'))

    _prefetchonlytrees(repo, opts)

def _prefetchonlyfiles(orig, ui, repo, *pats, **opts):
    if shallowrepo.requirement in repo.requirements:
        orig(ui, repo, *pats, **opts)

def _prefetchonlytrees(repo, opts):
    opts = resolveprefetchopts(repo.ui, opts)
    revs = scmutil.revrange(repo, opts.get('rev'))

    # No trees need to be downloaded for the non-public commits.
    spec = revsetlang.formatspec('%ld & public()', revs)
    mfnodes = set(ctx.manifestnode() for ctx in repo.set(spec))

    basemfnode = set()
    base = opts.get('base')
    if base is not None:
        basemfnode.add(repo[base].manifestnode())

    repo.prefetchtrees(mfnodes, basemfnodes=basemfnode)

def _gettrees(repo, remote, rootdir, mfnodes, basemfnodes, directories, start):
    if 'gettreepack' not in shallowutil.peercapabilities(remote):
        raise error.Abort(_("missing gettreepack capability on remote"))
    remote.ui.pushbuffer()
    bundle = remote.gettreepack(rootdir, mfnodes, basemfnodes, directories)

    try:
        op = bundle2.processbundle(repo, bundle, None)

        receivednodes = op.records[RECEIVEDNODE_RECORD]
        count = 0
        missingnodes = set(mfnodes)
        for reply in receivednodes:
            missingnodes.difference_update(n for d, n
                                           in reply
                                           if d == rootdir)
            count += len(reply)
        if op.repo.ui.configbool("remotefilelog", "debug"):
            op.repo.ui.warn(_("%s trees fetched over %0.2fs\n") %
                            (count, time.time() - start))

        if missingnodes:
            raise shallowutil.MissingNodesError(
                (('', n) for n in missingnodes),
                'tree nodes missing from server response')
    except bundle2.AbortFromPart as exc:
        repo.ui.debug('remote: abort: %s\n' % exc)
        # Give stderr some time to reach the client, so we can read it into the
        # currently pushed ui buffer, instead of it randomly showing up in a
        # future ui read.
        time.sleep(0.1)
        raise shallowutil.MissingNodesError((('', n) for n in mfnodes),
                                            hint=exc.hint)
    except error.BundleValueError as exc:
        raise error.Abort(_('missing support for %s') % exc)
    finally:
        if util.safehasattr(remote, '_readerr'):
            remote._readerr()
        output = remote.ui.popbuffer()
        if output:
            repo.ui.debug(output)

def _registerbundle2parts():
    @bundle2.parthandler(TREEGROUP_PARTTYPE2, ('version', 'cache', 'category'))
    def treeparthandler2(op, part):
        """Handles received tree packs. If `cache` is True, the received
        data goes in to the shared pack cache. Otherwise, the received data
        goes into the permanent repo local data.
        """
        repo = op.repo

        version = part.params.get('version')
        if version != '1':
            raise error.Abort(
                _("unknown treegroup bundle2 part version: %s") % version)

        category = part.params.get('category', '')
        if category != PACK_CATEGORY:
            raise error.Abort(_("invalid treegroup pack category: %s") %
                              category)

        # Treemanifest servers don't accept tree directly. They must go through
        # pushrebase, which uses it's own part type and handler.
        if repo.svfs.treemanifestserver:
            return

        if part.params.get('cache', 'False') == 'True':
            packpath = shallowutil.getcachepackpath(repo, PACK_CATEGORY)
        else:
            packpath = shallowutil.getlocalpackpath(repo.svfs.vfs.base,
                                                    PACK_CATEGORY)
        receivedhistory, receiveddata = wirepack.receivepack(repo.ui, part,
                                                             packpath)

        op.records.add(RECEIVEDNODE_RECORD, receiveddata)

    @bundle2.parthandler(TREEGROUP_PARTTYPE, ('version', 'treecache'))
    def treeparthandler(op, part):
        treecache = part.params.pop('treecache')
        part.params['cache'] = treecache
        part.params['category'] = PACK_CATEGORY
        return treeparthandler2(op, part)

    @exchange.b2partsgenerator(TREEGROUP_PARTTYPE)
    def gettreepackpart(pushop, bundler):
        # We no longer generate old tree groups
        pass

    @exchange.b2partsgenerator(TREEGROUP_PARTTYPE2)
    def gettreepackpart2(pushop, bundler):
        """add parts containing trees being pushed"""
        if ('treepack' in pushop.stepsdone or
            not treeenabled(pushop.repo.ui)):
            return
        pushop.stepsdone.add('treepack')

        # Only add trees if we have them
        if _cansendtrees(pushop.repo, pushop.outgoing.missing):
            part = createtreepackpart(pushop.repo, pushop.outgoing,
                                      TREEGROUP_PARTTYPE2)
            bundler.addpart(part)

    @exchange.getbundle2partsgenerator(TREEGROUP_PARTTYPE2)
    def _getbundlechangegrouppart(bundler, repo, source, bundlecaps=None,
                                  b2caps=None, heads=None, common=None,
                                  **kwargs):
        """add parts containing trees being pulled"""
        if ('True' not in b2caps.get('treemanifest', []) or
            not treeenabled(repo.ui) or
            repo.svfs.treemanifestserver or
            not kwargs.get('cg', True)):
            return

        outgoing = exchange._computeoutgoing(repo, heads, common)
        if _cansendtrees(repo, outgoing.missing):
            part = createtreepackpart(repo, outgoing, TREEGROUP_PARTTYPE2)
            bundler.addpart(part)

def _cansendtrees(repo, nodes):
    sendtrees = repo.ui.configbool('treemanifest', 'sendtrees')
    if not sendtrees:
        return False

    repo.prefetchtrees(repo[node].manifestnode() for node in nodes)
    return True

def createtreepackpart(repo, outgoing, partname):
    rootdir = ''
    mfnodes = []
    basemfnodes = []
    directories = []

    for node in outgoing.missing:
        mfnode = repo[node].manifestnode()
        mfnodes.append(mfnode)
    basectxs = repo.set('parents(roots(%ln))', outgoing.missing)
    for basectx in basectxs:
        basemfnodes.append(basectx.manifestnode())

    packstream = generatepackstream(repo, rootdir, mfnodes,
                                    basemfnodes, directories)
    part = bundle2.bundlepart(
        partname,
        data = packstream)
    part.addparam('version', '1')
    part.addparam('cache', 'False')
    part.addparam('category', PACK_CATEGORY)

    return part

def getfallbackpath(repo):
    if util.safehasattr(repo, 'fallbackpath'):
        return repo.fallbackpath
    else:
        path = repo.ui.config('paths', 'default')
        if not path:
            raise error.Abort(
                "no remote server configured to fetch trees from")
        return path

def pull(orig, ui, repo, *pats, **opts):
    # If we're not in treeonly mode, and we're missing public commits from the
    # revlog, backfill them.
    if not ui.configbool('treemanifest', 'treeonly'):
        tippublicrevs = repo.revs('last(public())')
        if tippublicrevs:
            ctx = repo[tippublicrevs.first()]
            mfnode = ctx.manifestnode()
            mfrevlog = repo.manifestlog._revlog
            if mfnode not in mfrevlog.nodemap:
                ui.status(_("backfilling missing flat manifests\n"))
                backfillmanifestrevlog(ui, repo)

    result = orig(ui, repo, *pats, **opts)
    if treeenabled(repo.ui):
        _postpullprefetch(ui, repo)
    return result

def _postpullprefetch(ui, repo):
    repo = repo.unfiltered()

    ctxs = []
    mfstore = repo.manifestlog.datastore

    # prefetch if it's configured
    prefetchcount = ui.configint('treemanifest', 'pullprefetchcount', None)
    if prefetchcount:
        # Calculate what recent manifests are we missing
        firstrev = max(0, repo['tip'].rev() - prefetchcount + 1)
        ctxs.extend(repo.set('%s: & public()', firstrev))

    # Prefetch specific commits
    prefetchrevs = ui.config('treemanifest', 'pullprefetchrevs', None)
    if prefetchrevs:
        ctxs.extend(repo.set(prefetchrevs))

    mfnodes = None
    if ctxs:
        mfnodes = list(c.manifestnode() for c in ctxs)

    if mfnodes:
        ui.status(_("prefetching trees\n"))
        # Calculate which parents we already have
        ctxnodes = list(ctx.node() for ctx in ctxs)
        parentctxs = repo.set('parents(%ln) - %ln',
                              ctxnodes, ctxnodes)
        basemfnodes = set(ctx.manifestnode() for ctx in parentctxs)
        missingbases = list(mfstore.getmissing(('', n) for n in basemfnodes))
        basemfnodes.difference_update(n for k, n in missingbases)

        repo.prefetchtrees(mfnodes, basemfnodes=basemfnodes)

def _findrecenttree(repo, startrev):
    cl = repo.changelog
    mfstore = repo.manifestlog.datastore
    phasecache = repo._phasecache
    maxrev = min(len(cl) - 1, startrev + BASENODESEARCHMAX)
    minrev = max(0, startrev - BASENODESEARCHMAX)

    # Look up and down from the given rev
    phase = phasecache.phase
    walksize = max(maxrev - startrev, startrev - minrev) + 1
    for offset in xrange(0, walksize):
        revs = []
        uprev = startrev + offset
        downrev = startrev - offset
        if uprev <= maxrev:
            revs.append(uprev)
        if downrev >= minrev:
            revs.append(downrev)
        for rev in revs:
            if phase(repo, rev) != phases.public:
                continue
            mfnode = cl.changelogrevision(rev).manifest
            missing = mfstore.getmissing([('', mfnode)])
            if not missing:
                return [mfnode]

    return []

def clientgettreepack(remote, rootdir, mfnodes, basemfnodes, directories):
    opts = {}
    opts['rootdir'] = rootdir
    opts['mfnodes'] = wireproto.encodelist(mfnodes)
    opts['basemfnodes'] = wireproto.encodelist(basemfnodes)
    opts['directories'] = ','.join(wireproto.escapearg(d) for d in directories)

    f = remote._callcompressable("gettreepack", **opts)
    return bundle2.getunbundler(remote.ui, f)

def localgettreepack(remote, rootdir, mfnodes, basemfnodes, directories):
    bundler = _gettreepack(remote._repo, rootdir, mfnodes, basemfnodes,
                           directories)
    chunks = bundler.getchunks()
    cb = util.chunkbuffer(chunks)
    return bundle2.getunbundler(remote._repo.ui, cb)

class treememoizer(object):
    """A class that keeps references to trees until they've been consumed the
    expected number of times.
    """
    def __init__(self, store):
        self._store = store
        self._counts = {}
        self._cache = {}

    def adduse(self, node):
        self._counts[node] = self._counts.get(node, 0) + 1

    def get(self, node):
        tree = self._cache.get(node)
        if tree is None:
            tree = cstore.treemanifest(self._store, node)
            self._cache[node] = tree

        count = self._counts.get(node, 1)
        count -= 1
        self._counts[node] = max(count, 0)
        if count <= 0:
            del self._cache[node]

        return tree

def servergettreepack(repo, proto, args):
    """A server api for requesting a pack of tree information.
    """
    if shallowrepo.requirement in repo.requirements:
        raise error.Abort(_('cannot fetch remote files from shallow repo'))
    if not isinstance(proto, sshserver.sshserver):
        raise error.Abort(_('cannot fetch remote files over non-ssh protocol'))

    rootdir = args['rootdir']

    # Sort to produce a consistent output
    mfnodes = sorted(wireproto.decodelist(args['mfnodes']))
    basemfnodes = sorted(wireproto.decodelist(args['basemfnodes']))
    directories = sorted(list(wireproto.unescapearg(d) for d
                              in args['directories'].split(',') if d != ''))

    bundler = _gettreepack(repo, rootdir, mfnodes, basemfnodes, directories)
    return wireproto.streamres(gen=bundler.getchunks(), v1compressible=True)

def _gettreepack(repo, rootdir, mfnodes, basemfnodes, directories):
    try:
        bundler = bundle2.bundle20(repo.ui)
        packstream = generatepackstream(repo, rootdir, mfnodes,
                                        basemfnodes, directories)
        part = bundler.newpart(TREEGROUP_PARTTYPE2, data=packstream)
        part.addparam('version', '1')
        part.addparam('cache', 'True')
        part.addparam('category', PACK_CATEGORY)

    except error.Abort as exc:
        # cleanly forward Abort error to the client
        bundler = bundle2.bundle20(repo.ui)
        manargs = [('message', str(exc))]
        advargs = []
        if exc.hint is not None:
            advargs.append(('hint', exc.hint))
        bundler.addpart(bundle2.bundlepart('error:abort',
                                           manargs, advargs))

    return bundler

def generatepackstream(repo, rootdir, mfnodes, basemfnodes, directories):
    """
    All size/len/counts are network order unsigned ints.

    Request args:

    `rootdir` - The directory of the tree to send (including its children)
    `mfnodes` - The manifest nodes of the specified root directory to send.
    `basemfnodes` - The manifest nodes of the specified root directory that are
    already on the client.
    `directories` - The fullpath (not relative path) of directories underneath
    the rootdir that should be sent.

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
    if directories:
        raise RuntimeError("directories arg is not supported yet ('%s')" %
                            ', '.join(directories))

    historystore = repo.manifestlog.historystore
    datastore = repo.manifestlog.datastore

    # If asking for a sub-tree, start from the top level tree since the native
    # treemanifest currently doesn't support
    if rootdir != '':
        mfrevlog = repo.manifestlog.treemanifestlog._revlog.dirlog(rootdir)
        cl = repo.changelog
        topnodes = []
        for node in mfnodes:
            clrev = mfrevlog.linkrev(mfrevlog.rev(node))
            topnode = cl.changelogrevision(clrev).manifest
            topnodes.append(topnode)
        mfnodes = topnodes
        rootdir = ''

        # Since the native treemanifest implementation currently doesn't support
        # sub-tree traversals, we can't do base node comparisons correctly.
        basemfnodes = []

    # Only use the first two base trees, since the current tree
    # implementation cannot handle more yet.
    basemfnodes = basemfnodes[:2]

    mfnodeset = set(mfnodes)
    basemfnodeset = set(basemfnodes)

    # Count how many times we will need each comparison node, so we can keep
    # trees in memory the appropriate amount of time.
    trees = treememoizer(datastore)
    prevmfnode = None
    for node in mfnodes:
        p1node, p2node = historystore.getnodeinfo(rootdir, node)[:2]
        if p1node != nullid and (p1node in mfnodeset or
                                 p1node in basemfnodeset):
            trees.adduse(p1node)
        elif basemfnodes:
            for basenode in basemfnodes:
                trees.adduse(basenode)
        elif prevmfnode:
            # If there are no base nodes and the parent isn't one of the
            # requested mfnodes, then pick another mfnode as a base.
            trees.adduse(prevmfnode)

        prevmfnode = node
        if p2node != nullid and (p2node in mfnodeset or
                                 p2node in basemfnodeset):
            trees.adduse(p2node)

    prevmfnode = None
    for node in mfnodes:
        treemf = trees.get(node)

        p1node, p2node = historystore.getnodeinfo(rootdir, node)[:2]
        # If p1 is being sent or is already on the client, chances are
        # that's the best thing for us to delta against.
        if p1node != nullid and (p1node in mfnodeset or
                                 p1node in basemfnodeset):
            basetrees = [trees.get(p1node)]
        elif basemfnodes:
            basetrees = [trees.get(basenode) for basenode in basemfnodes]
        elif prevmfnode:
            # If there are no base nodes and the parent isn't one of the
            # requested mfnodes, then pick another mfnode as a base.
            basetrees = [trees.get(prevmfnode)]
        else:
            basetrees = []
        prevmfnode = node

        if p2node != nullid and (p2node in mfnodeset or
                                 p2node in basemfnodeset):
            basetrees.append(trees.get(p2node))

        subtrees = treemf.walksubtrees(comparetrees=basetrees)
        for subname, subnode, subtext, x, x, x in subtrees:
            # Append data
            data = [(subnode, nullid, subtext)]

            # Append history
            # Only append first history for now, since the entire manifest
            # history is very long.
            # Append data
            data = [(subnode, nullid, subtext)]

            # Append history
            histdata = historystore.getnodeinfo(subname, subnode)
            p1node, p2node, linknode, copyfrom = histdata
            history = [(subnode, p1node, p2node, linknode, copyfrom)]

            for chunk in wirepack.sendpackpart(subname, history, data):
                yield chunk

    yield wirepack.closepart()

class generatingdatastore(object):
    """Abstract base class representing stores which generate trees on the
    fly and write them to the shared store. Thereafter, the stores replay the
    lookup operation on the shared store expecting it to succeed."""
    # Make this an abstract class, so it cannot be instantiated on its own.
    __metaclass__ = abc.ABCMeta

    def __init__(self, repo):
        self._repo = repo
        self._shareddata = None

    def setshared(self, shareddata, sharedhistory):
        self._shareddata = shareddata
        self._sharedhistory = sharedhistory

    @abc.abstractmethod
    def _generatetrees(self, name, node):
        pass

    def get(self, name, node):
        self._generatetrees(name, node)
        return self._shareddata.get(name, node)

    def getdeltachain(self, name, node):
        self._generatetrees(name, node)
        return self._shareddata.getdeltachain(name, node)

    def add(self, name, node, data):
        raise RuntimeError("cannot add to a generating store")

    def getmissing(self, keys):
        return keys

    def markledger(self, ledger, options=None):
        pass

    def getmetrics(self):
        return {}

    def getancestors(self, name, node, known=None):
        self._generatetrees(name, node)
        return self._sharedhistory.getancestors(name, node, known=known)

    def getnodeinfo(self, name, node):
        self._generatetrees(name, node)
        return self._sharedhistory.getnodeinfo(name, node)

class remotetreestore(generatingdatastore):
    def _generatetrees(self, name, node):
        # Only look at the server if not root or is public
        basemfnodes = []
        if name == '':
            if util.safehasattr(self._repo.manifestlog, '_revlog'):
                mfrevlog = self._repo.manifestlog._revlog
                rev = mfrevlog.rev(node)
                linkrev = mfrevlog.linkrev(rev)
                if self._repo[linkrev].phase() != phases.public:
                    raise KeyError((name, node))
            else:
                # TODO: improve linkrev guessing when the revlog isn't available
                linkrev = self._repo['tip'].rev()

            # Find a recent tree that we already have
            basemfnodes = _findrecenttree(self._repo, linkrev)

        self._repo._prefetchtrees(name, [node], basemfnodes, [])
        self._shareddata.markforrefresh()
        self._sharedhistory.markforrefresh()

def serverrepack(repo, incremental=False, options=None):
    packpath = repo.vfs.join('cache/packs/%s' % PACK_CATEGORY)

    revlogstore = manifestrevlogstore(repo)

    try:
        files = osutil.listdir(packpath, stat=True)
    except OSError:
        files = []

    # Data store
    fulldatapackstore = datapackstore(repo.ui, packpath)
    if incremental:
        datastores = _topacks(packpath,
            _computeincrementaldatapack(repo.ui, files),
            datapack)
    else:
        datastores = [fulldatapackstore]
    datastores.append(revlogstore)
    datastore = unioncontentstore(*datastores)

    # History store
    if incremental:
        historystores = _topacks(packpath,
            _computeincrementalhistorypack(repo.ui, files),
            historypack)
    else:
        historystores = [historypackstore(repo.ui, packpath)]
    historystores.append(revlogstore)
    histstore = unionmetadatastore(*historystores)

    startrev = repo.ui.configint('treemanifest', 'repackstartrev', 0)
    endrev = repo.ui.configint('treemanifest', 'repackendrev',
                               len(repo.changelog) - 1)
    if startrev == 0 and incremental:
        latestpackedlinkrev = 0
        mfrevlog = repo.manifestlog.treemanifestlog._revlog
        for i in xrange(len(mfrevlog) - 1, 0, -1):
            node = mfrevlog.node(i)
            if not fulldatapackstore.getmissing([('', node)]):
                latestpackedlinkrev = mfrevlog.linkrev(i)
                break
        startrev = latestpackedlinkrev + 1

    revlogstore.setrepacklinkrevrange(startrev, endrev)
    _runrepack(repo, datastore, histstore, packpath, PACK_CATEGORY,
        options=options)

def striptrees(orig, repo, tr, striprev, files):
    if not treeenabled(repo.ui):
        return orig(repo, tr, striprev, files)

    if repo.ui.configbool('treemanifest', 'server'):
        treerevlog = repo.manifestlog.treemanifestlog._revlog
        for dir in util.dirs(files):
            # If the revlog doesn't exist, this returns an empty revlog and is a
            # no-op.
            rl = treerevlog.dirlog(dir)
            rl.strip(striprev, tr)

        treerevlog.strip(striprev, tr)

def _addpartsfromopts(orig, ui, repo, bundler, source, outgoing, opts):
    orig(ui, repo, bundler, source, outgoing, opts)

    # Only add trees if we have them
    if _cansendtrees(repo, outgoing.missing):
        part = createtreepackpart(repo, outgoing, TREEGROUP_PARTTYPE2)
        bundler.addpart(part)

def _handlebundle2part(orig, self, bundle, part):
    if part.type == TREEGROUP_PARTTYPE2:
        tempstore = wirepack.wirepackstore(part.read())

        # Point the bundle repo at the temp stores
        mfl = self.manifestlog
        mfl.datastore = unioncontentstore(
            tempstore,
            mfl.datastore)
        mfl.historystore = unionmetadatastore(
            tempstore,
            mfl.historystore)
    else:
        orig(self, bundle, part)

NODEINFOFORMAT = '!20s20s20sI'
NODEINFOLEN = struct.calcsize(NODEINFOFORMAT)
class cachestore(object):
    def __init__(self, store, vfs, maxcachesize, evictionrate, version=1):
        self.store = store
        self.vfs = vfs
        self.version = version
        self.maxcachesize = maxcachesize
        self.evictionrate = evictionrate

    def _key(self, name, node, category):
        shakey = hex(hashlib.sha1(name + node).digest())
        return os.path.join('trees', 'v' + str(self.version), category,
                            shakey[:2], shakey[2:])

    def _cachedirectory(self, key):
        # The given key is of the format:
        #   trees/v1/category/XX/XXXX...{38 character hash}
        # So the directory is key[:-39] which is equivalent to
        #   trees/v1/category/XX
        return key[:-39]

    def get(self, name, node):
        if node == nullid:
            return ''

        try:
            key = self._key(name, node, 'get')
            return self._read(key)
        except (IOError, OSError):
            data = self.store.get(name, node)
            self._write(key, data)
            return data

    def getdelta(self, name, node):
        revision = self.get(name, node)
        return (revision, name, nullid,
                self.getmeta(name, node))

    def getdeltachain(self, name, node):
        revision = self.get(name, node)
        return [(name, node, None, nullid, revision)]

    def getmeta(self, name, node):
        # TODO: We should probably cache getmeta as well
        return self.store.getmeta(name, node)

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            if not self.vfs.exists(self._key(name, node, 'get')):
                missing.append((name, node))

        return self.store.getmissing(missing)

    def getancestors(self, name, node, known=None):
        return self.store.getancestors(name, node, known=known)

    def _serializenodeinfo(self, nodeinfo):
        p1, p2, linknode, copyfrom = nodeinfo
        if copyfrom is None:
            copyfrom = ''
        raw = struct.pack(NODEINFOFORMAT, p1, p2, linknode, len(copyfrom))
        return raw + copyfrom

    def _deserializenodeinfo(self, raw):
        p1, p2, linknode, copyfromlen = struct.unpack_from(NODEINFOFORMAT, raw,
                                                           0)
        if len(raw) != NODEINFOLEN + copyfromlen:
            raise IOError("invalid nodeinfo serialization: %s %s %s %s %s" %
                          (hex(p1), hex(p2), hex(linknode), str(copyfromlen),
                           raw[NODEINFOLEN:]))
        return p1, p2, linknode, raw[NODEINFOLEN:NODEINFOLEN + copyfromlen]

    def _verifyvalue(self, value):
        sha, value = value[:20], value[20:]
        realsha = hashlib.sha1(value).digest()
        if sha != realsha:
            raise IOError()
        return value

    def _read(self, key):
        with self.vfs(key) as f:
            raw = f.read()
            if raw == '':
                raise IOError("missing file contents: %s" % self.vfs.join(key))

            sha, value = raw[:20], raw[20:]
            realsha = hashlib.sha1(value).digest()
            if sha != realsha:
                raise IOError("invalid file contents: %s" % self.vfs.join(key))
            return value

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
                        for i in xrange(0, int(len(entries) * evictionpercent)):
                            unlink(os.path.join(cachedir, entries[i]))
            except Exception:
                pass

        with self.vfs(key, 'w+', atomictemp=True) as f:
            sha = hashlib.sha1(value).digest()
            f.write(sha)
            f.write(value)

    def getnodeinfo(self, name, node):
        key = self._key(name, node, 'nodeinfo')
        try:
            raw = self._read(key)
            return self._deserializenodeinfo(raw)
        except (IOError, OSError):
            nodeinfo = self.store.getnodeinfo(name, node)
            self._write(key, self._serializenodeinfo(nodeinfo))
            return nodeinfo
