# merge.py - directory-level update/merge handling for Mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from i18n import gettext as _
from demandload import *
demandload(globals(), "util os tempfile")

def fmerge(f, local, other, ancestor):
    """merge executable flags"""
    a, b, c = ancestor.execf(f), local.execf(f), other.execf(f)
    return ((a^b) | (a^c)) ^ a

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
    linear_path = (pa == p1 or pa == p2)
    if branchmerge and linear_path:
        raise util.Abort(_("there is nothing to merge, just use "
                           "'hg update' or look at 'hg heads'"))

    if not linear_path and not (overwrite or branchmerge):
        raise util.Abort(_("update spans branches, use 'hg merge' "
                           "or 'hg update -C' to lose changes"))

    modified, added, removed, deleted, unknown = repo.status()[:5]
    if branchmerge and not forcemerge:
        if modified or added or removed:
            raise util.Abort(_("outstanding uncommitted changes"))

    m1 = repo.changectx(p1).manifest().copy()
    m2 = repo.changectx(p2).manifest().copy()
    ma = repo.changectx(pa).manifest()

    if not force:
        for f in unknown:
            if f in m2:
                if repo.file(f).cmp(m2[f], repo.wread(f)):
                    raise util.Abort(_("'%s' already exists in the working"
                                       " dir and differs from remote") % f)

    # resolve the manifest to determine which files
    # we care about merging
    repo.ui.note(_("resolving manifests\n"))
    repo.ui.debug(_(" overwrite %s branchmerge %s partial %s linear %s\n") %
                  (overwrite, branchmerge, bool(partial), linear_path))
    repo.ui.debug(_(" ancestor %s local %s remote %s\n") %
                  (short(p1), short(p2), short(pa)))

    action = {}
    forget = []

    # update m1 from working dir
    umap = dict.fromkeys(unknown)

    for f in added + modified + unknown:
        m1[f] = m1.get(f, nullid) + "+"
        m1.set(f, util.is_exec(repo.wjoin(f), m1.execf(f)))

    for f in deleted + removed:
        del m1[f]

        # If we're jumping between revisions (as opposed to merging),
        # and if neither the working directory nor the target rev has
        # the file, then we need to remove it from the dirstate, to
        # prevent the dirstate from listing the file when it is no
        # longer in the manifest.
        if linear_path and f not in m2:
            forget.append(f)

    if partial:
        for f in m1.keys():
            if not partial(f): del m1[f]
        for f in m2.keys():
            if not partial(f): del m2[f]

    # Compare manifests
    for f, n in m1.iteritems():
        if f in m2:
            queued = 0

            # are files different?
            if n != m2[f]:
                a = ma.get(f, nullid)
                # are both different from the ancestor?
                if not overwrite and n != a and m2[f] != a:
                    repo.ui.debug(_(" %s versions differ, resolve\n") % f)
                    action[f] = (fmerge(f, m1, m2, ma), n[:20], m2[f])
                    queued = 1
                # are we clobbering?
                # is remote's version newer?
                # or are we going back in time and clean?
                elif overwrite or m2[f] != a or (backwards and not n[20:]):
                    repo.ui.debug(_(" remote %s is newer, get\n") % f)
                    action[f] = (m2.execf(f), m2[f], None)
                    queued = 1
            elif f in umap or f in added:
                # this unknown file is the same as the checkout
                # we need to reset the dirstate if the file was added
                action[f] = (m2.execf(f), m2[f], None)

            # do we still need to look at mode bits?
            if not queued and m1.execf(f) != m2.execf(f):
                if overwrite:
                    repo.ui.debug(_(" updating permissions for %s\n") % f)
                    util.set_exec(repo.wjoin(f), m2.execf(f))
                else:
                    if fmerge(f, m1, m2, ma) != m1.execf(f):
                        repo.ui.debug(_(" updating permissions for %s\n")
                                      % f)
                        util.set_exec(repo.wjoin(f), mode)
            del m2[f]
        elif f in ma:
            if n != ma[f]:
                r = _("d")
                if not overwrite:
                    r = repo.ui.prompt(
                        (_(" local changed %s which remote deleted\n") % f) +
                         _("(k)eep or (d)elete?"), _("[kd]"), _("k"))
                if r == _("d"):
                    action[f] = (None, None, None)
            else:
                repo.ui.debug(_("other deleted %s\n") % f)
                action[f] = (None, None, None)
        else:
            # file is created on branch or in working directory
            if overwrite and f not in umap:
                repo.ui.debug(_("remote deleted %s, clobbering\n") % f)
                action[f] = (None, None, None)
            elif not n[20:]: # same as parent
                if backwards:
                    repo.ui.debug(_("remote deleted %s\n") % f)
                    action[f] = (None, None, None)
                else:
                    repo.ui.debug(_("local modified %s, keeping\n") % f)
            else:
                repo.ui.debug(_("working dir created %s, keeping\n") % f)

    for f, n in m2.iteritems():
        if f[0] == "/":
            continue
        if f in ma and n != ma[f]:
            r = _("k")
            if not overwrite:
                r = repo.ui.prompt(
                    (_("remote changed %s which local deleted\n") % f) +
                     _("(k)eep or (d)elete?"), _("[kd]"), _("k"))
            if r == _("k"):
                action[f] = (m2.execf(f), n, None)
        elif f not in ma:
            repo.ui.debug(_("remote created %s\n") % f)
            action[f] = (m2.execf(f), n, None)
        else:
            if overwrite or backwards:
                repo.ui.debug(_("local deleted %s, recreating\n") % f)
                action[f] = (m2.execf(f), n, None)
            else:
                repo.ui.debug(_("local deleted %s\n") % f)

    del m1, m2, ma

    ### apply phase

    if linear_path or overwrite:
        # we don't need to do any magic, just jump to the new rev
        p1, p2 = p2, nullid

    xp1 = hex(p1)
    xp2 = hex(p2)
    if p2 == nullid: xxp2 = ''
    else: xxp2 = xp2

    repo.hook('preupdate', throw=True, parent1=xp1, parent2=xxp2)

    # update files
    unresolved = []
    updated, merged, removed = 0, 0, 0
    files = action.keys()
    files.sort()
    for f in files:
        flag, my, other = action[f]
        if f[0] == "/":
            continue
        if not my:
            repo.ui.note(_("removing %s\n") % f)
            util.audit_path(f)
            try:
                util.unlink(repo.wjoin(f))
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    repo.ui.warn(_("update failed to remove %s: %s!\n") %
                                 (f, inst.strerror))
            removed +=1
        elif other:
            repo.ui.status(_("merging %s\n") % f)
            if merge3(repo, f, my, other, xp1, xp2):
                unresolved.append(f)
            util.set_exec(repo.wjoin(f), flag)
            merged += 1
        else:
            repo.ui.note(_("getting %s\n") % f)
            t = repo.file(f).read(my)
            repo.wwrite(f, t)
            util.set_exec(repo.wjoin(f), flag)
            updated += 1

    # update dirstate
    if not partial:
        repo.dirstate.setparents(p1, p2)
        repo.dirstate.forget(forget)
        files = action.keys()
        files.sort()
        for f in files:
            flag, my, other = action[f]
            if not my:
                if branchmerge:
                    repo.dirstate.update([f], 'r')
                else:
                    repo.dirstate.forget([f])
            elif not other:
                if branchmerge:
                    repo.dirstate.update([f], 'n', st_mtime=-1)
                else:
                    repo.dirstate.update([f], 'n')
            else:
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

    if show_stats:
        stats = ((updated, _("updated")),
                 (merged - len(unresolved), _("merged")),
                 (removed, _("removed")),
                 (len(unresolved), _("unresolved")))
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

    repo.hook('update', parent1=xp1, parent2=xxp2, error=len(unresolved))
    return len(unresolved)

