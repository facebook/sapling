# copies.py - copy detection for Mercurial
#
# Copyright 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import nullid, nullrev
from i18n import _
import util, ancestor

def _nonoverlap(d1, d2, d3):
    "Return list of elements in d1 not in d2 or d3"
    l = [d for d in d1 if d not in d3 and d not in d2]
    l.sort()
    return l

def _dirname(f):
    s = f.rfind("/")
    if s == -1:
        return ""
    return f[:s]

def _dirs(files):
    d = {}
    for f in files:
        f = _dirname(f)
        while f not in d:
            d[f] = True
            f = _dirname(f)
    return d

def _findoldnames(fctx, limit):
    "find files that path was copied from, back to linkrev limit"
    old = {}
    seen = {}
    orig = fctx.path()
    visit = [fctx]
    while visit:
        fc = visit.pop()
        s = str(fc)
        if s in seen:
            continue
        seen[s] = 1
        if fc.path() != orig and fc.path() not in old:
            old[fc.path()] = 1
        if fc.rev() < limit and fc.rev() is not None:
            continue
        visit += fc.parents()

    old = old.keys()
    old.sort()
    return old

def copies(repo, c1, c2, ca):
    """
    Find moves and copies between context c1 and c2
    """
    # avoid silly behavior for update from empty dir
    if not c1 or not c2:
        return {}, {}

    rev1, rev2 = c1.rev(), c2.rev()
    if rev1 is None: # c1 is a workingctx
        rev1 = c1.parents()[0].rev()
    if rev2 is None: # c2 is a workingctx
        rev2 = c2.parents()[0].rev()
    pr = repo.changelog.parentrevs
    def parents(rev):
        return [p for p in pr(rev) if p != nullrev]
    limit = min(ancestor.symmetricdifference(rev1, rev2, parents))
    m1 = c1.manifest()
    m2 = c2.manifest()
    ma = ca.manifest()

    def makectx(f, n):
        if len(n) != 20: # in a working context?
            if c1.rev() is None:
                return c1.filectx(f)
            return c2.filectx(f)
        return repo.filectx(f, fileid=n)
    ctx = util.cachefunc(makectx)

    copy = {}
    fullcopy = {}
    diverge = {}

    def checkcopies(f, m1, m2):
        '''check possible copies of f from m1 to m2'''
        c1 = ctx(f, m1[f])
        for of in _findoldnames(c1, limit):
            fullcopy[f] = of # remember for dir rename detection
            if of in m2: # original file not in other manifest?
                # if the original file is unchanged on the other branch,
                # no merge needed
                if m2[of] != ma.get(of):
                    c2 = ctx(of, m2[of])
                    ca = c1.ancestor(c2)
                    # related and named changed on only one side?
                    if ca and (ca.path() == f or ca.path() == c2.path()):
                        if c1 != ca or c2 != ca: # merge needed?
                            copy[f] = of
            elif of in ma:
                diverge.setdefault(of, []).append(f)

    if not repo.ui.configbool("merge", "followcopies", True):
        return {}, {}

    repo.ui.debug(_("  searching for copies back to rev %d\n") % limit)

    u1 = _nonoverlap(m1, m2, ma)
    u2 = _nonoverlap(m2, m1, ma)

    if u1:
        repo.ui.debug(_("  unmatched files in local:\n   %s\n")
                      % "\n   ".join(u1))
    if u2:
        repo.ui.debug(_("  unmatched files in other:\n   %s\n")
                      % "\n   ".join(u2))

    for f in u1:
        checkcopies(f, m1, m2)
    for f in u2:
        checkcopies(f, m2, m1)

    diverge2 = {}
    for of, fl in diverge.items():
        if len(fl) == 1:
            del diverge[of] # not actually divergent
        else:
            diverge2.update(dict.fromkeys(fl)) # reverse map for below

    if fullcopy:
        repo.ui.debug(_("  all copies found (* = to merge, ! = divergent):\n"))
        for f in fullcopy:
            note = ""
            if f in copy: note += "*"
            if f in diverge2: note += "!"
            repo.ui.debug(_("   %s -> %s %s\n") % (f, fullcopy[f], note))
    del diverge2

    if not fullcopy or not repo.ui.configbool("merge", "followdirs", True):
        return copy, diverge

    repo.ui.debug(_("  checking for directory renames\n"))

    # generate a directory move map
    d1, d2 = _dirs(m1), _dirs(m2)
    invalid = {}
    dirmove = {}

    # examine each file copy for a potential directory move, which is
    # when all the files in a directory are moved to a new directory
    for dst, src in fullcopy.items():
        dsrc, ddst = _dirname(src), _dirname(dst)
        if dsrc in invalid:
            # already seen to be uninteresting
            continue
        elif dsrc in d1 and ddst in d1:
            # directory wasn't entirely moved locally
            invalid[dsrc] = True
        elif dsrc in d2 and ddst in d2:
            # directory wasn't entirely moved remotely
            invalid[dsrc] = True
        elif dsrc in dirmove and dirmove[dsrc] != ddst:
            # files from the same directory moved to two different places
            invalid[dsrc] = True
        else:
            # looks good so far
            dirmove[dsrc + "/"] = ddst + "/"

    for i in invalid:
        if i in dirmove:
            del dirmove[i]
    del d1, d2, invalid

    if not dirmove:
        return copy, diverge

    for d in dirmove:
        repo.ui.debug(_("  dir %s -> %s\n") % (d, dirmove[d]))

    # check unaccounted nonoverlapping files against directory moves
    for f in u1 + u2:
        if f not in fullcopy:
            for d in dirmove:
                if f.startswith(d):
                    # new file added in a directory that was moved, move it
                    copy[f] = dirmove[d] + f[len(d):]
                    repo.ui.debug(_("  file %s -> %s\n") % (f, copy[f]))
                    break

    return copy, diverge
