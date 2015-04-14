"""strip changesets and their descendants from history

This extension allows you to strip changesets and all their descendants from the
repository. See the command help for details.
"""
from mercurial.i18n import _
from mercurial.node import nullid
from mercurial.lock import release
from mercurial import cmdutil, hg, scmutil, util
from mercurial import repair, bookmarks, merge

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def checksubstate(repo, baserev=None):
    '''return list of subrepos at a different revision than substate.
    Abort if any subrepos have uncommitted changes.'''
    inclsubs = []
    wctx = repo[None]
    if baserev:
        bctx = repo[baserev]
    else:
        bctx = wctx.parents()[0]
    for s in sorted(wctx.substate):
        wctx.sub(s).bailifchanged(True)
        if s not in bctx.substate or bctx.sub(s).dirty():
            inclsubs.append(s)
    return inclsubs

def checklocalchanges(repo, force=False, excsuffix=''):
    cmdutil.checkunfinished(repo)
    s = repo.status()
    if not force:
        if s.modified or s.added or s.removed or s.deleted:
            _("local changes found") # i18n tool detection
            raise util.Abort(_("local changes found" + excsuffix))
        if checksubstate(repo):
            _("local changed subrepos found") # i18n tool detection
            raise util.Abort(_("local changed subrepos found" + excsuffix))
    return s

def strip(ui, repo, revs, update=True, backup=True, force=None, bookmark=None):
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()

        if update:
            checklocalchanges(repo, force=force)
            urev, p2 = repo.changelog.parents(revs[0])
            if (util.safehasattr(repo, 'mq') and
                p2 != nullid
                and p2 in [x.node for x in repo.mq.applied]):
                urev = p2
            hg.clean(repo, urev)
            repo.dirstate.write()

        repair.strip(ui, repo, revs, backup)

        marks = repo._bookmarks
        if bookmark:
            if bookmark == repo._bookmarkcurrent:
                bookmarks.deactivate(repo)
            del marks[bookmark]
            marks.write()
            ui.write(_("bookmark '%s' deleted\n") % bookmark)
    finally:
        release(lock, wlock)


@command("strip",
         [
          ('r', 'rev', [], _('strip specified revision (optional, '
                               'can specify revisions without this '
                               'option)'), _('REV')),
          ('f', 'force', None, _('force removal of changesets, discard '
                                 'uncommitted changes (no backup)')),
          ('', 'no-backup', None, _('no backups')),
          ('', 'nobackup', None, _('no backups (DEPRECATED)')),
          ('n', '', None, _('ignored  (DEPRECATED)')),
          ('k', 'keep', None, _("do not modify working directory during "
                                "strip")),
          ('B', 'bookmark', '', _("remove revs only reachable from given"
                                  " bookmark"))],
          _('hg strip [-k] [-f] [-n] [-B bookmark] [-r] REV...'))
def stripcmd(ui, repo, *revs, **opts):
    """strip changesets and all their descendants from the repository

    The strip command removes the specified changesets and all their
    descendants. If the working directory has uncommitted changes, the
    operation is aborted unless the --force flag is supplied, in which
    case changes will be discarded.

    If a parent of the working directory is stripped, then the working
    directory will automatically be updated to the most recent
    available ancestor of the stripped parent after the operation
    completes.

    Any stripped changesets are stored in ``.hg/strip-backup`` as a
    bundle (see :hg:`help bundle` and :hg:`help unbundle`). They can
    be restored by running :hg:`unbundle .hg/strip-backup/BUNDLE`,
    where BUNDLE is the bundle file created by the strip. Note that
    the local revision numbers will in general be different after the
    restore.

    Use the --no-backup option to discard the backup bundle once the
    operation completes.

    Strip is not a history-rewriting operation and can be used on
    changesets in the public phase. But if the stripped changesets have
    been pushed to a remote repository you will likely pull them again.

    Return 0 on success.
    """
    backup = True
    if opts.get('no_backup') or opts.get('nobackup'):
        backup = False

    cl = repo.changelog
    revs = list(revs) + opts.get('rev')
    revs = set(scmutil.revrange(repo, revs))

    wlock = repo.wlock()
    try:
        if opts.get('bookmark'):
            mark = opts.get('bookmark')
            marks = repo._bookmarks
            if mark not in marks:
                raise util.Abort(_("bookmark '%s' not found") % mark)

            # If the requested bookmark is not the only one pointing to a
            # a revision we have to only delete the bookmark and not strip
            # anything. revsets cannot detect that case.
            uniquebm = True
            for m, n in marks.iteritems():
                if m != mark and n == repo[mark].node():
                    uniquebm = False
                    break
            if uniquebm:
                rsrevs = repo.revs("ancestors(bookmark(%s)) - "
                                   "ancestors(head() and not bookmark(%s)) - "
                                   "ancestors(bookmark() and not bookmark(%s))",
                                   mark, mark, mark)
                revs.update(set(rsrevs))
            if not revs:
                del marks[mark]
                marks.write()
                ui.write(_("bookmark '%s' deleted\n") % mark)

        if not revs:
            raise util.Abort(_('empty revision set'))

        descendants = set(cl.descendants(revs))
        strippedrevs = revs.union(descendants)
        roots = revs.difference(descendants)

        update = False
        # if one of the wdir parent is stripped we'll need
        # to update away to an earlier revision
        for p in repo.dirstate.parents():
            if p != nullid and cl.rev(p) in strippedrevs:
                update = True
                break

        rootnodes = set(cl.node(r) for r in roots)

        q = getattr(repo, 'mq', None)
        if q is not None and q.applied:
            # refresh queue state if we're about to strip
            # applied patches
            if cl.rev(repo.lookup('qtip')) in strippedrevs:
                q.applieddirty = True
                start = 0
                end = len(q.applied)
                for i, statusentry in enumerate(q.applied):
                    if statusentry.node in rootnodes:
                        # if one of the stripped roots is an applied
                        # patch, only part of the queue is stripped
                        start = i
                        break
                del q.applied[start:end]
                q.savedirty()

        revs = sorted(rootnodes)
        if update and opts.get('keep'):
            urev, p2 = repo.changelog.parents(revs[0])
            if (util.safehasattr(repo, 'mq') and p2 != nullid
                and p2 in [x.node for x in repo.mq.applied]):
                urev = p2
            uctx = repo[urev]

            # only reset the dirstate for files that would actually change
            # between the working context and uctx
            descendantrevs = repo.revs("%s::." % uctx.rev())
            changedfiles = []
            for rev in descendantrevs:
                # blindly reset the files, regardless of what actually changed
                changedfiles.extend(repo[rev].files())

            # reset files that only changed in the dirstate too
            dirstate = repo.dirstate
            dirchanges = [f for f in dirstate if dirstate[f] != 'n']
            changedfiles.extend(dirchanges)

            repo.dirstate.rebuild(urev, uctx.manifest(), changedfiles)
            repo.dirstate.write()

            # clear resolve state
            ms = merge.mergestate(repo)
            ms.reset(repo['.'].node())

            update = False


        strip(ui, repo, revs, backup=backup, update=update,
              force=opts.get('force'), bookmark=opts.get('bookmark'))
    finally:
        wlock.release()

    return 0
