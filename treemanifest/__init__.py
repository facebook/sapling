# __init__.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
The treemanifest extension is to aid in the transition from flat manifests to
treemanifests. It has a client portion that's used to construct trees during
client pulls and commits, and a server portion which is used to generate
tree manifests side-by-side normal flat manifests.

Configs:

    ``treemanifest.server`` is used to indicate that this repo can serve
    treemanifests
"""

"""allows using and migrating to tree manifests

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

Setting `treemanifest.sendtrees` to True will include tree packs in sent
bundles.

    [treemanifest]
    sendtrees = False

Setting `treemanifest.repackstartrev` and `treemanifest.repackendrev` causes `hg
repack --incremental` to only repack the revlog entries in the given range. The
default values are 0 and len(changelog) - 1, respectively.

   [treemanifest]
   repackstartrev = 0
   repackendrev = 1000

Setting `treemanifest.sendflat` to False will stop flat manifests from being
sent as part of changegroups during push. It defaults to True.

   [treemanifest]
   sendflat = True

Setting `treemanifest.treeonly` to True will force all manifest reads to use the
tree format. This is useful in the final stages of a migration to treemanifest
to prevent accesses of flat manifests.

  [treemanifest]
  treeonly = True

"""

from mercurial import (
    bundle2,
    changegroup,
    commands,
    error,
    exchange,
    extensions,
    hg,
    localrepo,
    manifest,
    mdiff,
    phases,
    registrar,
    repair,
    revlog,
    sshserver,
    util,
    wireproto,
)
from mercurial.i18n import _
from mercurial.node import bin, hex, nullid

from remotefilelog.contentstore import (
    manifestrevlogstore,
    unioncontentstore,
)
from remotefilelog.metadatastore import (
    unionmetadatastore,
)
from remotefilelog.datapack import datapackstore, mutabledatapack
from remotefilelog.historypack import historypackstore, mutablehistorypack
from remotefilelog import shallowrepo, shallowutil, wirepack
from remotefilelog.repack import _runrepack
import cstore

import os
import struct
import time

cmdtable = {}
command = registrar.command(cmdtable)

PACK_CATEGORY='manifests'

TREEGROUP_PARTTYPE = 'b2x:treegroup'
# Temporary part type while we migrate the arguments
TREEGROUP_PARTTYPE2 = 'b2x:treegroup2'
RECEIVEDNODE_RECORD = 'receivednodes'

# When looking for a recent manifest to consider our base during tree
# prefetches, this constant defines how far back we should search.
BASENODESEARCHMAX = 25000

def uisetup(ui):
    extensions.wrapfunction(changegroup.cg1unpacker, '_unpackmanifests',
                            _unpackmanifests)
    extensions.wrapfunction(revlog.revlog, 'checkhash', _checkhash)

    wrappropertycache(localrepo.localrepository, 'manifestlog', getmanifestlog)

    extensions.wrapfunction(manifest.memmanifestctx, 'write', _writemanifest)

    extensions.wrapcommand(commands.table, 'pull', pull)

    wireproto.commands['gettreepack'] = (servergettreepack, '*')
    wireproto.wirepeer.gettreepack = clientgettreepack

    extensions.wrapfunction(repair, 'striptrees', striptrees)
    _registerbundle2parts()

def reposetup(ui, repo):
    wraprepo(repo)

def wraprepo(repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    repo.svfs.treemanifestserver = repo.ui.configbool('treemanifest', 'server')
    if repo.svfs.treemanifestserver:
        serverreposetup(repo)
    else:
        clientreposetup(repo)

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

def setuptreestores(repo, mfl):
    if repo.ui.configbool("treemanifest", "server"):
        packpath = repo.vfs.join('cache/packs/%s' % PACK_CATEGORY)

        # Data store
        datastore = cstore.datapackstore(packpath)
        revlogstore = manifestrevlogstore(repo)
        mfl.datastore = unioncontentstore(datastore, revlogstore)

        # History store
        historystore = historypackstore(repo.ui, packpath)
        mfl.historystore = unionmetadatastore(
            historystore,
            revlogstore,
        )

        return

    usecdatapack = repo.ui.configbool('remotefilelog', 'fastdatapack')

    packpath = shallowutil.getcachepackpath(repo, PACK_CATEGORY)

    localpackpath = shallowutil.getlocalpackpath(repo.svfs.vfs.base,
                                                 PACK_CATEGORY)

    # Data store
    if repo.ui.configbool("treemanifest", "usecunionstore"):
        datastore = cstore.datapackstore(packpath)
        localdatastore = cstore.datapackstore(localpackpath)
        # TODO: can't use remotedatastore with cunionstore yet
        mfl.datastore = cstore.uniondatapackstore([localdatastore, datastore])
    else:
        datastore = datapackstore(repo.ui, packpath, usecdatapack=usecdatapack)
        localdatastore = datapackstore(repo.ui, localpackpath,
                                       usecdatapack=usecdatapack)
        stores = [datastore, localdatastore]
        remotedatastore = remotetreedatastore(repo)
        if repo.ui.configbool("treemanifest", "demanddownload", True):
            stores.append(remotedatastore)

        mfl.datastore = unioncontentstore(*stores,
                                          writestore=localdatastore)
        remotedatastore.setshared(mfl.datastore)

    mfl.shareddatastores = [datastore]
    mfl.localdatastores = [localdatastore]

    # History store
    sharedhistorystore = historypackstore(repo.ui, packpath)
    localhistorystore = historypackstore(repo.ui, localpackpath)
    mfl.sharedhistorystores = [
        sharedhistorystore
    ]
    mfl.localhistorystores = [
        localhistorystore,
    ]
    mfl.historystore = unionmetadatastore(
        sharedhistorystore,
        localhistorystore,
        writestore=localhistorystore,
    )

class treemanifestlog(manifest.manifestlog):
    def __init__(self, opener, treemanifest=False):
        assert treemanifest is False
        cachesize = 4

        opts = getattr(opener, 'options', None)
        if opts is not None:
            cachesize = opts.get('manifestcachesize', cachesize)
        self._treeinmem = True

        self._opener = opener
        self._revlog = manifest.manifestrevlog(opener,
                                               indexfile='00manifesttree.i',
                                               treemanifest=True)

        # A cache of the manifestctx or treemanifestctx for each directory
        self._dirmancache = {}
        self._dirmancache[''] = util.lrucachedict(cachesize)

        self.cachesize = cachesize

class treeonlymanifestlog(object):
    def __init__(self, opener):
        self._opener = opener

    def __getitem__(self, node):
        return self.get('', node)

    def get(self, dir, node, verify=True):
        if dir != '':
            raise RuntimeError("native tree manifestlog doesn't support "
                               "subdir reads: (%s, %s)" % (dir, hex(node)))

        store = self.datastore
        if not store.getmissing([(dir, node)]):
            return treemanifestctx(self, dir, node)
        else:
            raise KeyError("tree node not found (%s, %s)" %
                           (dir, hex(node)))

    def clearcaches(self):
        pass

class treemanifestctx(object):
    def __init__(self, manifestlog, dir, node):
        self._manifestlog = manifestlog
        self._dir = dir
        self._node = node
        self._data = None

    def read(self):
        if self._data is None:
            store = self._manifestlog.datastore
            self._data = cstore.treemanifest(store, self._node)
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
        return self.read().copy()

    @util.propertycache
    def parents(self):
        store = self._manifestlog.historystore
        p1, p2, linkrev, copyfrom = store.getnodeinfo((self._dir, self._node))
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
        p1, p2 = self.parents()
        m1 = self.read()
        m2 = cstore.treemanifest(store, p1)

        if shallow:
            # This appears to only be used for changegroup creation in
            # upstream changegroup.py. Since we use pack files for all native
            # tree exchanges, we shouldn't need to implement this.
            raise NotImplemented("native trees don't support shallow "
                                 "readdelta yet")
        else:
            md = cstore.treemanifest(store)
            for f, ((n1, fl1), (n2, fl2)) in m1.diff(m2).iteritems():
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

def serverreposetup(repo):
    extensions.wrapfunction(manifest.manifestrevlog, 'addgroup',
                            _addmanifestgroup)

    def _capabilities(orig, repo, proto):
        caps = orig(repo, proto)
        caps.append('gettreepack')
        return caps
    extensions.wrapfunction(wireproto, '_capabilities', _capabilities)

def _addmanifestgroup(*args, **kwargs):
    raise error.Abort(_("cannot push commits to a treemanifest transition "
                        "server without pushrebase"))

def getmanifestlog(orig, self):
    if self.ui.configbool('treemanifest', 'treeonly'):
        mfl = treeonlymanifestlog(self.svfs)
        setuptreestores(self, mfl)
    else:
        mfl = orig(self)
        mfl.treemanifestlog = treemanifestlog(self.svfs)
        setuptreestores(self, mfl.treemanifestlog)
        mfl.datastore = mfl.treemanifestlog.datastore
        mfl.historystore = mfl.treemanifestlog.historystore

        if util.safehasattr(mfl.treemanifestlog, 'shareddatastores'):
            mfl.shareddatastores = mfl.treemanifestlog.shareddatastores
            mfl.localdatastores = mfl.treemanifestlog.localdatastores
            mfl.sharedhistorystores = mfl.treemanifestlog.sharedhistorystores
            mfl.localhistorystores = mfl.treemanifestlog.localhistorystores

    return mfl

def _writemanifest(orig, self, transaction, link, p1, p2, added, removed):
    n = orig(self, transaction, link, p1, p2, added, removed)

    if not self._manifestlog._revlog.opener.treemanifestserver:
        return n

    # Since we're adding the root flat manifest, let's add the corresponding
    # root tree manifest.
    mfl = self._manifestlog
    treemfl = mfl.treemanifestlog

    m = self._manifestdict

    parentflat = mfl[p1].read()
    diff = parentflat.diff(m)

    newtree = treemfl[p1].read().copy()
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

    try:
        treemfrevlog = treemfl._revlog
        oldaddrevision = treemfrevlog.addrevision
        def addusingnode(*args, **kwargs):
            newkwargs = kwargs.copy()
            newkwargs['node'] = n
            return oldaddrevision(*args, **newkwargs)
        treemfrevlog.addrevision = addusingnode

        def readtree(dir, node):
            return treemfl.get(dir, node).read()
        treemfrevlog.add(newtree, transaction, link, p1, p2, added, removed,
                         readtree=readtree)
    finally:
        del treemfrevlog.__dict__['addrevision']

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

@command('backfilltree', [
    ('l', 'limit', '10000000', _(''))
    ], _('hg backfilltree [OPTIONS]'))
def backfilltree(ui, repo, *args, **opts):
    with repo.wlock(), repo.lock(), repo.transaction('backfilltree') as tr:
        _backfill(tr, repo, int(opts.get('limit')))

def _backfill(tr, repo, limit):
    ui = repo.ui
    cl = repo.changelog
    mfl = repo.manifestlog
    tmfl = mfl.treemanifestlog
    treerevlog = tmfl._revlog

    maxrev = len(treerevlog) - 1
    start = treerevlog.linkrev(maxrev) + 1
    end = min(len(cl), start + limit)

    converting = _("converting")

    ui.progress(converting, 0, total=end - start)
    for i in xrange(start, end):
        ctx = repo[i]
        newflat = ctx.manifest()
        p1 = ctx.p1()
        p2 = ctx.p2()
        p1node = p1.manifestnode()
        p2node = p2.manifestnode()
        if p1node != nullid:
            if (p1node not in treerevlog.nodemap or
                (p2node != nullid and p2node not in treerevlog.nodemap)):
                ui.warn(_("unable to find parent nodes %s %s\n") % (hex(p1node),
                                                                   hex(p2node)))
                return
            parentflat = mfl[p1node].read()
            parenttree = tmfl[p1node].read()
        else:
            parentflat = manifest.manifestdict()
            parenttree = manifest.treemanifest()

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

        try:
            oldaddrevision = treerevlog.addrevision
            def addusingnode(*args, **kwargs):
                newkwargs = kwargs.copy()
                newkwargs['node'] = ctx.manifestnode()
                return oldaddrevision(*args, **newkwargs)
            treerevlog.addrevision = addusingnode
            def readtree(dir, node):
                return tmfl.get(dir, node).read()
            treerevlog.add(newtree, tr, ctx.rev(), p1node, p2node, added,
                    removed, readtree=readtree)
        finally:
            del treerevlog.__dict__['addrevision']

        ui.progress(converting, i - start, total=end - start)

    ui.progress(converting, None)

def _unpackmanifests(orig, self, repo, *args, **kwargs):
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
                        pass

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
                    pass
                else:
                    l, r = fdiff
                    if l != (None, ''):
                        import pdb
                        pdb.set_trace()
                        pass

            for fname, fnode, fflags in adds:
                fdiff = diff.get(fname)
                if fdiff is None:
                    # Sometimes adds are no-ops, so they don't show up in the
                    # diff.
                    if origtree.get(fname) != newtree.get(fname):
                        import pdb
                        pdb.set_trace()
                        pass
                else:
                    l, r = fdiff
                    if l != (fnode, fflags):
                        import pdb
                        pdb.set_trace()
                        pass
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

@command('prefetchtrees', [
    ('r', 'rev', '', _("revs to prefetch the trees for")),
    ('', 'base', '', _("revs that are assumed to already be local")),
    ] + commands.walkopts, _('--rev REVS PATTERN..'))
def prefetchtrees(ui, repo, *args, **opts):
    revs = repo.revs(opts.get('rev'))
    baserevs = []
    if opts.get('base'):
        baserevs = repo.revs(opts.get('base'))

    mfnodes = set()
    for rev in revs:
        mfnodes.add(repo[rev].manifestnode())

    basemfnodes = set()
    for rev in baserevs:
        basemfnodes.add(repo[rev].manifestnode())

    _prefetchtrees(repo, '', mfnodes, basemfnodes, [])

def _prefetchtrees(repo, rootdir, mfnodes, basemfnodes, directories):
    # If possible, use remotefilelog's more expressive fallbackpath
    if util.safehasattr(repo, 'fallbackpath'):
        fallbackpath = repo.fallbackpath
    else:
        fallbackpath = repo.ui.config('paths', 'default')

    start = time.time()
    remote = hg.peer(repo.ui, {}, fallbackpath)
    if 'gettreepack' not in remote._capabilities():
        raise error.Abort(_("missing gettreepack capability on remote"))
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
            raise error.Abort(_("unable to download %d trees (%s,...)") %
                               (len(missingnodes), list(missingnodes)[0]))
    except bundle2.AbortFromPart as exc:
        repo.ui.status(_('remote: abort: %s\n') % exc)
        raise error.Abort(_('pull failed on remote'), hint=exc.hint)
    except error.BundleValueError as exc:
        raise error.Abort(_('missing support for %s') % exc)

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
            not pushop.repo.ui.configbool('treemanifest', 'sendtrees')):
            return
        pushop.stepsdone.add('treepack')

        part = createtreepackpart(pushop.repo, pushop.outgoing,
                                  TREEGROUP_PARTTYPE2)
        bundler.addpart(part)

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

def pull(orig, ui, repo, *pats, **opts):
    result = orig(ui, repo, *pats, **opts)
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
        missingnodes = mfstore.getmissing(('', c.manifestnode()) for c in ctxs)
        mfnodes = list(n for k, n in missingnodes)

    if mfnodes:
        ui.status(_("prefetching trees\n"))
        # Calculate which parents we already have
        ctxnodes = list(ctx.node() for ctx in ctxs)
        parentctxs = repo.set('parents(%ln) - %ln',
                              ctxnodes, ctxnodes)
        basemfnodes = set(ctx.manifestnode() for ctx in parentctxs)
        missingbases = list(mfstore.getmissing(('', n) for n in basemfnodes))
        basemfnodes.difference_update(n for k, n in missingbases)

        # If we have no base nodes, scan the change log looking for a
        # semi-recent manifest node to treat as the base.
        if not basemfnodes:
            basemfnodes = _findrecenttree(repo, len(repo.changelog) - 1)

        _prefetchtrees(repo, '', mfnodes, basemfnodes, [])

    return result

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
    return wireproto.streamres(gen=bundler.getchunks(), v1compressible=True)

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

class remotetreedatastore(object):
    def __init__(self, repo):
        self._repo = repo
        self._shared = None

    def setshared(self, shared):
        self._shared = shared

    def get(self, name, node):
        # Only look at the server if not root or is public
        basemfnodes = []
        if name == '':
            # TODO: once flat manifests are gone, we won't be able to peak at
            # the revlog here to figure out our linkrev
            mfrevlog = self._repo.manifestlog._revlog
            rev = mfrevlog.rev(node)
            linkrev = mfrevlog.linkrev(rev)
            if self._repo[linkrev].phase() != phases.public:
                raise KeyError((name, node))

            # Find a recent tree that we already have
            basemfnodes = _findrecenttree(self._repo, linkrev)

        _prefetchtrees(self._repo, name, [node], basemfnodes, [])
        self._shared.markforrefresh()
        return self._shared.get(name, node)

    def getdeltachain(self, name, node):
        # Since our remote content stores just contain full texts, we return a
        # fake delta chain that just consists of a single full text revision.
        # The nullid in the deltabasenode slot indicates that the revision is a
        # fulltext.
        revision = self.get(name, node)
        return [(name, node, None, nullid, revision)]

    def add(self, name, node, data):
        raise RuntimeError("cannot add to a remote store")

    def getmissing(self, keys):
        return keys

    def markledger(self, ledger):
        pass

def serverrepack(repo, incremental=False):
    packpath = repo.vfs.join('cache/packs/%s' % PACK_CATEGORY)

    dpackstore = datapackstore(repo.ui, packpath)
    revlogstore = manifestrevlogstore(repo)
    datastore = unioncontentstore(dpackstore, revlogstore)

    hpackstore = historypackstore(repo.ui, packpath)
    histstore = unionmetadatastore(hpackstore, revlogstore)

    startrev = repo.ui.configint('treemanifest', 'repackstartrev', 0)
    endrev = repo.ui.configint('treemanifest', 'repackendrev',
                               len(repo.changelog) - 1)
    if startrev == 0 and incremental:
        latestpackedlinkrev = 0
        mfrevlog = repo.manifestlog.treemanifestlog._revlog
        for i in xrange(len(mfrevlog) - 1, 0, -1):
            node = mfrevlog.node(i)
            if not dpackstore.getmissing([('', node)]):
                latestpackedlinkrev = mfrevlog.linkrev(i)
                break
        startrev = latestpackedlinkrev + 1

    revlogstore.setrepacklinkrevrange(startrev, endrev)
    _runrepack(repo, datastore, histstore, packpath, PACK_CATEGORY)

def striptrees(orig, repo, tr, striprev, files):
    if repo.ui.configbool('treemanifest', 'server'):
        treerevlog = repo.manifestlog.treemanifestlog._revlog
        for dir in util.dirs(files):
            # If the revlog doesn't exist, this returns an empty revlog and is a
            # no-op.
            rl = treerevlog.dirlog(dir)
            rl.strip(striprev, tr)

        treerevlog.strip(striprev, tr)
