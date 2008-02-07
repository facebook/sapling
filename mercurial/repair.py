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

def _bundle(repo, bases, heads, node, suffix, extranodes=None):
    """create a bundle with the specified revisions as a backup"""
    cg = repo.changegroupsubset(bases, heads, 'strip', extranodes)
    backupdir = repo.join("strip-backup")
    if not os.path.isdir(backupdir):
        os.mkdir(backupdir)
    name = os.path.join(backupdir, "%s-%s" % (short(node), suffix))
    repo.ui.warn("saving bundle to %s\n" % name)
    return changegroup.writebundle(cg, name, "HG10BZ")

def _collectfiles(repo, striprev):
    """find out the filelogs affected by the strip"""
    files = {}

    for x in xrange(striprev, repo.changelog.count()):
        for name in repo.changectx(x).files():
            if name in files:
                continue
            files[name] = 1

    files = files.keys()
    files.sort()
    return files

def _collectextranodes(repo, files, link):
    """return the nodes that have to be saved before the strip"""
    def collectone(revlog):
        extra = []
        startrev = count = revlog.count()
        # find the truncation point of the revlog
        for i in xrange(0, count):
            node = revlog.node(i)
            lrev = revlog.linkrev(node)
            if lrev >= link:
                startrev = i + 1
                break

        # see if any revision after that point has a linkrev less than link
        # (we have to manually save these guys)
        for i in xrange(startrev, count):
            node = revlog.node(i)
            lrev = revlog.linkrev(node)
            if lrev < link:
                extra.append((node, cl.node(lrev)))

        return extra

    extranodes = {}
    cl = repo.changelog
    extra = collectone(repo.manifest)
    if extra:
        extranodes[1] = extra
    for fname in files:
        f = repo.file(fname)
        extra = collectone(f)
        if extra:
            extranodes[fname] = extra

    return extranodes

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

    files = _collectfiles(repo, striprev)

    extranodes = _collectextranodes(repo, files, striprev)

    # create a changegroup for all the branches we need to keep
    if backup == "all":
        _bundle(repo, [node], cl.heads(), node, 'backup')
    if saveheads or extranodes:
        chgrpfile = _bundle(repo, savebases.keys(), saveheads, node, 'temp',
                            extranodes)

    cl.strip(striprev)
    repo.manifest.strip(striprev)
    for name in files:
        f = repo.file(name)
        f.strip(striprev)

    if saveheads or extranodes:
        ui.status("adding branch\n")
        f = open(chgrpfile, "rb")
        gen = changegroup.readbundle(f, chgrpfile)
        repo.addchangegroup(gen, 'strip', 'bundle:' + chgrpfile, True)
        f.close()
        if backup != "strip":
            os.unlink(chgrpfile)

