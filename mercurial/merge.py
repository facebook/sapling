# merge.py - directory-level update/merge handling for Mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from i18n import gettext as _
from demandload import *
demandload(globals(), "errno util os tempfile")

def filemerge(repo, fw, fo, wctx, mctx):
    """perform a 3-way merge in the working directory

    fw = filename in the working directory
    fo = filename in other parent
    wctx, mctx = working and merge changecontexts
    """

    def temp(prefix, ctx):
        pre = "%s~%s." % (os.path.basename(ctx.path()), prefix)
        (fd, name) = tempfile.mkstemp(prefix=pre)
        f = os.fdopen(fd, "wb")
        repo.wwrite(ctx.path(), ctx.data(), f)
        f.close()
        return name

    fcm = wctx.filectx(fw)
    fco = mctx.filectx(fo)

    if not fco.cmp(fcm.data()): # files identical?
        return 0

    fca = fcm.ancestor(fco)
    if not fca:
        fca = repo.filectx(fw, fileid=-1)
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
                raise util.Abort(_("'%s' already exists in the working"
                                   " dir and differs from remote") % f)

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

def nonoverlap(d1, d2, d3):
    "Return list of elements in d1 not in d2 or d3"

    l = []
    for d in d1:
        if d not in d3 and d not in d2:
            l.append(d)

    l.sort()
    return l

def findold(fctx, limit):
    "find files that path was copied from, back to linkrev limit"

    old = {}
    orig = fctx.path()
    visit = [fctx]
    while visit:
        fc = visit.pop()
        if fc.rev() < limit:
            continue
        if fc.path() != orig and fc.path() not in old:
            old[fc.path()] = 1
        visit += fc.parents()

    old = old.keys()
    old.sort()
    return old

