# merge.py - directory-level update/merge handling for Mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from i18n import _
import errno, util, os, tempfile, context

def filemerge(repo, fw, fo, wctx, mctx):
    """perform a 3-way merge in the working directory

    fw = filename in the working directory
    fo = filename in other parent
    wctx, mctx = working and merge changecontexts
    """

    def temp(prefix, ctx):
        pre = "%s~%s." % (os.path.basename(ctx.path()), prefix)
        (fd, name) = tempfile.mkstemp(prefix=pre)
        data = repo.wwritedata(ctx.path(), ctx.data())
        f = os.fdopen(fd, "wb")
        f.write(data)
        f.close()
        return name

    fcm = wctx.filectx(fw)
    fco = mctx.filectx(fo)

    if not fco.cmp(fcm.data()): # files identical?
        return None

    fca = fcm.ancestor(fco)
    if not fca:
        fca = repo.filectx(fw, fileid=nullrev)
    a = repo.wjoin(fw)
    b = temp("base", fca)
    c = temp("other", fco)

    if fw != fo:
        repo.ui.status(_("merging %s and %s\n") % (fw, fo))
    else:
        repo.ui.status(_("merging %s\n") % fw)

    repo.ui.debug(_("my %s other %s ancestor %s\n") % (fcm, fco, fca))

    cmd = (os.environ.get("HGMERGE") or repo.ui.config("ui", "merge")
           or "hgmerge")
    r = util.system('%s "%s" "%s" "%s"' % (cmd, a, b, c), cwd=repo.root,
                    environ={'HG_FILE': fw,
                             'HG_MY_NODE': str(wctx.parents()[0]),
                             'HG_OTHER_NODE': str(mctx)})
    if r:
        repo.ui.warn(_("merging %s failed!\n") % fw)

    os.unlink(b)
    os.unlink(c)
    return r

def checkunknown(wctx, mctx):
    "check for collisions between unknown files and files in mctx"
    man = mctx.manifest()
    for f in wctx.unknown():
        if f in man:
            if mctx.filectx(f).cmp(wctx.filectx(f).data()):
                raise util.Abort(_("untracked local file '%s' differs"
                                   " from remote version") % f)

def checkcollision(mctx):
    "check for case folding collisions in the destination context"
    folded = {}
    for fn in mctx.manifest():
        fold = fn.lower()
        if fold in folded:
            raise util.Abort(_("case-folding collision between %s and %s")
                             % (fn, folded[fold]))
        folded[fold] = fn

def forgetremoved(wctx, mctx):
    """
    Forget removed files

    If we're jumping between revisions (as opposed to merging), and if
    neither the working directory nor the target rev has the file,
    then we need to remove it from the dirstate, to prevent the
    dirstate from listing the file when it is no longer in the
    manifest.
    """

    action = []
    man = mctx.manifest()
    for f in wctx.deleted() + wctx.removed():
        if f not in man:
            action.append((f, "f"))

    return action

def findcopies(repo, m1, m2, ma, limit):
    """
    Find moves and copies between m1 and m2 back to limit linkrev
    """

    def nonoverlap(d1, d2, d3):
        "Return list of elements in d1 not in d2 or d3"
        l = [d for d in d1 if d not in d3 and d not in d2]
        l.sort()
        return l

    def dirname(f):
        s = f.rfind("/")
        if s == -1:
            return ""
        return f[:s]

    def dirs(files):
        d = {}
        for f in files:
            f = dirname(f)
            while f not in d:
                d[f] = True
                f = dirname(f)
        return d

    wctx = repo.workingctx()

    def makectx(f, n):
        if len(n) == 20:
            return repo.filectx(f, fileid=n)
        return wctx.filectx(f)
    ctx = util.cachefunc(makectx)

    def findold(fctx):
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

    copy = {}
    fullcopy = {}
    diverge = {}

    def checkcopies(c, man, aman):
        '''check possible copies for filectx c'''
        for of in findold(c):
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
                if c == ca or c2 == ca: # no merge needed, ignore copy
                    continue
                copy[c.path()] = of

    if not repo.ui.configbool("merge", "followcopies", True):
        return {}, {}

    # avoid silly behavior for update from empty dir
    if not m1 or not m2 or not ma:
        return {}, {}

    u1 = nonoverlap(m1, m2, ma)
    u2 = nonoverlap(m2, m1, ma)

    for f in u1:
        checkcopies(ctx(f, m1[f]), m2, ma)

    for f in u2:
        checkcopies(ctx(f, m2[f]), m1, ma)

    d2 = {}
    for of, fl in diverge.items():
        for f in fl:
            fo = list(fl)
            fo.remove(f)
            d2[f] = (of, fo)

    if not fullcopy or not repo.ui.configbool("merge", "followdirs", True):
        return copy, diverge

    # generate a directory move map
    d1, d2 = dirs(m1), dirs(m2)
    invalid = {}
    dirmove = {}

    # examine each file copy for a potential directory move, which is
    # when all the files in a directory are moved to a new directory
    for dst, src in fullcopy.items():
        dsrc, ddst = dirname(src), dirname(dst)
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

    # check unaccounted nonoverlapping files against directory moves
    for f in u1 + u2:
        if f not in fullcopy:
            for d in dirmove:
                if f.startswith(d):
                    # new file added in a directory that was moved, move it
                    copy[f] = dirmove[d] + f[len(d):]
                    break

    return copy, diverge

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
        a, b, c = ma.execf(fa), m1.execf(f), m2.execf(f2)
        if ((a^b) | (a^c)) ^ a:
            return 'x'
        a, b, c = ma.linkf(fa), m1.linkf(f), m2.linkf(f2)
        if ((a^b) | (a^c)) ^ a:
            return 'l'
        return ''

    def act(msg, m, f, *args):
        repo.ui.debug(" %s: %s -> %s\n" % (f, msg, m))
        action.append((f, m) + args)

    if not (backwards or overwrite):
        copy, diverge = findcopies(repo, m1, m2, ma, pa.rev())

    for of, fl in diverge.items():
        act("divergent renames", "dr", of, fl)

    copied = dict.fromkeys(copy.values())

    # Compare manifests
    for f, n in m1.iteritems():
        if partial and not partial(f):
            continue
        if f in m2:
            # are files different?
            if n != m2[f]:
                a = ma.get(f, nullid)
                # are both different from the ancestor?
                if not overwrite and n != a and m2[f] != a:
                    act("versions differ", "m", f, f, f, fmerge(f), False)
                # are we clobbering?
                # is remote's version newer?
                # or are we going back in time and clean?
                elif overwrite or m2[f] != a or (backwards and not n[20:]):
                    act("remote is newer", "g", f, m2.flags(f))
                # local is newer, not overwrite, check mode bits
                elif fmerge(f) != m1.flags(f):
                    act("update permissions", "e", f, m2.flags(f))
            # contents same, check mode bits
            elif m1.flags(f) != m2.flags(f):
                if overwrite or fmerge(f) != m1.flags(f):
                    act("update permissions", "e", f, m2.flags(f))
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
                    (_(" local changed %s which remote deleted\n") % f) +
                    _("(k)eep or (d)elete?"), _("[kd]"), _("k")) == _("d"):
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
                    (_("remote changed %s which local deleted\n") % f) +
                    _("(k)eep or (d)elete?"), _("[kd]"), _("k")) == _("k"):
                    act("prompt recreating", "g", f, m2.flags(f))
        else:
            act("remote created", "g", f, m2.flags(f))

    return action

