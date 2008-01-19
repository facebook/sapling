# repair.py - functions for repository repair for mercurial
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
# Copyright 2007 Matt Mackall
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import changegroup, os
from node import *

def strip(ui, repo, node, backup="all"):
    def limitheads(cl, stop):
        """return the list of all nodes that have no children"""
        p = {}
        h = []
        stoprev = 0
        if stop in cl.nodemap:
            stoprev = cl.rev(stop)

        for r in xrange(cl.count() - 1, -1, -1):
            n = cl.node(r)
            if n not in p:
                h.append(n)
            if n == stop:
                break
            if r < stoprev:
                break
            for pn in cl.parents(n):
                p[pn] = 1
        return h

    def bundle(repo, bases, heads, node, suffix):
        cg = repo.changegroupsubset(bases, heads, 'strip')
        backupdir = repo.join("strip-backup")
        if not os.path.isdir(backupdir):
            os.mkdir(backupdir)
        name = os.path.join(backupdir, "%s-%s" % (short(node), suffix))
        ui.warn("saving bundle to %s\n" % name)
        return changegroup.writebundle(cg, name, "HG10BZ")

    def stripall(striprev):
        mm = repo.changectx(node).manifest()
        seen = {}

        for x in xrange(striprev, repo.changelog.count()):
            for f in repo.changectx(x).files():
                if f in seen:
                    continue
                seen[f] = 1
                if f in mm:
                    filerev = mm[f]
                else:
                    filerev = 0
                seen[f] = filerev
        # we go in two steps here so the strip loop happens in a
        # sensible order.  When stripping many files, this helps keep
        # our disk access patterns under control.
        seen_list = seen.keys()
        seen_list.sort()
        for f in seen_list:
            ff = repo.file(f)
            filerev = seen[f]
            if filerev != 0:
                if filerev in ff.nodemap:
                    filerev = ff.rev(filerev)
                else:
                    filerev = 0
            ff.strip(filerev, striprev)

    cl = repo.changelog
    # TODO delete the undo files, and handle undo of merge sets
    pp = cl.parents(node)
    striprev = cl.rev(node)

    # save is a list of all the branches we are truncating away
    # that we actually want to keep.  changegroup will be used
    # to preserve them and add them back after the truncate
    saveheads = []
    savebases = {}

    heads = limitheads(cl, node)
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
        bundle(repo, [node], cl.heads(), node, 'backup')
    if saveheads:
        chgrpfile = bundle(repo, savebases.keys(), saveheads, node, 'temp')

    stripall(striprev)

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

