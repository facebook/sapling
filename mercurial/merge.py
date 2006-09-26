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

def merge3(repo, fn, my, other, p1, p2):
    """perform a 3-way merge in the working directory"""

    def temp(prefix, node):
        pre = "%s~%s." % (os.path.basename(fn), prefix)
        (fd, name) = tempfile.mkstemp(prefix=pre)
        f = os.fdopen(fd, "wb")
        repo.wwrite(fn, fl.read(node), f)
        f.close()
        return name

    fl = repo.file(fn)
    base = fl.ancestor(my, other)
    a = repo.wjoin(fn)
    b = temp("base", base)
    c = temp("other", other)

    repo.ui.note(_("resolving %s\n") % fn)
    repo.ui.debug(_("file %s: my %s other %s ancestor %s\n") %
                          (fn, short(my), short(other), short(base)))

    cmd = (os.environ.get("HGMERGE") or repo.ui.config("ui", "merge")
           or "hgmerge")
    r = util.system('%s "%s" "%s" "%s"' % (cmd, a, b, c), cwd=repo.root,
                    environ={'HG_FILE': fn,
                             'HG_MY_NODE': p1,
                             'HG_OTHER_NODE': p2,
                             'HG_FILE_MY_NODE': hex(my),
                             'HG_FILE_OTHER_NODE': hex(other),
                             'HG_FILE_BASE_NODE': hex(base)})
    if r:
        repo.ui.warn(_("merging %s failed!\n") % fn)

    os.unlink(b)
    os.unlink(c)
    return r

def checkunknown(repo, m2, status):
    """
    check for collisions between unknown files and files in m2
    """
    modified, added, removed, deleted, unknown = status[:5]
    for f in unknown:
        if f in m2:
            if repo.file(f).cmp(m2[f], repo.wread(f)):
                raise util.Abort(_("'%s' already exists in the working"
                                   " dir and differs from remote") % f)

def workingmanifest(repo, man, status):
    """
    Update manifest to correspond to the working directory
    """

    copied = repo.dirstate.copies()
    modified, added, removed, deleted, unknown = status[:5]
    for i,l in (("a", added), ("m", modified), ("u", unknown)):
        for f in l:
            man[f] = man.get(copied.get(f, f), nullid) + i
            man.set(f, util.is_exec(repo.wjoin(f), man.execf(f)))

    for f in deleted + removed:
        del man[f]

    return man

def forgetremoved(m2, status):
    """
    Forget removed files

    If we're jumping between revisions (as opposed to merging), and if
    neither the working directory nor the target rev has the file,
    then we need to remove it from the dirstate, to prevent the
    dirstate from listing the file when it is no longer in the
    manifest.
    """

    modified, added, removed, deleted, unknown = status[:5]
    action = []

    for f in deleted + removed:
        if f not in m2:
            action.append((f, "f"))

    return action

def nonoverlap(d1, d2):
    """
    Return list of elements in d1 not in d2
    """

    l = []
    for d in d1:
        if d not in d2:
            l.append(d)

    l.sort()
    return l

def findold(fctx, limit):
    """
    find files that path was copied from, back to linkrev limit
    """

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

def findcopies(repo, m1, m2, limit):
    """
    Find moves and copies between m1 and m2 back to limit linkrev
    """

    # avoid silly behavior for update from empty dir
    if not m1:
        return {}

    dcopies = repo.dirstate.copies()
    copy = {}
    match = {}
    u1 = nonoverlap(m1, m2)
    u2 = nonoverlap(m2, m1)
    ctx = util.cachefunc(lambda f,n: repo.filectx(f, fileid=n[:20]))

    def checkpair(c, f2, man):
        ''' check if an apparent pair actually matches '''
        c2 = ctx(f2, man[f2])
        ca = c.ancestor(c2)
        if ca:
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

def manifestmerge(ui, m1, m2, ma, overwrite, backwards, partial):
    """
    Merge manifest m1 with m2 using ancestor ma and generate merge action list
    """

    def fmerge(f):
        """merge executable flags"""
        a, b, c = ma.execf(f), m1.execf(f), m2.execf(f)
        return ((a^b) | (a^c)) ^ a

    action = []

    def act(msg, f, m, *args):
        ui.debug(" %s: %s -> %s\n" % (f, msg, m))
        action.append((f, m) + args)

    # Filter manifests
    if partial:
        for f in m1.keys():
            if not partial(f): del m1[f]
        for f in m2.keys():
            if not partial(f): del m2[f]

    # Compare manifests
    for f, n in m1.iteritems():
        if f in m2:
            # are files different?
            if n != m2[f]:
                a = ma.get(f, nullid)
                # are both different from the ancestor?
                if not overwrite and n != a and m2[f] != a:
                    act("versions differ", f, "m", fmerge(f), n[:20], m2[f])
                # are we clobbering?
                # is remote's version newer?
                # or are we going back in time and clean?
                elif overwrite or m2[f] != a or (backwards and not n[20:]):
                    act("remote is newer", f, "g", m2.execf(f), m2[f])
                # local is newer, not overwrite, check mode bits
                elif fmerge(f) != m1.execf(f):
                    act("update permissions", f, "e", m2.execf(f))
            # contents same, check mode bits
            elif m1.execf(f) != m2.execf(f):
                if overwrite or fmerge(f) != m1.execf(f):
                    act("update permissions", f, "e", m2.execf(f))
            del m2[f]
        elif f in ma:
            if n != ma[f] and not overwrite:
                if ui.prompt(
                    (_(" local changed %s which remote deleted\n") % f) +
                    _("(k)eep or (d)elete?"), _("[kd]"), _("k")) == _("d"):
                    act("prompt delete", f, "r")
            else:
                act("other deleted", f, "r")
        else:
            # file is created on branch or in working directory
            if (overwrite and n[20:] != "u") or (backwards and not n[20:]):
                act("remote deleted", f, "r")

    for f, n in m2.iteritems():
        if f in ma:
            if overwrite or backwards:
                act("recreating", f, "g", m2.execf(f), n)
            elif n != ma[f]:
                if ui.prompt(
                    (_("remote changed %s which local deleted\n") % f) +
                    _("(k)eep or (d)elete?"), _("[kd]"), _("k")) == _("k"):
                    act("prompt recreating", f, "g", m2.execf(f), n)
        else:
            act("remote created", f, "g", m2.execf(f), n)

    return action

