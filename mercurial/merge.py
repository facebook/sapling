# merge.py - directory-level update/merge handling for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import nullid, nullrev
from i18n import _
import errno, util, os, heapq, filemerge

def _checkunknown(wctx, mctx):
    "check for collisions between unknown files and files in mctx"
    man = mctx.manifest()
    for f in wctx.unknown():
        if f in man:
            if mctx.filectx(f).cmp(wctx.filectx(f).data()):
                raise util.Abort(_("untracked file in working directory differs"
                                   " from file in requested revision: '%s'")
                                 % f)

def _checkcollision(mctx):
    "check for case folding collisions in the destination context"
    folded = {}
    for fn in mctx.manifest():
        fold = fn.lower()
        if fold in folded:
            raise util.Abort(_("case-folding collision between %s and %s")
                             % (fn, folded[fold]))
        folded[fold] = fn

def _forgetremoved(wctx, mctx, branchmerge):
    """
    Forget removed files

    If we're jumping between revisions (as opposed to merging), and if
    neither the working directory nor the target rev has the file,
    then we need to remove it from the dirstate, to prevent the
    dirstate from listing the file when it is no longer in the
    manifest.

    If we're merging, and the other revision has removed a file
    that is not present in the working directory, we need to mark it
    as removed.
    """

    action = []
    man = mctx.manifest()
    state = branchmerge and 'r' or 'f'
    for f in wctx.deleted():
        if f not in man:
            action.append((f, state))

    if not branchmerge:
        for f in wctx.removed():
            if f not in man:
                action.append((f, "f"))

    return action

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
        if fc.rev() < limit:
            continue
        visit += fc.parents()

    old = old.keys()
    old.sort()
    return old

def findcopies(repo, m1, m2, ma, limit):
    """
    Find moves and copies between m1 and m2 back to limit linkrev
    """

    wctx = repo.workingctx()

    def makectx(f, n):
        if len(n) == 20:
            return repo.filectx(f, fileid=n)
        return wctx.filectx(f)
    ctx = util.cachefunc(makectx)

    copy = {}
    fullcopy = {}
    diverge = {}

    def checkcopies(c, man, aman):
        '''check possible copies for filectx c'''
        for of in _findoldnames(c, limit):
            fullcopy[c.path()] = of # remember for dir rename detection
            if of not in man: # original file not in other manifest?
                if of in ma:
                    diverge.setdefault(of, []).append(c.path())
                continue
            # if the original file is unchanged on the other branch,
            # no merge needed
            if man[of] == aman.get(of):
                continue
            c2 = ctx(of, man[of])
            ca = c.ancestor(c2)
            if not ca: # unrelated?
                continue
            # named changed on only one side?
            if ca.path() == c.path() or ca.path() == c2.path():
                if c == ca and c2 == ca: # no merge needed, ignore copy
                    continue
                copy[c.path()] = of

    if not repo.ui.configbool("merge", "followcopies", True):
        return {}, {}

    # avoid silly behavior for update from empty dir
    if not m1 or not m2 or not ma:
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
        checkcopies(ctx(f, m1[f]), m2, ma)

    for f in u2:
        checkcopies(ctx(f, m2[f]), m1, ma)

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

def _symmetricdifference(repo, rev1, rev2):
    """symmetric difference of the sets of ancestors of rev1 and rev2

    I.e. revisions that are ancestors of rev1 or rev2, but not both.
    """
    # basic idea:
    # - mark rev1 and rev2 with different colors
    # - walk the graph in topological order with the help of a heap;
    #   for each revision r:
    #     - if r has only one color, we want to return it
    #     - add colors[r] to its parents
    #
    # We keep track of the number of revisions in the heap that
    # we may be interested in.  We stop walking the graph as soon
    # as this number reaches 0.
    WHITE = 1
    BLACK = 2
    ALLCOLORS = WHITE | BLACK
    colors = {rev1: WHITE, rev2: BLACK}

    cl = repo.changelog

    visit = [-rev1, -rev2]
    heapq.heapify(visit)
    n_wanted = len(visit)
    ret = []

    while n_wanted:
        r = -heapq.heappop(visit)
        wanted = colors[r] != ALLCOLORS
        n_wanted -= wanted
        if wanted:
            ret.append(r)

        for p in cl.parentrevs(r):
            if p == nullrev:
                continue
            if p not in colors:
                # first time we see p; add it to visit
                n_wanted += wanted
                colors[p] = colors[r]
                heapq.heappush(visit, -p)
            elif colors[p] != ALLCOLORS and colors[p] != colors[r]:
                # at first we thought we wanted p, but now
                # we know we don't really want it
                n_wanted -= 1
                colors[p] |= colors[r]

        del colors[r]

    return ret

