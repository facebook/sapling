# rebase.py - rebasing feature for mercurial
#
# Copyright 2008 Stefano Tortarolo <stefano.tortarolo at gmail dot com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''command to move sets of revisions to a different ancestor

This extension lets you rebase changesets in an existing Mercurial
repository.

For more information:
http://mercurial.selenic.com/wiki/RebaseExtension
'''

from mercurial import hg, util, repair, merge, cmdutil, commands, bookmarks
from mercurial import extensions, patch, scmutil, phases, obsolete, error
from mercurial import copies, repoview
from mercurial.commands import templateopts
from mercurial.node import nullrev, nullid, hex, short
from mercurial.lock import release
from mercurial.i18n import _
import os, errno

revtodo = -1
nullmerge = -2
revignored = -3

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def _savegraft(ctx, extra):
    s = ctx.extra().get('source', None)
    if s is not None:
        extra['source'] = s

def _savebranch(ctx, extra):
    extra['branch'] = ctx.branch()

def _makeextrafn(copiers):
    """make an extrafn out of the given copy-functions.

    A copy function takes a context and an extra dict, and mutates the
    extra dict as needed based on the given context.
    """
    def extrafn(ctx, extra):
        for c in copiers:
            c(ctx, extra)
    return extrafn

@command('rebase',
    [('s', 'source', '',
     _('rebase the specified changeset and descendants'), _('REV')),
    ('b', 'base', '',
     _('rebase everything from branching point of specified changeset'),
     _('REV')),
    ('r', 'rev', [],
     _('rebase these revisions'),
     _('REV')),
    ('d', 'dest', '',
     _('rebase onto the specified changeset'), _('REV')),
    ('', 'collapse', False, _('collapse the rebased changesets')),
    ('m', 'message', '',
     _('use text as collapse commit message'), _('TEXT')),
    ('e', 'edit', False, _('invoke editor on commit messages')),
    ('l', 'logfile', '',
     _('read collapse commit message from file'), _('FILE')),
    ('k', 'keep', False, _('keep original changesets')),
    ('', 'keepbranches', False, _('keep original branch names')),
    ('D', 'detach', False, _('(DEPRECATED)')),
    ('i', 'interactive', False, _('(DEPRECATED)')),
    ('t', 'tool', '', _('specify merge tool')),
    ('c', 'continue', False, _('continue an interrupted rebase')),
    ('a', 'abort', False, _('abort an interrupted rebase'))] +
     templateopts,
    _('[-s REV | -b REV] [-d REV] [OPTION]'))
def rebase(ui, repo, **opts):
    """move changeset (and descendants) to a different branch

    Rebase uses repeated merging to graft changesets from one part of
    history (the source) onto another (the destination). This can be
    useful for linearizing *local* changes relative to a master
    development tree.

    You should not rebase changesets that have already been shared
    with others. Doing so will force everybody else to perform the
    same rebase or they will end up with duplicated changesets after
    pulling in your rebased changesets.

    In its default configuration, Mercurial will prevent you from
    rebasing published changes. See :hg:`help phases` for details.

    If you don't specify a destination changeset (``-d/--dest``),
    rebase uses the current branch tip as the destination. (The
    destination changeset is not modified by rebasing, but new
    changesets are added as its descendants.)

    You can specify which changesets to rebase in two ways: as a
    "source" changeset or as a "base" changeset. Both are shorthand
    for a topologically related set of changesets (the "source
    branch"). If you specify source (``-s/--source``), rebase will
    rebase that changeset and all of its descendants onto dest. If you
    specify base (``-b/--base``), rebase will select ancestors of base
    back to but not including the common ancestor with dest. Thus,
    ``-b`` is less precise but more convenient than ``-s``: you can
    specify any changeset in the source branch, and rebase will select
    the whole branch. If you specify neither ``-s`` nor ``-b``, rebase
    uses the parent of the working directory as the base.

    For advanced usage, a third way is available through the ``--rev``
    option. It allows you to specify an arbitrary set of changesets to
    rebase. Descendants of revs you specify with this option are not
    automatically included in the rebase.

    By default, rebase recreates the changesets in the source branch
    as descendants of dest and then destroys the originals. Use
    ``--keep`` to preserve the original source changesets. Some
    changesets in the source branch (e.g. merges from the destination
    branch) may be dropped if they no longer contribute any change.

    One result of the rules for selecting the destination changeset
    and source branch is that, unlike ``merge``, rebase will do
    nothing if you are at the branch tip of a named branch
    with two heads. You need to explicitly specify source and/or
    destination (or ``update`` to the other head, if it's the head of
    the intended source branch).

    If a rebase is interrupted to manually resolve a merge, it can be
    continued with --continue/-c or aborted with --abort/-a.

    .. container:: verbose

      Examples:

      - move "local changes" (current commit back to branching point)
        to the current branch tip after a pull::

          hg rebase

      - move a single changeset to the stable branch::

          hg rebase -r 5f493448 -d stable

      - splice a commit and all its descendants onto another part of history::

          hg rebase --source c0c3 --dest 4cf9

      - rebase everything on a branch marked by a bookmark onto the
        default branch::

          hg rebase --base myfeature --dest default

      - collapse a sequence of changes into a single commit::

          hg rebase --collapse -r 1520:1525 -d .

      - move a named branch while preserving its name::

          hg rebase -r "branch(featureX)" -d 1.3 --keepbranches

    Returns 0 on success, 1 if nothing to rebase or there are
    unresolved conflicts.

    """
    originalwd = target = None
    activebookmark = None
    external = nullrev
    state = {}
    skipped = set()
    targetancestors = set()


    lock = wlock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()

        # Validate input and define rebasing points
        destf = opts.get('dest', None)
        srcf = opts.get('source', None)
        basef = opts.get('base', None)
        revf = opts.get('rev', [])
        contf = opts.get('continue')
        abortf = opts.get('abort')
        collapsef = opts.get('collapse', False)
        collapsemsg = cmdutil.logmessage(ui, opts)
        e = opts.get('extrafn') # internal, used by e.g. hgsubversion
        extrafns = [_savegraft]
        if e:
            extrafns = [e]
        keepf = opts.get('keep', False)
        keepbranchesf = opts.get('keepbranches', False)
        # keepopen is not meant for use on the command line, but by
        # other extensions
        keepopen = opts.get('keepopen', False)

        if opts.get('interactive'):
            msg = _("interactive history editing is supported by the "
                    "'histedit' extension (see \"hg help histedit\")")
            raise util.Abort(msg)

        if collapsemsg and not collapsef:
            raise util.Abort(
                _('message can only be specified with collapse'))

        if contf or abortf:
            if contf and abortf:
                raise util.Abort(_('cannot use both abort and continue'))
            if collapsef:
                raise util.Abort(
                    _('cannot use collapse with continue or abort'))
            if srcf or basef or destf:
                raise util.Abort(
                    _('abort and continue do not allow specifying revisions'))
            if opts.get('tool', False):
                ui.warn(_('tool option will be ignored\n'))

            try:
                (originalwd, target, state, skipped, collapsef, keepf,
                 keepbranchesf, external, activebookmark) = restorestatus(repo)
            except error.RepoLookupError:
                if abortf:
                    clearstatus(repo)
                    repo.ui.warn(_('rebase aborted (no revision is removed,'
                                   ' only broken state is cleared)\n'))
                    return 0
                else:
                    msg = _('cannot continue inconsistent rebase')
                    hint = _('use "hg rebase --abort" to clear broken state')
                    raise util.Abort(msg, hint=hint)
            if abortf:
                return abort(repo, originalwd, target, state,
                             activebookmark=activebookmark)
        else:
            if srcf and basef:
                raise util.Abort(_('cannot specify both a '
                                   'source and a base'))
            if revf and basef:
                raise util.Abort(_('cannot specify both a '
                                   'revision and a base'))
            if revf and srcf:
                raise util.Abort(_('cannot specify both a '
                                   'revision and a source'))

            cmdutil.checkunfinished(repo)
            cmdutil.bailifchanged(repo)

            if not destf:
                # Destination defaults to the latest revision in the
                # current branch
                branch = repo[None].branch()
                dest = repo[branch]
            else:
                dest = scmutil.revsingle(repo, destf)

            if revf:
                rebaseset = scmutil.revrange(repo, revf)
                if not rebaseset:
                    ui.status(_('empty "rev" revision set - '
                                'nothing to rebase\n'))
                    return 1
            elif srcf:
                src = scmutil.revrange(repo, [srcf])
                if not src:
                    ui.status(_('empty "source" revision set - '
                                'nothing to rebase\n'))
                    return 1
                rebaseset = repo.revs('(%ld)::', src)
                assert rebaseset
            else:
                base = scmutil.revrange(repo, [basef or '.'])
                if not base:
                    ui.status(_('empty "base" revision set - '
                                "can't compute rebase set\n"))
                    return 1
                commonanc = repo.revs('ancestor(%ld, %d)', base, dest).first()
                if commonanc is not None:
                    rebaseset = repo.revs('(%d::(%ld) - %d)::',
                                          commonanc, base, commonanc)
                else:
                    rebaseset = []

                if not rebaseset:
                    # transform to list because smartsets are not comparable to
                    # lists. This should be improved to honor laziness of
                    # smartset.
                    if list(base) == [dest.rev()]:
                        if basef:
                            ui.status(_('nothing to rebase - %s is both "base"'
                                        ' and destination\n') % dest)
                        else:
                            ui.status(_('nothing to rebase - working directory '
                                        'parent is also destination\n'))
                    elif not repo.revs('%ld - ::%d', base, dest):
                        if basef:
                            ui.status(_('nothing to rebase - "base" %s is '
                                        'already an ancestor of destination '
                                        '%s\n') %
                                      ('+'.join(str(repo[r]) for r in base),
                                       dest))
                        else:
                            ui.status(_('nothing to rebase - working '
                                        'directory parent is already an '
                                        'ancestor of destination %s\n') % dest)
                    else: # can it happen?
                        ui.status(_('nothing to rebase from %s to %s\n') %
                                  ('+'.join(str(repo[r]) for r in base), dest))
                    return 1

            allowunstable = obsolete.isenabled(repo, obsolete.allowunstableopt)
            if (not (keepf or allowunstable)
                  and repo.revs('first(children(%ld) - %ld)',
                                rebaseset, rebaseset)):
                raise util.Abort(
                    _("can't remove original changesets with"
                      " unrebased descendants"),
                    hint=_('use --keep to keep original changesets'))

            result = buildstate(repo, dest, rebaseset, collapsef)
            if not result:
                # Empty state built, nothing to rebase
                ui.status(_('nothing to rebase\n'))
                return 1

            root = min(rebaseset)
            if not keepf and not repo[root].mutable():
                raise util.Abort(_("can't rebase immutable changeset %s")
                                 % repo[root],
                                 hint=_('see "hg help phases" for details'))

            originalwd, target, state = result
            if collapsef:
                targetancestors = repo.changelog.ancestors([target],
                                                           inclusive=True)
                external = externalparent(repo, state, targetancestors)

            if dest.closesbranch() and not keepbranchesf:
                ui.status(_('reopening closed branch head %s\n') % dest)

        if keepbranchesf:
            # insert _savebranch at the start of extrafns so if
            # there's a user-provided extrafn it can clobber branch if
            # desired
            extrafns.insert(0, _savebranch)
            if collapsef:
                branches = set()
                for rev in state:
                    branches.add(repo[rev].branch())
                    if len(branches) > 1:
                        raise util.Abort(_('cannot collapse multiple named '
                            'branches'))

        # Rebase
        if not targetancestors:
            targetancestors = repo.changelog.ancestors([target], inclusive=True)

        # Keep track of the current bookmarks in order to reset them later
        currentbookmarks = repo._bookmarks.copy()
        activebookmark = activebookmark or repo._activebookmark
        if activebookmark:
            bookmarks.deactivate(repo)

        extrafn = _makeextrafn(extrafns)

        sortedstate = sorted(state)
        total = len(sortedstate)
        pos = 0
        for rev in sortedstate:
            ctx = repo[rev]
            desc = '%d:%s "%s"' % (ctx.rev(), ctx,
                                   ctx.description().split('\n', 1)[0])
            names = repo.nodetags(ctx.node()) + repo.nodebookmarks(ctx.node())
            if names:
                desc += ' (%s)' % ' '.join(names)
            pos += 1
            if state[rev] == revtodo:
                ui.status(_('rebasing %s\n') % desc)
                ui.progress(_("rebasing"), pos, ("%d:%s" % (rev, ctx)),
                            _('changesets'), total)
                p1, p2, base = defineparents(repo, rev, target, state,
                                             targetancestors)
                storestatus(repo, originalwd, target, state, collapsef, keepf,
                            keepbranchesf, external, activebookmark)
                if len(repo.parents()) == 2:
                    repo.ui.debug('resuming interrupted rebase\n')
                else:
                    try:
                        ui.setconfig('ui', 'forcemerge', opts.get('tool', ''),
                                     'rebase')
                        stats = rebasenode(repo, rev, p1, base, state,
                                           collapsef, target)
                        if stats and stats[3] > 0:
                            raise error.InterventionRequired(
                                _('unresolved conflicts (see hg '
                                  'resolve, then hg rebase --continue)'))
                    finally:
                        ui.setconfig('ui', 'forcemerge', '', 'rebase')
                if not collapsef:
                    merging = p2 != nullrev
                    editform = cmdutil.mergeeditform(merging, 'rebase')
                    editor = cmdutil.getcommiteditor(editform=editform, **opts)
                    newnode = concludenode(repo, rev, p1, p2, extrafn=extrafn,
                                           editor=editor)
                else:
                    # Skip commit if we are collapsing
                    repo.dirstate.beginparentchange()
                    repo.setparents(repo[p1].node())
                    repo.dirstate.endparentchange()
                    newnode = None
                # Update the state
                if newnode is not None:
                    state[rev] = repo[newnode].rev()
                    ui.debug('rebased as %s\n' % short(newnode))
                else:
                    ui.warn(_('note: rebase of %d:%s created no changes '
                              'to commit\n') % (rev, ctx))
                    if not collapsef:
                        skipped.add(rev)
                    state[rev] = p1
                    ui.debug('next revision set to %s\n' % p1)
            elif state[rev] == nullmerge:
                ui.debug('ignoring null merge rebase of %s\n' % rev)
            elif state[rev] == revignored:
                ui.status(_('not rebasing ignored %s\n') % desc)
            else:
                ui.status(_('already rebased %s as %s\n') %
                          (desc, repo[state[rev]]))

        ui.progress(_('rebasing'), None)
        ui.note(_('rebase merging completed\n'))

        if collapsef and not keepopen:
            p1, p2, _base = defineparents(repo, min(state), target,
                                          state, targetancestors)
            editopt = opts.get('edit')
            editform = 'rebase.collapse'
            if collapsemsg:
                commitmsg = collapsemsg
            else:
                commitmsg = 'Collapsed revision'
                for rebased in state:
                    if rebased not in skipped and state[rebased] > nullmerge:
                        commitmsg += '\n* %s' % repo[rebased].description()
                editopt = True
            editor = cmdutil.getcommiteditor(edit=editopt, editform=editform)
            newnode = concludenode(repo, rev, p1, external, commitmsg=commitmsg,
                                   extrafn=extrafn, editor=editor)
            if newnode is None:
                newrev = target
            else:
                newrev = repo[newnode].rev()
            for oldrev in state.iterkeys():
                if state[oldrev] > nullmerge:
                    state[oldrev] = newrev

        if 'qtip' in repo.tags():
            updatemq(repo, state, skipped, **opts)

        if currentbookmarks:
            # Nodeids are needed to reset bookmarks
            nstate = {}
            for k, v in state.iteritems():
                if v > nullmerge:
                    nstate[repo[k].node()] = repo[v].node()
            # XXX this is the same as dest.node() for the non-continue path --
            # this should probably be cleaned up
            targetnode = repo[target].node()

        # restore original working directory
        # (we do this before stripping)
        newwd = state.get(originalwd, originalwd)
        if newwd < 0:
            # original directory is a parent of rebase set root or ignored
            newwd = originalwd
        if newwd not in [c.rev() for c in repo[None].parents()]:
            ui.note(_("update back to initial working directory parent\n"))
            hg.updaterepo(repo, newwd, False)

        if not keepf:
            collapsedas = None
            if collapsef:
                collapsedas = newnode
            clearrebased(ui, repo, state, skipped, collapsedas)

        if currentbookmarks:
            updatebookmarks(repo, targetnode, nstate, currentbookmarks)
            if activebookmark not in repo._bookmarks:
                # active bookmark was divergent one and has been deleted
                activebookmark = None

        clearstatus(repo)
        ui.note(_("rebase completed\n"))
        util.unlinkpath(repo.sjoin('undo'), ignoremissing=True)
        if skipped:
            ui.note(_("%d revisions have been skipped\n") % len(skipped))

        if (activebookmark and
            repo['.'].node() == repo._bookmarks[activebookmark]):
                bookmarks.activate(repo, activebookmark)

    finally:
        release(lock, wlock)

def externalparent(repo, state, targetancestors):
    """Return the revision that should be used as the second parent
    when the revisions in state is collapsed on top of targetancestors.
    Abort if there is more than one parent.
    """
    parents = set()
    source = min(state)
    for rev in state:
        if rev == source:
            continue
        for p in repo[rev].parents():
            if (p.rev() not in state
                        and p.rev() not in targetancestors):
                parents.add(p.rev())
    if not parents:
        return nullrev
    if len(parents) == 1:
        return parents.pop()
    raise util.Abort(_('unable to collapse on top of %s, there is more '
                       'than one external parent: %s') %
                     (max(targetancestors),
                      ', '.join(str(p) for p in sorted(parents))))

def concludenode(repo, rev, p1, p2, commitmsg=None, editor=None, extrafn=None):
    '''Commit the wd changes with parents p1 and p2. Reuse commit info from rev
    but also store useful information in extra.
    Return node of committed revision.'''
    dsguard = cmdutil.dirstateguard(repo, 'rebase')
    try:
        repo.setparents(repo[p1].node(), repo[p2].node())
        ctx = repo[rev]
        if commitmsg is None:
            commitmsg = ctx.description()
        extra = {'rebase_source': ctx.hex()}
        if extrafn:
            extrafn(ctx, extra)

        backup = repo.ui.backupconfig('phases', 'new-commit')
        try:
            targetphase = max(ctx.phase(), phases.draft)
            repo.ui.setconfig('phases', 'new-commit', targetphase, 'rebase')
            # Commit might fail if unresolved files exist
            newnode = repo.commit(text=commitmsg, user=ctx.user(),
                                  date=ctx.date(), extra=extra, editor=editor)
        finally:
            repo.ui.restoreconfig(backup)

        repo.dirstate.setbranch(repo[newnode].branch())
        dsguard.close()
        return newnode
    finally:
        release(dsguard)

def rebasenode(repo, rev, p1, base, state, collapse, target):
    'Rebase a single revision rev on top of p1 using base as merge ancestor'
    # Merge phase
    # Update to target and merge it with local
    if repo['.'].rev() != p1:
        repo.ui.debug(" update to %d:%s\n" % (p1, repo[p1]))
        merge.update(repo, p1, False, True, False)
    else:
        repo.ui.debug(" already in target\n")
    repo.dirstate.write()
    repo.ui.debug(" merge against %d:%s\n" % (rev, repo[rev]))
    if base is not None:
        repo.ui.debug("   detach base %d:%s\n" % (base, repo[base]))
    # When collapsing in-place, the parent is the common ancestor, we
    # have to allow merging with it.
    stats = merge.update(repo, rev, True, True, False, base, collapse,
                        labels=['dest', 'source'])
    if collapse:
        copies.duplicatecopies(repo, rev, target)
    else:
        # If we're not using --collapse, we need to
        # duplicate copies between the revision we're
        # rebasing and its first parent, but *not*
        # duplicate any copies that have already been
        # performed in the destination.
        p1rev = repo[rev].p1().rev()
        copies.duplicatecopies(repo, rev, p1rev, skiprev=target)
    return stats

def nearestrebased(repo, rev, state):
    """return the nearest ancestors of rev in the rebase result"""
    rebased = [r for r in state if state[r] > nullmerge]
    candidates = repo.revs('max(%ld  and (::%d))', rebased, rev)
    if candidates:
        return state[candidates.first()]
    else:
        return None

def defineparents(repo, rev, target, state, targetancestors):
    'Return the new parent relationship of the revision that will be rebased'
    parents = repo[rev].parents()
    p1 = p2 = nullrev

    p1n = parents[0].rev()
    if p1n in targetancestors:
        p1 = target
    elif p1n in state:
        if state[p1n] == nullmerge:
            p1 = target
        elif state[p1n] == revignored:
            p1 = nearestrebased(repo, p1n, state)
            if p1 is None:
                p1 = target
        else:
            p1 = state[p1n]
    else: # p1n external
        p1 = target
        p2 = p1n

    if len(parents) == 2 and parents[1].rev() not in targetancestors:
        p2n = parents[1].rev()
        # interesting second parent
        if p2n in state:
            if p1 == target: # p1n in targetancestors or external
                p1 = state[p2n]
            elif state[p2n] == revignored:
                p2 = nearestrebased(repo, p2n, state)
                if p2 is None:
                    # no ancestors rebased yet, detach
                    p2 = target
            else:
                p2 = state[p2n]
        else: # p2n external
            if p2 != nullrev: # p1n external too => rev is a merged revision
                raise util.Abort(_('cannot use revision %d as base, result '
                        'would have 3 parents') % rev)
            p2 = p2n
    repo.ui.debug(" future parents are %d and %d\n" %
                            (repo[p1].rev(), repo[p2].rev()))

    if rev == min(state):
        # Case (1) initial changeset of a non-detaching rebase.
        # Let the merge mechanism find the base itself.
        base = None
    elif not repo[rev].p2():
        # Case (2) detaching the node with a single parent, use this parent
        base = repo[rev].p1().rev()
    else:
        # Assuming there is a p1, this is the case where there also is a p2.
        # We are thus rebasing a merge and need to pick the right merge base.
        #
        # Imagine we have:
        # - M: current rebase revision in this step
        # - A: one parent of M
        # - B: other parent of M
        # - D: destination of this merge step (p1 var)
        #
        # Consider the case where D is a descendant of A or B and the other is
        # 'outside'. In this case, the right merge base is the D ancestor.
        #
        # An informal proof, assuming A is 'outside' and B is the D ancestor:
        #
        # If we pick B as the base, the merge involves:
        # - changes from B to M (actual changeset payload)
        # - changes from B to D (induced by rebase) as D is a rebased
        #   version of B)
        # Which exactly represent the rebase operation.
        #
        # If we pick A as the base, the merge involves:
        # - changes from A to M (actual changeset payload)
        # - changes from A to D (with include changes between unrelated A and B
        #   plus changes induced by rebase)
        # Which does not represent anything sensible and creates a lot of
        # conflicts. A is thus not the right choice - B is.
        #
        # Note: The base found in this 'proof' is only correct in the specified
        # case. This base does not make sense if is not D a descendant of A or B
        # or if the other is not parent 'outside' (especially not if the other
        # parent has been rebased). The current implementation does not
        # make it feasible to consider different cases separately. In these
        # other cases we currently just leave it to the user to correctly
        # resolve an impossible merge using a wrong ancestor.
        for p in repo[rev].parents():
            if state.get(p.rev()) == p1:
                base = p.rev()
                break
        else: # fallback when base not found
            base = None

            # Raise because this function is called wrong (see issue 4106)
            raise AssertionError('no base found to rebase on '
                                 '(defineparents called wrong)')
    return p1, p2, base

def isagitpatch(repo, patchname):
    'Return true if the given patch is in git format'
    mqpatch = os.path.join(repo.mq.path, patchname)
    for line in patch.linereader(file(mqpatch, 'rb')):
        if line.startswith('diff --git'):
            return True
    return False

def updatemq(repo, state, skipped, **opts):
    'Update rebased mq patches - finalize and then import them'
    mqrebase = {}
    mq = repo.mq
    original_series = mq.fullseries[:]
    skippedpatches = set()

    for p in mq.applied:
        rev = repo[p.node].rev()
        if rev in state:
            repo.ui.debug('revision %d is an mq patch (%s), finalize it.\n' %
                                        (rev, p.name))
            mqrebase[rev] = (p.name, isagitpatch(repo, p.name))
        else:
            # Applied but not rebased, not sure this should happen
            skippedpatches.add(p.name)

    if mqrebase:
        mq.finish(repo, mqrebase.keys())

        # We must start import from the newest revision
        for rev in sorted(mqrebase, reverse=True):
            if rev not in skipped:
                name, isgit = mqrebase[rev]
                repo.ui.note(_('updating mq patch %s to %s:%s\n') %
                             (name, state[rev], repo[state[rev]]))
                mq.qimport(repo, (), patchname=name, git=isgit,
                                rev=[str(state[rev])])
            else:
                # Rebased and skipped
                skippedpatches.add(mqrebase[rev][0])

        # Patches were either applied and rebased and imported in
        # order, applied and removed or unapplied. Discard the removed
        # ones while preserving the original series order and guards.
        newseries = [s for s in original_series
                     if mq.guard_re.split(s, 1)[0] not in skippedpatches]
        mq.fullseries[:] = newseries
        mq.seriesdirty = True
        mq.savedirty()

def updatebookmarks(repo, targetnode, nstate, originalbookmarks):
    'Move bookmarks to their correct changesets, and delete divergent ones'
    marks = repo._bookmarks
    for k, v in originalbookmarks.iteritems():
        if v in nstate:
            # update the bookmarks for revs that have moved
            marks[k] = nstate[v]
            bookmarks.deletedivergent(repo, [targetnode], k)

    marks.write()

def storestatus(repo, originalwd, target, state, collapse, keep, keepbranches,
                external, activebookmark):
    'Store the current status to allow recovery'
    f = repo.vfs("rebasestate", "w")
    f.write(repo[originalwd].hex() + '\n')
    f.write(repo[target].hex() + '\n')
    f.write(repo[external].hex() + '\n')
    f.write('%d\n' % int(collapse))
    f.write('%d\n' % int(keep))
    f.write('%d\n' % int(keepbranches))
    f.write('%s\n' % (activebookmark or ''))
    for d, v in state.iteritems():
        oldrev = repo[d].hex()
        if v >= 0:
            newrev = repo[v].hex()
        elif v == revtodo:
            # To maintain format compatibility, we have to use nullid.
            # Please do remove this special case when upgrading the format.
            newrev = hex(nullid)
        else:
            newrev = v
        f.write("%s:%s\n" % (oldrev, newrev))
    f.close()
    repo.ui.debug('rebase status stored\n')

def clearstatus(repo):
    'Remove the status files'
    _clearrebasesetvisibiliy(repo)
    util.unlinkpath(repo.join("rebasestate"), ignoremissing=True)

def restorestatus(repo):
    'Restore a previously stored status'
    try:
        keepbranches = None
        target = None
        collapse = False
        external = nullrev
        activebookmark = None
        state = {}
        f = repo.vfs("rebasestate")
        for i, l in enumerate(f.read().splitlines()):
            if i == 0:
                originalwd = repo[l].rev()
            elif i == 1:
                target = repo[l].rev()
            elif i == 2:
                external = repo[l].rev()
            elif i == 3:
                collapse = bool(int(l))
            elif i == 4:
                keep = bool(int(l))
            elif i == 5:
                keepbranches = bool(int(l))
            elif i == 6 and not (len(l) == 81 and ':' in l):
                # line 6 is a recent addition, so for backwards compatibility
                # check that the line doesn't look like the oldrev:newrev lines
                activebookmark = l
            else:
                oldrev, newrev = l.split(':')
                if newrev in (str(nullmerge), str(revignored)):
                    state[repo[oldrev].rev()] = int(newrev)
                elif newrev == nullid:
                    state[repo[oldrev].rev()] = revtodo
                    # Legacy compat special case
                else:
                    state[repo[oldrev].rev()] = repo[newrev].rev()

        if keepbranches is None:
            raise util.Abort(_('.hg/rebasestate is incomplete'))

        skipped = set()
        # recompute the set of skipped revs
        if not collapse:
            seen = set([target])
            for old, new in sorted(state.items()):
                if new != revtodo and new in seen:
                    skipped.add(old)
                seen.add(new)
        repo.ui.debug('computed skipped revs: %s\n' %
                      (' '.join(str(r) for r in sorted(skipped)) or None))
        repo.ui.debug('rebase status resumed\n')
        _setrebasesetvisibility(repo, state.keys())
        return (originalwd, target, state, skipped,
                collapse, keep, keepbranches, external, activebookmark)
    except IOError, err:
        if err.errno != errno.ENOENT:
            raise
        raise util.Abort(_('no rebase in progress'))

def needupdate(repo, state):
    '''check whether we should `update --clean` away from a merge, or if
    somehow the working dir got forcibly updated, e.g. by older hg'''
    parents = [p.rev() for p in repo.parents()]

    # Are we in a merge state at all?
    if len(parents) < 2:
        return False

    # We should be standing on the first as-of-yet unrebased commit.
    firstunrebased = min([old for old, new in state.iteritems()
                          if new == nullrev])
    if firstunrebased in parents:
        return True

    return False

def abort(repo, originalwd, target, state, activebookmark=None):
    '''Restore the repository to its original state.  Additional args:

    activebookmark: the name of the bookmark that should be active after the
        restore'''
    dstates = [s for s in state.values() if s >= 0]
    immutable = [d for d in dstates if not repo[d].mutable()]
    cleanup = True
    if immutable:
        repo.ui.warn(_("warning: can't clean up immutable changesets %s\n")
                     % ', '.join(str(repo[r]) for r in immutable),
                     hint=_('see "hg help phases" for details'))
        cleanup = False

    descendants = set()
    if dstates:
        descendants = set(repo.changelog.descendants(dstates))
    if descendants - set(dstates):
        repo.ui.warn(_("warning: new changesets detected on target branch, "
                       "can't strip\n"))
        cleanup = False

    if cleanup:
        # Update away from the rebase if necessary
        if needupdate(repo, state):
            merge.update(repo, originalwd, False, True, False)

        # Strip from the first rebased revision
        rebased = filter(lambda x: x >= 0 and x != target, state.values())
        if rebased:
            strippoints = [c.node()  for c in repo.set('roots(%ld)', rebased)]
            # no backup of rebased cset versions needed
            repair.strip(repo.ui, repo, strippoints)

    if activebookmark and activebookmark in repo._bookmarks:
        bookmarks.activate(repo, activebookmark)

    clearstatus(repo)
    repo.ui.warn(_('rebase aborted\n'))
    return 0

def buildstate(repo, dest, rebaseset, collapse):
    '''Define which revisions are going to be rebased and where

    repo: repo
    dest: context
    rebaseset: set of rev
    '''
    _setrebasesetvisibility(repo, rebaseset)

    # This check isn't strictly necessary, since mq detects commits over an
    # applied patch. But it prevents messing up the working directory when
    # a partially completed rebase is blocked by mq.
    if 'qtip' in repo.tags() and (dest.node() in
                            [s.node for s in repo.mq.applied]):
        raise util.Abort(_('cannot rebase onto an applied mq patch'))

    roots = list(repo.set('roots(%ld)', rebaseset))
    if not roots:
        raise util.Abort(_('no matching revisions'))
    roots.sort()
    state = {}
    detachset = set()
    for root in roots:
        commonbase = root.ancestor(dest)
        if commonbase == root:
            raise util.Abort(_('source is ancestor of destination'))
        if commonbase == dest:
            samebranch = root.branch() == dest.branch()
            if not collapse and samebranch and root in dest.children():
                repo.ui.debug('source is a child of destination\n')
                return None

        repo.ui.debug('rebase onto %d starting from %s\n' % (dest, root))
        state.update(dict.fromkeys(rebaseset, revtodo))
        # Rebase tries to turn <dest> into a parent of <root> while
        # preserving the number of parents of rebased changesets:
        #
        # - A changeset with a single parent will always be rebased as a
        #   changeset with a single parent.
        #
        # - A merge will be rebased as merge unless its parents are both
        #   ancestors of <dest> or are themselves in the rebased set and
        #   pruned while rebased.
        #
        # If one parent of <root> is an ancestor of <dest>, the rebased
        # version of this parent will be <dest>. This is always true with
        # --base option.
        #
        # Otherwise, we need to *replace* the original parents with
        # <dest>. This "detaches" the rebased set from its former location
        # and rebases it onto <dest>. Changes introduced by ancestors of
        # <root> not common with <dest> (the detachset, marked as
        # nullmerge) are "removed" from the rebased changesets.
        #
        # - If <root> has a single parent, set it to <dest>.
        #
        # - If <root> is a merge, we cannot decide which parent to
        #   replace, the rebase operation is not clearly defined.
        #
        # The table below sums up this behavior:
        #
        # +------------------+----------------------+-------------------------+
        # |                  |     one parent       |  merge                  |
        # +------------------+----------------------+-------------------------+
        # | parent in        | new parent is <dest> | parents in ::<dest> are |
        # | ::<dest>         |                      | remapped to <dest>      |
        # +------------------+----------------------+-------------------------+
        # | unrelated source | new parent is <dest> | ambiguous, abort        |
        # +------------------+----------------------+-------------------------+
        #
        # The actual abort is handled by `defineparents`
        if len(root.parents()) <= 1:
            # ancestors of <root> not ancestors of <dest>
            detachset.update(repo.changelog.findmissingrevs([commonbase.rev()],
                                                            [root.rev()]))
    for r in detachset:
        if r not in state:
            state[r] = nullmerge
    if len(roots) > 1:
        # If we have multiple roots, we may have "hole" in the rebase set.
        # Rebase roots that descend from those "hole" should not be detached as
        # other root are. We use the special `revignored` to inform rebase that
        # the revision should be ignored but that `defineparents` should search
        # a rebase destination that make sense regarding rebased topology.
        rebasedomain = set(repo.revs('%ld::%ld', rebaseset, rebaseset))
        for ignored in set(rebasedomain) - set(rebaseset):
            state[ignored] = revignored
    return repo['.'].rev(), dest.rev(), state

def clearrebased(ui, repo, state, skipped, collapsedas=None):
    """dispose of rebased revision at the end of the rebase

    If `collapsedas` is not None, the rebase was a collapse whose result if the
    `collapsedas` node."""
    if obsolete.isenabled(repo, obsolete.createmarkersopt):
        markers = []
        for rev, newrev in sorted(state.items()):
            if newrev >= 0:
                if rev in skipped:
                    succs = ()
                elif collapsedas is not None:
                    succs = (repo[collapsedas],)
                else:
                    succs = (repo[newrev],)
                markers.append((repo[rev], succs))
        if markers:
            obsolete.createmarkers(repo, markers)
    else:
        rebased = [rev for rev in state if state[rev] > nullmerge]
        if rebased:
            stripped = []
            for root in repo.set('roots(%ld)', rebased):
                if set(repo.changelog.descendants([root.rev()])) - set(state):
                    ui.warn(_("warning: new changesets detected "
                              "on source branch, not stripping\n"))
                else:
                    stripped.append(root.node())
            if stripped:
                # backup the old csets by default
                repair.strip(ui, repo, stripped, "all")


def pullrebase(orig, ui, repo, *args, **opts):
    'Call rebase after pull if the latter has been invoked with --rebase'
    if opts.get('rebase'):
        if opts.get('update'):
            del opts['update']
            ui.debug('--update and --rebase are not compatible, ignoring '
                     'the update flag\n')

        movemarkfrom = repo['.'].node()
        revsprepull = len(repo)
        origpostincoming = commands.postincoming
        def _dummy(*args, **kwargs):
            pass
        commands.postincoming = _dummy
        try:
            orig(ui, repo, *args, **opts)
        finally:
            commands.postincoming = origpostincoming
        revspostpull = len(repo)
        if revspostpull > revsprepull:
            # --rev option from pull conflict with rebase own --rev
            # dropping it
            if 'rev' in opts:
                del opts['rev']
            # positional argument from pull conflicts with rebase's own
            # --source.
            if 'source' in opts:
                del opts['source']
            rebase(ui, repo, **opts)
            branch = repo[None].branch()
            dest = repo[branch].rev()
            if dest != repo['.'].rev():
                # there was nothing to rebase we force an update
                hg.update(repo, dest)
                if bookmarks.update(repo, [movemarkfrom], repo['.'].node()):
                    ui.status(_("updating bookmark %s\n")
                              % repo._activebookmark)
    else:
        if opts.get('tool'):
            raise util.Abort(_('--tool can only be used with --rebase'))
        orig(ui, repo, *args, **opts)

def _setrebasesetvisibility(repo, revs):
    """store the currently rebased set on the repo object

    This is used by another function to prevent rebased revision to because
    hidden (see issue4505)"""
    repo = repo.unfiltered()
    revs = set(revs)
    repo._rebaseset = revs
    # invalidate cache if visibility changes
    hiddens = repo.filteredrevcache.get('visible', set())
    if revs & hiddens:
        repo.invalidatevolatilesets()

def _clearrebasesetvisibiliy(repo):
    """remove rebaseset data from the repo"""
    repo = repo.unfiltered()
    if '_rebaseset' in vars(repo):
        del repo._rebaseset

def _rebasedvisible(orig, repo):
    """ensure rebased revs stay visible (see issue4505)"""
    blockers = orig(repo)
    blockers.update(getattr(repo, '_rebaseset', ()))
    return blockers

def summaryhook(ui, repo):
    if not os.path.exists(repo.join('rebasestate')):
        return
    try:
        state = restorestatus(repo)[2]
    except error.RepoLookupError:
        # i18n: column positioning for "hg summary"
        msg = _('rebase: (use "hg rebase --abort" to clear broken state)\n')
        ui.write(msg)
        return
    numrebased = len([i for i in state.itervalues() if i >= 0])
    # i18n: column positioning for "hg summary"
    ui.write(_('rebase: %s, %s (rebase --continue)\n') %
             (ui.label(_('%d rebased'), 'rebase.rebased') % numrebased,
              ui.label(_('%d remaining'), 'rebase.remaining') %
              (len(state) - numrebased)))

def uisetup(ui):
    #Replace pull with a decorator to provide --rebase option
    entry = extensions.wrapcommand(commands.table, 'pull', pullrebase)
    entry[1].append(('', 'rebase', None,
                     _("rebase working directory to branch head")))
    entry[1].append(('t', 'tool', '',
                     _("specify merge tool for rebase")))
    cmdutil.summaryhooks.add('rebase', summaryhook)
    cmdutil.unfinishedstates.append(
        ['rebasestate', False, False, _('rebase in progress'),
         _("use 'hg rebase --continue' or 'hg rebase --abort'")])
    # ensure rebased rev are not hidden
    extensions.wrapfunction(repoview, '_getdynamicblockers', _rebasedvisible)
