# repair.py - functions for repository repair for mercurial
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
# Copyright 2007 Matt Mackall
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import changegroup, revlog, os, commands

def strip(ui, repo, rev, backup="all"):
    def limitheads(chlog, stop):
        """return the list of all nodes that have no children"""
        p = {}
        h = []
        stoprev = 0
        if stop in chlog.nodemap:
            stoprev = chlog.rev(stop)

        for r in xrange(chlog.count() - 1, -1, -1):
            n = chlog.node(r)
            if n not in p:
                h.append(n)
            if n == stop:
                break
            if r < stoprev:
                break
            for pn in chlog.parents(n):
                p[pn] = 1
        return h

    def bundle(repo, bases, heads, rev, suffix):
        cg = repo.changegroupsubset(bases, heads, 'strip')
        backupdir = repo.join("strip-backup")
        if not os.path.isdir(backupdir):
            os.mkdir(backupdir)
        name = os.path.join(backupdir, "%s-%s" % (revlog.short(rev), suffix))
        ui.warn("saving bundle to %s\n" % name)
        return changegroup.writebundle(cg, name, "HG10BZ")

    def stripall(revnum):
        mm = repo.changectx(rev).manifest()
        seen = {}

        for x in xrange(revnum, repo.changelog.count()):
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
            ff.strip(filerev, revnum)

    chlog = repo.changelog
    # TODO delete the undo files, and handle undo of merge sets
    pp = chlog.parents(rev)
    revnum = chlog.rev(rev)

    # save is a list of all the branches we are truncating away
    # that we actually want to keep.  changegroup will be used
    # to preserve them and add them back after the truncate
    saveheads = []
    savebases = {}

    heads = limitheads(chlog, rev)
    seen = {}

    # search through all the heads, finding those where the revision
    # we want to strip away is an ancestor.  Also look for merges
    # that might be turned into new heads by the strip.
    while heads:
        h = heads.pop()
        n = h
        while True:
            seen[n] = 1
            pp = chlog.parents(n)
            if pp[1] != revlog.nullid:
                for p in pp:
                    if chlog.rev(p) > revnum and p not in seen:
                        heads.append(p)
            if pp[0] == revlog.nullid:
                break
            if chlog.rev(pp[0]) < revnum:
                break
            n = pp[0]
            if n == rev:
                break
        r = chlog.reachable(h, rev)
        if rev not in r:
            saveheads.append(h)
            for x in r:
                if chlog.rev(x) > revnum:
                    savebases[x] = 1

    # create a changegroup for all the branches we need to keep
    if backup == "all":
        bundle(repo, [rev], chlog.heads(), rev, 'backup')
    if saveheads:
        chgrpfile = bundle(repo, savebases.keys(), saveheads, rev, 'temp')

    stripall(revnum)

    change = chlog.read(rev)
    chlog.strip(revnum, revnum)
    repo.manifest.strip(repo.manifest.rev(change[0]), revnum)
    if saveheads:
        ui.status("adding branch\n")
        commands.unbundle(ui, repo, "file:%s" % chgrpfile, update=False)
        if backup != "strip":
            os.unlink(chgrpfile)