def manifestmerge(repo, p1, p2, pa, overwrite, partial):
    """
    Merge p1 and p2 with ancestor ma and generate merge action list

    overwrite = whether we clobber working files
    partial = function to filter file lists
    """

    repo.ui.note(_("resolving manifests\n"))
    repo.ui.debug(_(" overwrite %s partial %s\n") % (overwrite, bool(partial)))
    repo.ui.debug(_(" ancestor %s local %s remote %s\n") % (pa, p1, p2))

    m1 = p1.manifest()
    m2 = p2.manifest()
    ma = pa.manifest()
    backwards = (pa == p2)
    action = []
    copy = {}
    diverge = {}

    def fmerge(f, f2=None, fa=None):
        """merge flags"""
        if not f2:
            f2 = f
            fa = f
        a, m, n = ma.flags(fa), m1.flags(f), m2.flags(f2)
        if m == n: # flags agree
            return m # unchanged
        if m and n: # flags are set but don't agree
            if not a: # both differ from parent
                r = repo.ui.prompt(
                    _(" conflicting flags for %s\n"
                      "(n)one, e(x)ec or sym(l)ink?") % f, "[nxl]", "n")
                return r != "n" and r or ''
            if m == a:
                return n # changed from m to n
            return m # changed from n to m
        if m and m != a: # changed from a to m
            return m
        if n and n != a: # changed from a to n
            return n
        return '' # flag was cleared

    def act(msg, m, f, *args):
        repo.ui.debug(" %s: %s -> %s\n" % (f, msg, m))
        action.append((f, m) + args)

    if not (backwards or overwrite):
        rev1 = p1.rev()
        if rev1 is None:
            # p1 is a workingctx
            rev1 = p1.parents()[0].rev()
        limit = min(_symmetricdifference(repo, rev1, p2.rev()))
        copy, diverge = findcopies(repo, m1, m2, ma, limit)

    for of, fl in diverge.items():
        act("divergent renames", "dr", of, fl)

    copied = dict.fromkeys(copy.values())

    # Compare manifests
    for f, n in m1.iteritems():
        if partial and not partial(f):
            continue
        if f in m2:
            if overwrite or backwards:
                rflags = m2.flags(f)
            else:
                rflags = fmerge(f)
            # are files different?
            if n != m2[f]:
                a = ma.get(f, nullid)
                # are we clobbering?
                if overwrite:
                    act("clobbering", "g", f, rflags)
                # or are we going back in time and clean?
                elif backwards and not n[20:]:
                    act("reverting", "g", f, rflags)
                # are both different from the ancestor?
                elif n != a and m2[f] != a:
                    act("versions differ", "m", f, f, f, rflags, False)
                # is remote's version newer?
                elif m2[f] != a:
                    act("remote is newer", "g", f, rflags)
                # local is newer, not overwrite, check mode bits
                elif m1.flags(f) != rflags:
                    act("update permissions", "e", f, rflags)
            # contents same, check mode bits
            elif m1.flags(f) != rflags:
                act("update permissions", "e", f, rflags)
        elif f in copied:
            continue
        elif f in copy:
            f2 = copy[f]
            if f2 not in m2: # directory rename
                act("remote renamed directory to " + f2, "d",
                    f, None, f2, m1.flags(f))
            elif f2 in m1: # case 2 A,B/B/B
                act("local copied to " + f2, "m",
                    f, f2, f, fmerge(f, f2, f2), False)
            else: # case 4,21 A/B/B
                act("local moved to " + f2, "m",
                    f, f2, f, fmerge(f, f2, f2), False)
        elif f in ma:
            if n != ma[f] and not overwrite:
                if repo.ui.prompt(
                    _(" local changed %s which remote deleted\n"
                      "use (c)hanged version or (d)elete?") % f,
                    _("[cd]"), _("c")) == _("d"):
                    act("prompt delete", "r", f)
            else:
                act("other deleted", "r", f)
        else:
            # file is created on branch or in working directory
            if (overwrite and n[20:] != "u") or (backwards and not n[20:]):
                act("remote deleted", "r", f)

    for f, n in m2.iteritems():
        if partial and not partial(f):
            continue
        if f in m1:
            continue
        if f in copied:
            continue
        if f in copy:
            f2 = copy[f]
            if f2 not in m1: # directory rename
                act("local renamed directory to " + f2, "d",
                    None, f, f2, m2.flags(f))
            elif f2 in m2: # rename case 1, A/A,B/A
                act("remote copied to " + f, "m",
                    f2, f, f, fmerge(f2, f, f2), False)
            else: # case 3,20 A/B/A
                act("remote moved to " + f, "m",
                    f2, f, f, fmerge(f2, f, f2), True)
        elif f in ma:
            if overwrite or backwards:
                act("recreating", "g", f, m2.flags(f))
            elif n != ma[f]:
                if repo.ui.prompt(
                    _("remote changed %s which local deleted\n"
                      "use (c)hanged version or leave (d)eleted?") % f,
                    _("[cd]"), _("c")) == _("c"):
                    act("prompt recreating", "g", f, m2.flags(f))
        else:
            act("remote created", "g", f, m2.flags(f))

    return action

