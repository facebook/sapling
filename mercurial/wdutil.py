# wdutil.py - working dir utilities
#
# Copyright 2011 Patrick Mezard <pmezard@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import glob, os
import util, similar, scmutil
import match as matchmod
from i18n import _

def expandpats(pats):
    if not util.expandglobs:
        return list(pats)
    ret = []
    for p in pats:
        kind, name = matchmod._patsplit(p, None)
        if kind is None:
            try:
                globbed = glob.glob(name)
            except re.error:
                globbed = [name]
            if globbed:
                ret.extend(globbed)
                continue
        ret.append(p)
    return ret

def match(repo, pats=[], opts={}, globbed=False, default='relpath'):
    if pats == ("",):
        pats = []
    if not globbed and default == 'relpath':
        pats = expandpats(pats or [])
    m = matchmod.match(repo.root, repo.getcwd(), pats,
                       opts.get('include'), opts.get('exclude'), default,
                       auditor=repo.auditor)
    def badfn(f, msg):
        repo.ui.warn("%s: %s\n" % (m.rel(f), msg))
    m.bad = badfn
    return m

def matchall(repo):
    return matchmod.always(repo.root, repo.getcwd())

def matchfiles(repo, files):
    return matchmod.exact(repo.root, repo.getcwd(), files)

def addremove(repo, pats=[], opts={}, dry_run=None, similarity=None):
    if dry_run is None:
        dry_run = opts.get('dry_run')
    if similarity is None:
        similarity = float(opts.get('similarity') or 0)
    # we'd use status here, except handling of symlinks and ignore is tricky
    added, unknown, deleted, removed = [], [], [], []
    audit_path = scmutil.pathauditor(repo.root)
    m = match(repo, pats, opts)
    for abs in repo.walk(m):
        target = repo.wjoin(abs)
        good = True
        try:
            audit_path(abs)
        except (OSError, util.Abort):
            good = False
        rel = m.rel(abs)
        exact = m.exact(abs)
        if good and abs not in repo.dirstate:
            unknown.append(abs)
            if repo.ui.verbose or not exact:
                repo.ui.status(_('adding %s\n') % ((pats and rel) or abs))
        elif repo.dirstate[abs] != 'r' and (not good or not os.path.lexists(target)
            or (os.path.isdir(target) and not os.path.islink(target))):
            deleted.append(abs)
            if repo.ui.verbose or not exact:
                repo.ui.status(_('removing %s\n') % ((pats and rel) or abs))
        # for finding renames
        elif repo.dirstate[abs] == 'r':
            removed.append(abs)
        elif repo.dirstate[abs] == 'a':
            added.append(abs)
    copies = {}
    if similarity > 0:
        for old, new, score in similar.findrenames(repo,
                added + unknown, removed + deleted, similarity):
            if repo.ui.verbose or not m.exact(old) or not m.exact(new):
                repo.ui.status(_('recording removal of %s as rename to %s '
                                 '(%d%% similar)\n') %
                               (m.rel(old), m.rel(new), score * 100))
            copies[new] = old

    if not dry_run:
        wctx = repo[None]
        wlock = repo.wlock()
        try:
            wctx.remove(deleted)
            wctx.add(unknown)
            for new, old in copies.iteritems():
                wctx.copy(old, new)
        finally:
            wlock.release()

def updatedir(ui, repo, patches, similarity=0):
    '''Update dirstate after patch application according to metadata'''
    if not patches:
        return []
    copies = []
    removes = set()
    cfiles = patches.keys()
    cwd = repo.getcwd()
    if cwd:
        cfiles = [util.pathto(repo.root, cwd, f) for f in patches.keys()]
    for f in patches:
        gp = patches[f]
        if not gp:
            continue
        if gp.op == 'RENAME':
            copies.append((gp.oldpath, gp.path))
            removes.add(gp.oldpath)
        elif gp.op == 'COPY':
            copies.append((gp.oldpath, gp.path))
        elif gp.op == 'DELETE':
            removes.add(gp.path)

    wctx = repo[None]
    for src, dst in copies:
        dirstatecopy(ui, repo, wctx, src, dst, cwd=cwd)
    if (not similarity) and removes:
        wctx.remove(sorted(removes), True)

    for f in patches:
        gp = patches[f]
        if gp and gp.mode:
            islink, isexec = gp.mode
            dst = repo.wjoin(gp.path)
            # patch won't create empty files
            if gp.op == 'ADD' and not os.path.lexists(dst):
                flags = (isexec and 'x' or '') + (islink and 'l' or '')
                repo.wwrite(gp.path, '', flags)
            util.setflags(dst, islink, isexec)
    addremove(repo, cfiles, similarity=similarity)
    files = patches.keys()
    files.extend([r for r in removes if r not in files])
    return sorted(files)

def dirstatecopy(ui, repo, wctx, src, dst, dryrun=False, cwd=None):
    """Update the dirstate to reflect the intent of copying src to dst. For
    different reasons it might not end with dst being marked as copied from src.
    """
    origsrc = repo.dirstate.copied(src) or src
    if dst == origsrc: # copying back a copy?
        if repo.dirstate[dst] not in 'mn' and not dryrun:
            repo.dirstate.normallookup(dst)
    else:
        if repo.dirstate[origsrc] == 'a' and origsrc == src:
            if not ui.quiet:
                ui.warn(_("%s has not been committed yet, so no copy "
                          "data will be stored for %s.\n")
                        % (repo.pathto(origsrc, cwd), repo.pathto(dst, cwd)))
            if repo.dirstate[dst] in '?r' and not dryrun:
                wctx.add([dst])
        elif not dryrun:
            wctx.copy(origsrc, dst)