def applyupdates(repo, action, xp1, xp2):
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
            flag, my, other = a[2:]
            repo.ui.status(_("merging %s\n") % f)
            if merge3(repo, f, my, other, xp1, xp2):
                unresolved += 1
            util.set_exec(repo.wjoin(f), flag)
            merged += 1
        elif m == "g": # get
            flag, node = a[2:]
            repo.ui.note(_("getting %s\n") % f)
            t = repo.file(f).read(node)
            repo.wwrite(f, t)
            util.set_exec(repo.wjoin(f), flag)
            updated += 1
        elif m == "e": # exec
            flag = a[2:]
            util.set_exec(repo.wjoin(f), flag)

    return updated, merged, removed, unresolved

def recordupdates(repo, action, branchmerge):
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
            flag, my, other = a[2:]
            if branchmerge:
                # We've done a branch merge, mark this file as merged
                # so that we properly record the merger later
                repo.dirstate.update([f], 'm')
            else:
                # We've update-merged a locally modified file, so
                # we set the dirstate to emulate a normal checkout
                # of that file some time in the past. Thus our
                # merge will appear as a normal local file
                # modification.
                fl = repo.file(f)
                f_len = fl.size(fl.rev(other))
                repo.dirstate.update([f], 'n', st_size=f_len, st_mtime=-1)

def update(repo, node, branchmerge=False, force=False, partial=None,
           wlock=None, show_stats=True, remind=True):

    overwrite = force and not branchmerge
    forcemerge = force and branchmerge

    if not wlock:
        wlock = repo.wlock()

    ### check phase

    pl = repo.dirstate.parents()
    if not overwrite and pl[1] != nullid:
        raise util.Abort(_("outstanding uncommitted merges"))

    p1, p2 = pl[0], node
    pa = repo.changelog.ancestor(p1, p2)

    # are we going backwards?
    backwards = (pa == p2)

    # is there a linear path from p1 to p2?
    if pa == p1 or pa == p2:
        if branchmerge:
            raise util.Abort(_("there is nothing to merge, just use "
                               "'hg update' or look at 'hg heads'"))
    elif not (overwrite or branchmerge):
        raise util.Abort(_("update spans branches, use 'hg merge' "
                           "or 'hg update -C' to lose changes"))

    status = repo.status()
    modified, added, removed, deleted, unknown = status[:5]
    if branchmerge and not forcemerge:
        if modified or added or removed:
            raise util.Abort(_("outstanding uncommitted changes"))

    m1 = repo.changectx(p1).manifest().copy()
    m2 = repo.changectx(p2).manifest().copy()
    ma = repo.changectx(pa).manifest()

    # resolve the manifest to determine which files
    # we care about merging
    repo.ui.note(_("resolving manifests\n"))
    repo.ui.debug(_(" overwrite %s branchmerge %s partial %s\n") %
                  (overwrite, branchmerge, bool(partial)))
    repo.ui.debug(_(" ancestor %s local %s remote %s\n") %
                  (short(p1), short(p2), short(pa)))

    action = []

    copy = {}
    if not (backwards or overwrite):
        copy = findcopies(repo, m1, m2, repo.changelog.rev(pa))

    m1 = workingmanifest(repo, m1, status)

    if not force:
        checkunknown(repo, m2, status)
    if not branchmerge:
        action += forgetremoved(m2, status)

    action += manifestmerge(repo.ui, m1, m2, ma, overwrite, backwards, partial)
    del m1, m2, ma

    ### apply phase

    if not branchmerge:
        # we don't need to do any magic, just jump to the new rev
        p1, p2 = p2, nullid

    xp1, xp2 = hex(p1), hex(p2)
    if p2 == nullid: xp2 = ''

    repo.hook('preupdate', throw=True, parent1=xp1, parent2=xp2)

    updated, merged, removed, unresolved = applyupdates(repo, action, xp1, xp2)

    # update dirstate
    if not partial:
        repo.dirstate.setparents(p1, p2)
        recordupdates(repo, action, branchmerge)

    if show_stats:
        stats = ((updated, _("updated")),
                 (merged - unresolved, _("merged")),
                 (removed, _("removed")),
                 (unresolved, _("unresolved")))
        note = ", ".join([_("%d files %s") % s for s in stats])
        repo.ui.status("%s\n" % note)
    if not partial:
        if branchmerge:
            if unresolved:
                repo.ui.status(_("There are unresolved merges,"
                                " you can redo the full merge using:\n"
                                "  hg update -C %s\n"
                                "  hg merge %s\n"
                                % (repo.changelog.rev(p1),
                                    repo.changelog.rev(p2))))
            elif remind:
                repo.ui.status(_("(branch merge, don't forget to commit)\n"))
        elif unresolved:
            repo.ui.status(_("There are unresolved merges with"
                             " locally modified files.\n"))

    repo.hook('update', parent1=xp1, parent2=xp2, error=unresolved)
    return unresolved