def applyupdates(repo, action, wctx, mctx):
    "apply the merge action list to the working directory"

    updated, merged, removed, unresolved = 0, 0, 0, 0
    action.sort()
    # prescan for copy/renames
    for a in action:
        f, m = a[:2]
        if m == 'm': # merge
            f2, fd, flags, move = a[2:]
            if f != fd:
                repo.ui.debug(_("copying %s to %s\n") % (f, fd))
                repo.wwrite(fd, repo.wread(f), flags)

    audit_path = util.path_auditor(repo.root)

    for a in action:
        f, m = a[:2]
        if f and f[0] == "/":
            continue
        if m == "r": # remove
            repo.ui.note(_("removing %s\n") % f)
            audit_path(f)
            try:
                util.unlink(repo.wjoin(f))
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    repo.ui.warn(_("update failed to remove %s: %s!\n") %
                                 (f, inst.strerror))
            removed += 1
        elif m == "m": # merge
            f2, fd, flags, move = a[2:]
            r = filemerge.filemerge(repo, f, fd, f2, wctx, mctx)
            if r > 0:
                unresolved += 1
            else:
                if r is None:
                    updated += 1
                else:
                    merged += 1
            util.set_flags(repo.wjoin(fd), flags)
            if f != fd and move and util.lexists(repo.wjoin(f)):
                repo.ui.debug(_("removing %s\n") % f)
                os.unlink(repo.wjoin(f))
        elif m == "g": # get
            flags = a[2]
            repo.ui.note(_("getting %s\n") % f)
            t = mctx.filectx(f).data()
            repo.wwrite(f, t, flags)
            updated += 1
        elif m == "d": # directory rename
            f2, fd, flags = a[2:]
            if f:
                repo.ui.note(_("moving %s to %s\n") % (f, fd))
                t = wctx.filectx(f).data()
                repo.wwrite(fd, t, flags)
                util.unlink(repo.wjoin(f))
            if f2:
                repo.ui.note(_("getting %s to %s\n") % (f2, fd))
                t = mctx.filectx(f2).data()
                repo.wwrite(fd, t, flags)
            updated += 1
        elif m == "dr": # divergent renames
            fl = a[2]
            repo.ui.warn("warning: detected divergent renames of %s to:\n" % f)
            for nf in fl:
                repo.ui.warn(" %s\n" % nf)
        elif m == "e": # exec
            flags = a[2]
            util.set_flags(repo.wjoin(f), flags)

    return updated, merged, removed, unresolved