def findcopies(repo, m1, m2, ma, limit):
    """
    Find moves and copies between m1 and m2 back to limit linkrev
    """

    if not repo.ui.configbool("merge", "followcopies", True):
        return {}

    # avoid silly behavior for update from empty dir
    if not m1:
        return {}

    dcopies = repo.dirstate.copies()
    copy = {}
    match = {}
    u1 = nonoverlap(m1, m2, ma)
    u2 = nonoverlap(m2, m1, ma)
    ctx = util.cachefunc(lambda f,n: repo.filectx(f, fileid=n[:20]))

    def checkpair(c, f2, man):
        ''' check if an apparent pair actually matches '''
        c2 = ctx(f2, man[f2])
        ca = c.ancestor(c2)
        if ca and ca.path() == c.path() or ca.path() == c2.path():
            copy[c.path()] = f2
            copy[f2] = c.path()

    for f in u1:
        c = ctx(dcopies.get(f, f), m1[f])
        for of in findold(c, limit):
            if of in m2:
                checkpair(c, of, m2)
            else:
                match.setdefault(of, []).append(f)

    for f in u2:
        c = ctx(f, m2[f])
        for of in findold(c, limit):
            if of in m1:
                checkpair(c, of, m1)
            elif of in match:
                for mf in match[of]:
                    checkpair(c, mf, m1)

    return copy

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

    def fmerge(f, f2=None, fa=None):
        """merge executable flags"""
        if not f2:
            f2 = f
            fa = f
        a, b, c = ma.execf(fa), m1.execf(f), m2.execf(f2)
        return ((a^b) | (a^c)) ^ a

    def act(msg, m, f, *args):
        repo.ui.debug(" %s: %s -> %s\n" % (f, msg, m))
        action.append((f, m) + args)

    if not (backwards or overwrite):
        copy = findcopies(repo, m1, m2, ma, pa.rev())

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
                    act("remote is newer", "g", f, m2.execf(f))
                # local is newer, not overwrite, check mode bits
                elif fmerge(f) != m1.execf(f):
                    act("update permissions", "e", f, m2.execf(f))
            # contents same, check mode bits
            elif m1.execf(f) != m2.execf(f):
                if overwrite or fmerge(f) != m1.execf(f):
                    act("update permissions", "e", f, m2.execf(f))
        elif f in copy:
            f2 = copy[f]
            if f in ma: # case 3,20 A/B/A
                act("remote moved", "m", f, f2, f2, fmerge(f, f2, f), True)
            else:
                if f2 in m1: # case 2 A,B/B/B
                    act("local copied", "m",
                        f, f2, f, fmerge(f, f2, f2), False)
                else: # case 4,21 A/B/B
                    act("local moved", "m",
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
        if f in copy:
            f2 = copy[f]
            if f2 not in m2: # already seen
                continue
            # rename case 1, A/A,B/A
            act("remote copied", "m", f2, f, f, fmerge(f2, f, f2), False)
        elif f in ma:
            if overwrite or backwards:
                act("recreating", "g", f, m2.execf(f))
            elif n != ma[f]:
                if repo.ui.prompt(
                    (_("remote changed %s which local deleted\n") % f) +
                    _("(k)eep or (d)elete?"), _("[kd]"), _("k")) == _("k"):
                    act("prompt recreating", "g", f, m2.execf(f))
        else:
            act("remote created", "g", f, m2.execf(f))

    return action

def applyupdates(repo, action, wctx, mctx):
    "apply the merge action list to the working directory"

    updated, merged, removed, unresolved = 0, 0, 0, 0
    action.sort()
    for a in action:
        f, m = a[:2]
        if f[0] == "/":
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
            removed +=1
        elif m == "m": # merge
            f2, fd, flag, move = a[2:]
            if filemerge(repo, f, f2, wctx, mctx):
                unresolved += 1
            else:
                merged += 1
                if f != fd:
                    repo.ui.debug(_("copying %s to %s\n") % (f, fd))
                    repo.wwrite(fd, repo.wread(f))
                    if move:
                        repo.ui.debug(_("removing %s\n") % f)
                        os.unlink(repo.wjoin(f))
            util.set_exec(repo.wjoin(fd), flag)
        elif m == "g": # get
            flag = a[2]
            repo.ui.note(_("getting %s\n") % f)
            t = mctx.filectx(f).data()
            repo.wwrite(f, t)
            util.set_exec(repo.wjoin(f), flag)
            updated += 1
        elif m == "e": # exec
            flag = a[2]
            util.set_exec(repo.wjoin(f), flag)

    return updated, merged, removed, unresolved

def recordupdates(repo, action, branchmerge, mctx):
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
            else:
                # We've update-merged a locally modified file, so
                # we set the dirstate to emulate a normal checkout
                # of that file some time in the past. Thus our
                # merge will appear as a normal local file
                # modification.
                f_len = mctx.filectx(f).size()
                repo.dirstate.update([fd], 'n', st_size=f_len, st_mtime=-1)
            if f != f2: # copy/rename
                if move:
                    repo.dirstate.update([f], 'r')
                if f != fd:
                    repo.dirstate.copy(f, fd)
                else:
                    repo.dirstate.copy(f2, fd)

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

    overwrite = force and not branchmerge
    forcemerge = force and branchmerge
    wc = repo.workingctx()
    pl = wc.parents()
    p1, p2 = pl[0], repo.changectx(node)
    pa = p1.ancestor(p2)
    fp1, fp2, xp1, xp2 = p1.node(), p2.node(), str(p1), str(p2)

    ### check phase
    if not overwrite and len(pl) > 1:
        raise util.Abort(_("outstanding uncommitted merges"))
    if pa == p1 or pa == p2: # is there a linear path from p1 to p2?
        if branchmerge:
            raise util.Abort(_("there is nothing to merge, just use "
                               "'hg update' or look at 'hg heads'"))
    elif not (overwrite or branchmerge):
        raise util.Abort(_("update spans branches, use 'hg merge' "
                           "or 'hg update -C' to lose changes"))
    if branchmerge and not forcemerge:
        if wc.modified() or wc.added() or wc.removed():
            raise util.Abort(_("outstanding uncommitted changes"))

    ### calculate phase
    action = []
    if not force:
        checkunknown(wc, p2)
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
        recordupdates(repo, action, branchmerge, p2)
        repo.dirstate.setparents(fp1, fp2)
        repo.hook('update', parent1=xp1, parent2=xp2, error=stats[3])

    return stats