def applyupdates(repo, action, wctx, mctx):
    "apply the merge action list to the working directory"

    updated, merged, removed, unresolved = 0, 0, 0, 0
    action.sort()
    for a in action:
        f, m = a[:2]
        if f and f[0] == "/":
            continue
        if m == "r": # remove
            repo.ui.note(_("removing %s\n") % f)
            util.audit_path(f)
            try:
                util.unlink(repo.wjoin(f))
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    repo.ui.warn(_("update failed to remove %s: %s!\n") %
                                 (f, inst.strerror))
            removed += 1
        elif m == "m": # merge
            f2, fd, flags, move = a[2:]
            r = filemerge(repo, f, f2, wctx, mctx)
            if r > 0:
                unresolved += 1
            else:
                if r is None:
                    updated += 1
                else:
                    merged += 1
            if f != fd:
                repo.ui.debug(_("copying %s to %s\n") % (f, fd))
                repo.wwrite(fd, repo.wread(f), flags)
                if move:
                    repo.ui.debug(_("removing %s\n") % f)
                    os.unlink(repo.wjoin(f))
            util.set_exec(repo.wjoin(fd), "x" in flags)
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
            util.set_exec(repo.wjoin(f), flags)

    return updated, merged, removed, unresolved

def recordupdates(repo, action, branchmerge):
    "record merge actions to the dirstate"

    for a in action:
        f, m = a[:2]
        if m == "r": # remove
            if branchmerge:
                repo.dirstate.update([f], 'r')
            else:
                repo.dirstate.forget([f])
        elif m == "f": # forget
            repo.dirstate.forget([f])
        elif m == "g": # get
            if branchmerge:
                repo.dirstate.update([f], 'n', st_mtime=-1)
            else:
                repo.dirstate.update([f], 'n')
        elif m == "m": # merge
            f2, fd, flag, move = a[2:]
            if branchmerge:
                # We've done a branch merge, mark this file as merged
                # so that we properly record the merger later
                repo.dirstate.update([fd], 'm')
                if f != f2: # copy/rename
                    if move:
                        repo.dirstate.update([f], 'r')
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
                repo.dirstate.update([fd], 'n', st_size=-1, st_mtime=-1)
                if move:
                    repo.dirstate.forget([f])
        elif m == "d": # directory rename
            f2, fd, flag = a[2:]
            if not f2 and f not in repo.dirstate:
                # untracked file moved
                continue
            if branchmerge:
                repo.dirstate.update([fd], 'a')
                if f:
                    repo.dirstate.update([f], 'r')
                    repo.dirstate.copy(f, fd)
                if f2:
                    repo.dirstate.copy(f2, fd)
            else:
                repo.dirstate.update([fd], 'n')
                if f:
                    repo.dirstate.forget([f])

def update(repo, node, branchmerge, force, partial, wlock):
    """
    Perform a merge between the working directory and the given node

    branchmerge = whether to merge between branches
    force = whether to force branch merging or file overwriting
    partial = a function to filter file lists (dirstate not updated)
    wlock = working dir lock, if already held
    """

    if not wlock:
        wlock = repo.wlock()

    wc = repo.workingctx()
    if node is None:
        # tip of current branch
        try:
            node = repo.branchtags()[wc.branch()]
        except KeyError:
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
        if wc.files():
            raise util.Abort(_("outstanding uncommitted changes"))

    ### calculate phase
    action = []
    if not force:
        checkunknown(wc, p2)
    if not util.checkfolding(repo.path):
        checkcollision(p2)
    if not branchmerge:
        action += forgetremoved(wc, p2)
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