def recordupdates(repo, action, branchmerge):
    "record merge actions to the dirstate"

    for a in action:
        f, m = a[:2]
        if m == "r": # remove
            if branchmerge:
                repo.dirstate.remove(f)
            else:
                repo.dirstate.forget(f)
        elif m == "f": # forget
            repo.dirstate.forget(f)
        elif m in "ge": # get or exec change
            if branchmerge:
                repo.dirstate.normaldirty(f)
            else:
                repo.dirstate.normal(f)
        elif m == "m": # merge
            f2, fd, flag, move = a[2:]
            if branchmerge:
                # We've done a branch merge, mark this file as merged
                # so that we properly record the merger later
                repo.dirstate.merge(fd)
                if f != f2: # copy/rename
                    if move:
                        repo.dirstate.remove(f)
                    if f != fd:
                        repo.dirstate.copy(f, fd)
                    else:
                        repo.dirstate.copy(f2, fd)
            else:
                # We've update-merged a locally modified file, so
                # we set the dirstate to emulate a normal checkout
                # of that file some time in the past. Thus our
                # merge will appear as a normal local file
                # modification.
                repo.dirstate.normallookup(fd)
                if move:
                    repo.dirstate.forget(f)
        elif m == "d": # directory rename
            f2, fd, flag = a[2:]
            if not f2 and f not in repo.dirstate:
                # untracked file moved
                continue
            if branchmerge:
                repo.dirstate.add(fd)
                if f:
                    repo.dirstate.remove(f)
                    repo.dirstate.copy(f, fd)
                if f2:
                    repo.dirstate.copy(f2, fd)
            else:
                repo.dirstate.normal(fd)
                if f:
                    repo.dirstate.forget(f)

def update(repo, node, branchmerge, force, partial):
    """
    Perform a merge between the working directory and the given node

    branchmerge = whether to merge between branches
    force = whether to force branch merging or file overwriting
    partial = a function to filter file lists (dirstate not updated)
    """

    wlock = repo.wlock()
    try:
        wc = repo.workingctx()
        if node is None:
            # tip of current branch
            try:
                node = repo.branchtags()[wc.branch()]
            except KeyError:
                if wc.branch() == "default": # no default branch!
                    node = repo.lookup("tip") # update to tip
                else:
                    raise util.Abort(_("branch %s not found") % wc.branch())
        overwrite = force and not branchmerge
        forcemerge = force and branchmerge
        pl = wc.parents()
        p1, p2 = pl[0], repo.changectx(node)
        pa = p1.ancestor(p2)
        fp1, fp2, xp1, xp2 = p1.node(), p2.node(), str(p1), str(p2)
        fastforward = False

        ### check phase
        if not overwrite and len(pl) > 1:
            raise util.Abort(_("outstanding uncommitted merges"))
        if pa == p1 or pa == p2: # is there a linear path from p1 to p2?
            if branchmerge:
                if p1.branch() != p2.branch() and pa != p2:
                    fastforward = True
                else:
                    raise util.Abort(_("there is nothing to merge, just use "
                                       "'hg update' or look at 'hg heads'"))
        elif not (overwrite or branchmerge):
            raise util.Abort(_("update spans branches, use 'hg merge' "
                               "or 'hg update -C' to lose changes"))
        if branchmerge and not forcemerge:
            if wc.files() or wc.deleted():
                raise util.Abort(_("outstanding uncommitted changes"))

        ### calculate phase
        action = []
        if not force:
            _checkunknown(wc, p2)
        if not util.checkfolding(repo.path):
            _checkcollision(p2)
        action += _forgetremoved(wc, p2, branchmerge)
        action += manifestmerge(repo, wc, p2, pa, overwrite, partial)

        ### apply phase
        if not branchmerge: # just jump to the new rev
            fp1, fp2, xp1, xp2 = fp2, nullid, xp2, ''
        if not partial:
            repo.hook('preupdate', throw=True, parent1=xp1, parent2=xp2)

        stats = applyupdates(repo, action, wc, p2)

        if not partial:
            recordupdates(repo, action, branchmerge)
            repo.dirstate.setparents(fp1, fp2)
            if not branchmerge and not fastforward:
                repo.dirstate.setbranch(p2.branch())
            repo.hook('update', parent1=xp1, parent2=xp2, error=stats[3])

        return stats
    finally:
        del wlock
