"""strip changesets and their descendants from history

This extension allows you to strip changesets and all their descendants from the
repository. See the command help for details.
"""
from __future__ import absolute_import

from mercurial import (
    bookmarks as bookmarksmod,
    cmdutil,
    error,
    hg,
    lock as lockmod,
    merge,
    node as nodemod,
    repair,
    scmutil,
    util,
)
from mercurial.i18n import _
nullid = nodemod.nullid
release = lockmod.release

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
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
            raise error.Abort(_("local changes found" + excsuffix))
        if checksubstate(repo):
            _("local changed subrepos found") # i18n tool detection
            raise error.Abort(_("local changed subrepos found" + excsuffix))
    return s

def strip(ui, repo, revs, update=True, backup=True, force=None, bookmarks=None):
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
            repo.dirstate.write(repo.currenttransaction())

        repair.strip(ui, repo, revs, backup)

        repomarks = repo._bookmarks
        if bookmarks:
            with repo.transaction('strip') as tr:
                if repo._activebookmark in bookmarks:
                    bookmarksmod.deactivate(repo)
                for bookmark in bookmarks:
                    del repomarks[bookmark]
                repomarks.recordchange(tr)
            for bookmark in sorted(bookmarks):
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
          ('B', 'bookmark', [], _("remove revs only reachable from given"
                                  " bookmark"))],
          _('hg strip [-k] [-f] [-B bookmark] [-r] REV...'))
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

    with repo.wlock():
        bookmarks = set(opts.get('bookmark'))
        if bookmarks:
            repomarks = repo._bookmarks
            if not bookmarks.issubset(repomarks):
                raise error.Abort(_("bookmark '%s' not found") %
                    ','.join(sorted(bookmarks - set(repomarks.keys()))))

            # If the requested bookmark is not the only one pointing to a
            # a revision we have to only delete the bookmark and not strip
            # anything. revsets cannot detect that case.
            nodetobookmarks = {}
            for mark, node in repomarks.iteritems():
                nodetobookmarks.setdefault(node, []).append(mark)
            for marks in nodetobookmarks.values():
                if bookmarks.issuperset(marks):
                    rsrevs = repair.stripbmrevset(repo, marks[0])
                    revs.update(set(rsrevs))
            if not revs:
                lock = tr = None
                try:
                    lock = repo.lock()
                    tr = repo.transaction('bookmark')
                    for bookmark in bookmarks:
                        del repomarks[bookmark]
                    repomarks.recordchange(tr)
                    tr.close()
                    for bookmark in sorted(bookmarks):
                        ui.write(_("bookmark '%s' deleted\n") % bookmark)
                finally:
                    release(lock, tr)

        if not revs:
            raise error.Abort(_('empty revision set'))

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
            repo.dirstate.write(repo.currenttransaction())

            # clear resolve state
            merge.mergestate.clean(repo, repo['.'].node())

            update = False


        strip(ui, repo, revs, backup=backup, update=update,
              force=opts.get('force'), bookmarks=bookmarks)

    return 0
