# repair.py - functions for repository repair for mercurial
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
# Copyright 2007 Matt Mackall
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import changegroup, os
from node import *

def _limitheads(cl, stoprev):
    """return the list of all revs >= stoprev that have no children"""
    seen = {}
    heads = []

    for r in xrange(cl.count() - 1, stoprev - 1, -1):
        if r not in seen:
            heads.append(r)
        for p in cl.parentrevs(r):
            seen[p] = 1
    return heads

def _bundle(repo, bases, heads, node, suffix):
    """create a bundle with the specified revisions as a backup"""
    cg = repo.changegroupsubset(bases, heads, 'strip')
    backupdir = repo.join("strip-backup")
    if not os.path.isdir(backupdir):
        os.mkdir(backupdir)
    name = os.path.join(backupdir, "%s-%s" % (short(node), suffix))
    repo.ui.warn("saving bundle to %s\n" % name)
    return changegroup.writebundle(cg, name, "HG10BZ")

def _collectfilenodes(repo, striprev):
    """find out the first node that should be stripped from each filelog"""
    mm = repo.changectx(striprev).manifest()
    filenodes = {}

    for x in xrange(striprev, repo.changelog.count()):
        for name in repo.changectx(x).files():
            if name in filenodes:
                continue
            filenodes[name] = mm.get(name)

    return filenodes

def _stripall(repo, striprev, filenodes):
    """strip the requested nodes from the filelogs"""
    # we go in two steps here so the strip loop happens in a
    # sensible order.  When stripping many files, this helps keep
    # our disk access patterns under control.

    files = filenodes.keys()
    files.sort()
    for name in files:
        f = repo.file(name)
        fnode = filenodes[name]
        frev = 0
        if fnode is not None and fnode in f.nodemap:
            frev = f.rev(fnode)
        f.strip(frev, striprev)

def strip(ui, repo, node, backup="all"):
    cl = repo.changelog
    # TODO delete the undo files, and handle undo of merge sets
    pp = cl.parents(node)
    striprev = cl.rev(node)

    # save is a list of all the branches we are truncating away
    # that we actually want to keep.  changegroup will be used
    # to preserve them and add them back after the truncate
    saveheads = []
    savebases = {}

    heads = [cl.node(r) for r in _limitheads(cl, striprev)]
    seen = {}

    # search through all the heads, finding those where the revision
    # we want to strip away is an ancestor.  Also look for merges
    # that might be turned into new heads by the strip.
    while heads:
        h = heads.pop()
        n = h
        while True:
            seen[n] = 1
            pp = cl.parents(n)
            if pp[1] != nullid:
                for p in pp:
                    if cl.rev(p) > striprev and p not in seen:
                        heads.append(p)
            if pp[0] == nullid:
                break
            if cl.rev(pp[0]) < striprev:
                break
            n = pp[0]
            if n == node:
                break
        r = cl.reachable(h, node)
        if node not in r:
            saveheads.append(h)
            for x in r:
                if cl.rev(x) > striprev:
                    savebases[x] = 1

    # create a changegroup for all the branches we need to keep
    if backup == "all":
        _bundle(repo, [node], cl.heads(), node, 'backup')
    if saveheads:
        chgrpfile = _bundle(repo, savebases.keys(), saveheads, node, 'temp')

    filenodes = _collectfilenodes(repo, striprev)
    _stripall(repo, striprev, filenodes)

    change = cl.read(node)
    cl.strip(striprev, striprev)
    repo.manifest.strip(repo.manifest.rev(change[0]), striprev)
    if saveheads:
        ui.status("adding branch\n")
        f = open(chgrpfile, "rb")
        gen = changegroup.readbundle(f, chgrpfile)
        repo.addchangegroup(gen, 'strip', 'bundle:' + chgrpfile)
        f.close()
        if backup != "strip":
            os.unlink(chgrpfile)

