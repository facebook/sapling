# __init__.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import (
    changegroup,
    cmdutil,
    extensions,
    localrepo,
    scmutil,
    util,
)
from mercurial.i18n import _
from mercurial.node import bin, hex, nullrev

from remotefilelog.contentstore import unioncontentstore
from remotefilelog.datapack import datapackstore, mutabledatapack
from remotefilelog import shallowutil
import ctreemanifest

import struct

cmdtable = {}
command = cmdutil.command(cmdtable)

PACK_CATEGORY='manifest'

def extsetup(ui):
    extensions.wrapfunction(changegroup.cg1unpacker, '_unpackmanifests',
                            _unpackmanifests)

def reposetup(ui, repo):
    wraprepo(repo)

def wraprepo(repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    packpath = shallowutil.getpackpath(repo, PACK_CATEGORY)
    datastore = datapackstore(
        packpath,
        usecdatapack=repo.ui.configbool('remotefilelog', 'fastdatapack'))
    repo.svfs.manifestdatastore = unioncontentstore(datastore)

def _unpackmanifests(orig, self, repo, *args, **kwargs):
    mf = repo.manifest
    oldtip = len(mf)

    orig(self, repo, *args, **kwargs)

    if (util.safehasattr(repo.svfs, "manifestdatastore") and
        repo.ui.configbool('treemanifest', 'autocreatetrees')):
        packpath = shallowutil.getpackpath(repo, PACK_CATEGORY)
        opener = scmutil.vfs(packpath)
        with mutabledatapack(repo.ui, opener) as dpack:
            recordmanifest(dpack, repo, oldtip, len(mf))
            dpack.close()

        # Alert the store that there may be new packs
        repo.svfs.manifestdatastore.markforrefresh()

class InterceptedMutablePack(object):
    def __init__(self, pack, node):
        self._pack = pack
        self._node = node

    def add(self, name, node, deltabasenode, delta):
        # For the root node, provide the flat manifest as the key
        if name == "":
            node = self._node
        return self._pack.add(name, node, deltabasenode, delta)

def recordmanifest(pack, repo, oldtip, newtip):
    mf = repo.manifest
    total = newtip - oldtip
    ui = repo.ui
    builttrees = {}
    message = _('priming tree cache')
    ui.progress(message, 0, total=total)

    refcount = {}
    for rev in xrange(oldtip, newtip):
        p1 = mf.parentrevs(rev)[0]
        p1node = mf.node(p1)
        refcount[p1node] = refcount.get(p1node, 0) + 1

    for rev in xrange(oldtip, newtip):
        ui.progress(message, rev - oldtip, total=total)
        p1 = mf.parentrevs(rev)[0]
        p1node = mf.node(p1)

        if p1node in builttrees:
            origtree = builttrees[p1node]
        else:
            origtree = mf.read(p1node)._treemanifest()

        if not origtree:
            p1mf = mf.read(p1node)
            origtree = ctreemanifest.treemanifest(repo.svfs.manifestdatastore)
            for filename, node, flag in p1mf.iterentries():
                origtree.set(filename, node, flag)
            origtree.write(InterceptedMutablePack(pack, p1node))
            builttrees[p1node] = origtree

        # Remove the tree from the cache once we've processed its final use.
        # Otherwise memory explodes
        p1refcount = refcount[p1node] - 1
        if p1refcount == 0:
            builttrees.pop(p1node, None)
        refcount[p1node] = p1refcount

        # This will generally be very quick, since p1 == deltabase
        delta = mf.revdiff(p1, rev)

        allfiles = set(repo.changelog.readfiles(mf.linkrev(rev)))

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
                # Where start and end say what bytes to delete, and data says
                # what bytes to insert in their place. So we can just read
                # <data> to figure out all the added files.
                byte1, byte2, blocklen = struct.unpack(">lll",
                        delta[current:current + 12])
                current += 12
                if blocklen:
                    block = delta[current:current + blocklen]
                    current += blocklen
            except struct.error:
                raise mpatchError("patch cannot be decoded")

            # An individual delta block may contain multiple newline delimited
            # entries.
            for line in block.split('\n'):
                if not line:
                    continue
                fname, rest = line.split('\0')
                # It's possible for a delta to contain an entry that is a no-op
                # (deletes the same data it adds), so check it against allfiles.
                if fname not in allfiles:
                    continue
                fnode = rest[:40]
                fflag = rest[40:]
                adds.append((fname, bin(fnode), fflag))

        deletes = allfiles.difference(fname for fname, fnode, fflag
                                           in adds)

        # Apply the changes on top of the parent tree
        newtree = origtree.copy()
        for fname in deletes:
            newtree.set(fname, None, None)

        for fname, fnode, fflags in adds:
            newtree.set(fname, fnode, fflags)

        newtree.write(InterceptedMutablePack(pack, mf.node(rev)), origtree)
        diff = newtree.diff(origtree)

        if ui.configbool('treemanifest', 'verifyautocreate', True):
            if len(diff) != len(adds) + len(deletes):
                import pdb
                pdb.set_trace()

            for fname in deletes:
                l, r = diff[fname]
                if l != (None, ''):
                    import pdb
                    pdb.set_trace()
                    pass

            for fname, fnode, fflags in adds:
                l, r = diff[fname]
                if l != (fnode, fflags):
                    import pdb
                    pdb.set_trace()
                    pass
        builttrees[mf.node(rev)] = newtree

        mfnode = mf.node(rev)
        if refcount.get(mfnode) > 0:
            builttrees[mfnode] = newtree

    ui.progress(message, None)
