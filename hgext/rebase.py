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
from mercurial import extensions, patch, scmutil, phases
from mercurial.commands import templateopts
from mercurial.node import nullrev
from mercurial.lock import release
from mercurial.i18n import _
import os, errno

nullmerge = -2

cmdtable = {}
command = cmdutil.command(cmdtable)

@command('rebase',
    [('s', 'source', '',
     _('rebase from the specified changeset'), _('REV')),
    ('b', 'base', '',
     _('rebase from the base of the specified changeset '
       '(up to greatest common ancestor of base and dest)'),
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
    ('', 'keep', False, _('keep original changesets')),
    ('', 'keepbranches', False, _('keep original branch names')),
    ('D', 'detach', False, _('force detaching of source from its original '
                            'branch')),
    ('t', 'tool', '', _('specify merge tool')),
    ('c', 'continue', False, _('continue an interrupted rebase')),
    ('a', 'abort', False, _('abort an interrupted rebase'))] +
     templateopts,
    _('hg rebase [-s REV | -b REV] [-d REV] [options]\n'
      'hg rebase {-a|-c}'))
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

    If you don't specify a destination changeset (``-d/--dest``),
    rebase uses the tipmost head of the current named branch as the
    destination. (The destination changeset is not modified by
    rebasing, but new changesets are added as its descendants.)

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

    By default, rebase recreates the changesets in the source branch
    as descendants of dest and then destroys the originals. Use
    ``--keep`` to preserve the original source changesets. Some
    changesets in the source branch (e.g. merges from the destination
    branch) may be dropped if they no longer contribute any change.

    One result of the rules for selecting the destination changeset
    and source branch is that, unlike ``merge``, rebase will do
    nothing if you are at the latest (tipmost) head of a named branch
    with two heads. You need to explicitly specify source and/or
    destination (or ``update`` to the other head, if it's the head of
    the intended source branch).

    If a rebase is interrupted to manually resolve a merge, it can be
    continued with --continue/-c or aborted with --abort/-a.

    Returns 0 on success, 1 if nothing to rebase.
    """
    originalwd = target = None
    external = nullrev
    state = {}
    skipped = set()
    targetancestors = set()

    editor = None
    if opts.get('edit'):
        editor = cmdutil.commitforceeditor

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
        extrafn = opts.get('extrafn') # internal, used by e.g. hgsubversion
        keepf = opts.get('keep', False)
        keepbranchesf = opts.get('keepbranches', False)
        detachf = opts.get('detach', False)
        # keepopen is not meant for use on the command line, but by
        # other extensions
        keepopen = opts.get('keepopen', False)

        if collapsemsg and not collapsef:
            raise util.Abort(
                _('message can only be specified with collapse'))

        if contf or abortf:
            if contf and abortf:
                raise util.Abort(_('cannot use both abort and continue'))
            if collapsef:
                raise util.Abort(
                    _('cannot use collapse with continue or abort'))
            if detachf:
                raise util.Abort(_('cannot use detach with continue or abort'))
            if srcf or basef or destf:
                raise util.Abort(
                    _('abort and continue do not allow specifying revisions'))
            if opts.get('tool', False):
                ui.warn(_('tool option will be ignored\n'))

            (originalwd, target, state, skipped, collapsef, keepf,
                                keepbranchesf, external) = restorestatus(repo)
            if abortf:
                return abort(repo, originalwd, target, state)
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
            if detachf:
                if not (srcf or revf):
                    raise util.Abort(
                        _('detach requires a revision to be specified'))
                if basef:
                    raise util.Abort(_('cannot specify a base with detach'))

            cmdutil.bailifchanged(repo)

            if not destf:
                # Destination defaults to the latest revision in the
                # current branch
                branch = repo[None].branch()
                dest = repo[branch]
            else:
                dest = repo[destf]

            if revf:
                rebaseset = repo.revs('%lr', revf)
            elif srcf:
                src = scmutil.revrange(repo, [srcf])
                rebaseset = repo.revs('(%ld)::', src)
            else:
                base = scmutil.revrange(repo, [basef or '.'])
                rebaseset = repo.revs(
                    '(children(ancestor(%ld, %d)) and ::(%ld))::',
                    base, dest, base)

            if rebaseset:
                root = min(rebaseset)
            else:
                root = None

            if not rebaseset:
                repo.ui.debug('base is ancestor of destination')
                result = None
            elif not keepf and list(repo.revs('first(children(%ld) - %ld)',
                                              rebaseset, rebaseset)):
                raise util.Abort(
                    _("can't remove original changesets with"
                      " unrebased descendants"),
                    hint=_('use --keep to keep original changesets'))
            elif not keepf and not repo[root].mutable():
                raise util.Abort(_("can't rebase immutable changeset %s")
                                 % repo[root],
                                 hint=_('see hg help phases for details'))
            else:
                result = buildstate(repo, dest, rebaseset, detachf)

            if not result:
                # Empty state built, nothing to rebase
                ui.status(_('nothing to rebase\n'))
                return 1
            else:
                originalwd, target, state = result
                if collapsef:
                    targetancestors = set(repo.changelog.ancestors(target))
                    targetancestors.add(target)
                    external = checkexternal(repo, state, targetancestors)

        if keepbranchesf:
            assert not extrafn, 'cannot use both keepbranches and extrafn'
            def extrafn(ctx, extra):
                extra['branch'] = ctx.branch()
            if collapsef:
                branches = set()
                for rev in state:
                    branches.add(repo[rev].branch())
                    if len(branches) > 1:
                        raise util.Abort(_('cannot collapse multiple named '
                            'branches'))


        # Rebase
        if not targetancestors:
            targetancestors = set(repo.changelog.ancestors(target))
            targetancestors.add(target)

        # Keep track of the current bookmarks in order to reset them later
        currentbookmarks = repo._bookmarks.copy()

        sortedstate = sorted(state)
        total = len(sortedstate)
        pos = 0
        for rev in sortedstate:
            pos += 1
            if state[rev] == -1:
                ui.progress(_("rebasing"), pos, ("%d:%s" % (rev, repo[rev])),
                            _('changesets'), total)
                storestatus(repo, originalwd, target, state, collapsef, keepf,
                                                    keepbranchesf, external)
                p1, p2 = defineparents(repo, rev, target, state,
                                                        targetancestors)
                if len(repo.parents()) == 2:
                    repo.ui.debug('resuming interrupted rebase\n')
                else:
                    try:
                        ui.setconfig('ui', 'forcemerge', opts.get('tool', ''))
                        stats = rebasenode(repo, rev, p1, state)
                        if stats and stats[3] > 0:
                            raise util.Abort(_('unresolved conflicts (see hg '
                                        'resolve, then hg rebase --continue)'))
                    finally:
                        ui.setconfig('ui', 'forcemerge', '')
                cmdutil.duplicatecopies(repo, rev, target)
                if not collapsef:
                    newrev = concludenode(repo, rev, p1, p2, extrafn=extrafn,
                                          editor=editor)
                else:
                    # Skip commit if we are collapsing
                    repo.dirstate.setparents(repo[p1].node())
                    newrev = None
                # Update the state
                if newrev is not None:
                    state[rev] = repo[newrev].rev()
                else:
                    if not collapsef:
                        ui.note(_('no changes, revision %d skipped\n') % rev)
                        ui.debug('next revision set to %s\n' % p1)
                        skipped.add(rev)
                    state[rev] = p1

        ui.progress(_('rebasing'), None)
        ui.note(_('rebase merging completed\n'))

        if collapsef and not keepopen:
            p1, p2 = defineparents(repo, min(state), target,
                                                        state, targetancestors)
            if collapsemsg:
                commitmsg = collapsemsg
            else:
                commitmsg = 'Collapsed revision'
                for rebased in state:
                    if rebased not in skipped and state[rebased] != nullmerge:
                        commitmsg += '\n* %s' % repo[rebased].description()
                commitmsg = ui.edit(commitmsg, repo.ui.username())
            newrev = concludenode(repo, rev, p1, external, commitmsg=commitmsg,
                                  extrafn=extrafn, editor=editor)

        if 'qtip' in repo.tags():
            updatemq(repo, state, skipped, **opts)

        if currentbookmarks:
            # Nodeids are needed to reset bookmarks
            nstate = {}
            for k, v in state.iteritems():
                if v != nullmerge:
                    nstate[repo[k].node()] = repo[v].node()

        if not keepf:
            # Remove no more useful revisions
            rebased = [rev for rev in state if state[rev] != nullmerge]
            if rebased:
                if set(repo.changelog.descendants(min(rebased))) - set(state):
                    ui.warn(_("warning: new changesets detected "
                              "on source branch, not stripping\n"))
                else:
                    # backup the old csets by default
                    repair.strip(ui, repo, repo[min(rebased)].node(), "all")

        if currentbookmarks:
            updatebookmarks(repo, nstate, currentbookmarks, **opts)

        clearstatus(repo)
        ui.note(_("rebase completed\n"))
        if os.path.exists(repo.sjoin('undo')):
            util.unlinkpath(repo.sjoin('undo'))
        if skipped:
            ui.note(_("%d revisions have been skipped\n") % len(skipped))
    finally:
        release(lock, wlock)

def checkexternal(repo, state, targetancestors):
    """Check whether one or more external revisions need to be taken in
    consideration. In the latter case, abort.
    """
    external = nullrev
    source = min(state)
    for rev in state:
        if rev == source:
            continue
        # Check externals and fail if there are more than one
        for p in repo[rev].parents():
            if (p.rev() not in state
                        and p.rev() not in targetancestors):
                if external != nullrev:
                    raise util.Abort(_('unable to collapse, there is more '
                            'than one external parent'))
                external = p.rev()
    return external

def concludenode(repo, rev, p1, p2, commitmsg=None, editor=None, extrafn=None):
    'Commit the changes and store useful information in extra'
    try:
        repo.dirstate.setparents(repo[p1].node(), repo[p2].node())
        ctx = repo[rev]
        if commitmsg is None:
            commitmsg = ctx.description()
        extra = {'rebase_source': ctx.hex()}
        if extrafn:
            extrafn(ctx, extra)
        # Commit might fail if unresolved files exist
        newrev = repo.commit(text=commitmsg, user=ctx.user(),
                             date=ctx.date(), extra=extra, editor=editor)
        repo.dirstate.setbranch(repo[newrev].branch())
        targetphase = max(ctx.phase(), phases.draft)
        # retractboundary doesn't overwrite upper phase inherited from parent
        newnode = repo[newrev].node()
        if newnode:
            phases.retractboundary(repo, targetphase, [newnode])
        return newrev
    except util.Abort:
        # Invalidate the previous setparents
        repo.dirstate.invalidate()
        raise

def rebasenode(repo, rev, p1, state):
    'Rebase a single revision'
    # Merge phase
    # Update to target and merge it with local
    if repo['.'].rev() != repo[p1].rev():
        repo.ui.debug(" update to %d:%s\n" % (repo[p1].rev(), repo[p1]))
        merge.update(repo, p1, False, True, False)
    else:
        repo.ui.debug(" already in target\n")
    repo.dirstate.write()
    repo.ui.debug(" merge against %d:%s\n" % (repo[rev].rev(), repo[rev]))
    base = None
    if repo[rev].rev() != repo[min(state)].rev():
        base = repo[rev].p1().node()
    return merge.update(repo, rev, True, True, False, base)

def defineparents(repo, rev, target, state, targetancestors):
    'Return the new parent relationship of the revision that will be rebased'
    parents = repo[rev].parents()
    p1 = p2 = nullrev

    P1n = parents[0].rev()
    if P1n in targetancestors:
        p1 = target
    elif P1n in state:
        if state[P1n] == nullmerge:
            p1 = target
        else:
            p1 = state[P1n]
    else: # P1n external
        p1 = target
        p2 = P1n

    if len(parents) == 2 and parents[1].rev() not in targetancestors:
        P2n = parents[1].rev()
        # interesting second parent
        if P2n in state:
            if p1 == target: # P1n in targetancestors or external
                p1 = state[P2n]
            else:
                p2 = state[P2n]
        else: # P2n external
            if p2 != nullrev: # P1n external too => rev is a merged revision
                raise util.Abort(_('cannot use revision %d as base, result '
                        'would have 3 parents') % rev)
            p2 = P2n
    repo.ui.debug(" future parents are %d and %d\n" %
                            (repo[p1].rev(), repo[p2].rev()))
    return p1, p2

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

    for p in mq.applied:
        rev = repo[p.node].rev()
        if rev in state:
            repo.ui.debug('revision %d is an mq patch (%s), finalize it.\n' %
                                        (rev, p.name))
            mqrebase[rev] = (p.name, isagitpatch(repo, p.name))

    if mqrebase:
        mq.finish(repo, mqrebase.keys())

        # We must start import from the newest revision
        for rev in sorted(mqrebase, reverse=True):
            if rev not in skipped:
                name, isgit = mqrebase[rev]
                repo.ui.debug('import mq patch %d (%s)\n' % (state[rev], name))
                mq.qimport(repo, (), patchname=name, git=isgit,
                                rev=[str(state[rev])])

        # restore missing guards
        for s in original_series:
            pname = mq.guard_re.split(s, 1)[0]
            if pname in mq.fullseries:
                repo.ui.debug('restoring guard for patch %s' % (pname))
                mq.fullseries[mq.fullseries.index(pname)] = s
                mq.series_dirty = True
        mq.savedirty()

def updatebookmarks(repo, nstate, originalbookmarks, **opts):
    'Move bookmarks to their correct changesets'
    current = repo._bookmarkcurrent
    for k, v in originalbookmarks.iteritems():
        if v in nstate:
            if nstate[v] != nullmerge:
                # reset the pointer if the bookmark was moved incorrectly
                if k != current:
                    repo._bookmarks[k] = nstate[v]

    bookmarks.write(repo)

def storestatus(repo, originalwd, target, state, collapse, keep, keepbranches,
                                                                external):
    'Store the current status to allow recovery'
    f = repo.opener("rebasestate", "w")
    f.write(repo[originalwd].hex() + '\n')
    f.write(repo[target].hex() + '\n')
    f.write(repo[external].hex() + '\n')
    f.write('%d\n' % int(collapse))
    f.write('%d\n' % int(keep))
    f.write('%d\n' % int(keepbranches))
    for d, v in state.iteritems():
        oldrev = repo[d].hex()
        if v != nullmerge:
            newrev = repo[v].hex()
        else:
            newrev = v
        f.write("%s:%s\n" % (oldrev, newrev))
    f.close()
    repo.ui.debug('rebase status stored\n')

def clearstatus(repo):
    'Remove the status files'
    if os.path.exists(repo.join("rebasestate")):
        util.unlinkpath(repo.join("rebasestate"))

def restorestatus(repo):
    'Restore a previously stored status'
    try:
        target = None
        collapse = False
        external = nullrev
        state = {}
        f = repo.opener("rebasestate")
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
            else:
                oldrev, newrev = l.split(':')
                if newrev != str(nullmerge):
                    state[repo[oldrev].rev()] = repo[newrev].rev()
                else:
                    state[repo[oldrev].rev()] = int(newrev)
        skipped = set()
        # recompute the set of skipped revs
        if not collapse:
            seen = set([target])
            for old, new in sorted(state.items()):
                if new != nullrev and new in seen:
                    skipped.add(old)
                seen.add(new)
        repo.ui.debug('computed skipped revs: %s\n' % skipped)
        repo.ui.debug('rebase status resumed\n')
        return (originalwd, target, state, skipped,
                collapse, keep, keepbranches, external)
    except IOError, err:
        if err.errno != errno.ENOENT:
            raise
        raise util.Abort(_('no rebase in progress'))

def abort(repo, originalwd, target, state):
    'Restore the repository to its original state'
    dstates = [s for s in state.values() if s != nullrev]
    if [d for d in dstates if not repo[d].mutable()]:
        repo.ui.warn(_("warning: immutable rebased changeset detected, "
                       "can't abort\n"))
        return -1

    descendants = set()
    if dstates:
        descendants = set(repo.changelog.descendants(*dstates))
    if descendants - set(dstates):
        repo.ui.warn(_("warning: new changesets detected on target branch, "
                       "can't abort\n"))
        return -1
    else:
        # Strip from the first rebased revision
        merge.update(repo, repo[originalwd].rev(), False, True, False)
        rebased = filter(lambda x: x > -1 and x != target, state.values())
        if rebased:
            strippoint = min(rebased)
            # no backup of rebased cset versions needed
            repair.strip(repo.ui, repo, repo[strippoint].node())
        clearstatus(repo)
        repo.ui.warn(_('rebase aborted\n'))
        return 0

def buildstate(repo, dest, rebaseset, detach):
    '''Define which revisions are going to be rebased and where

    repo: repo
    dest: context
    rebaseset: set of rev
    detach: boolean'''

    # This check isn't strictly necessary, since mq detects commits over an
    # applied patch. But it prevents messing up the working directory when
    # a partially completed rebase is blocked by mq.
    if 'qtip' in repo.tags() and (dest.node() in
                            [s.node for s in repo.mq.applied]):
        raise util.Abort(_('cannot rebase onto an applied mq patch'))

    detachset = set()
    roots = list(repo.set('roots(%ld)', rebaseset))
    if not roots:
        raise util.Abort(_('no matching revisions'))
    if len(roots) > 1:
        raise util.Abort(_("can't rebase multiple roots"))
    root = roots[0]

    commonbase = root.ancestor(dest)
    if commonbase == root:
        raise util.Abort(_('source is ancestor of destination'))
    if commonbase == dest:
        samebranch = root.branch() == dest.branch()
        if samebranch and root in dest.children():
           repo.ui.debug('source is a child of destination')
           return None
        # rebase on ancestor, force detach
        detach = True
    if detach:
        detachset = repo.revs('::%d - ::%d - %d', root, commonbase, root)

    repo.ui.debug('rebase onto %d starting from %d\n' % (dest, root))
    state = dict.fromkeys(rebaseset, nullrev)
    state.update(dict.fromkeys(detachset, nullmerge))
    return repo['.'].rev(), dest.rev(), state

def pullrebase(orig, ui, repo, *args, **opts):
    'Call rebase after pull if the latter has been invoked with --rebase'
    if opts.get('rebase'):
        if opts.get('update'):
            del opts['update']
            ui.debug('--update and --rebase are not compatible, ignoring '
                     'the update flag\n')

        movemarkfrom = repo['.'].node()
        cmdutil.bailifchanged(repo)
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
            rebase(ui, repo, **opts)
            branch = repo[None].branch()
            dest = repo[branch].rev()
            if dest != repo['.'].rev():
                # there was nothing to rebase we force an update
                hg.update(repo, dest)
                if bookmarks.update(repo, [movemarkfrom], repo['.'].node()):
                    ui.status(_("updating bookmark %s\n")
                              % repo._bookmarkcurrent)
    else:
        if opts.get('tool'):
            raise util.Abort(_('--tool can only be used with --rebase'))
        orig(ui, repo, *args, **opts)

def uisetup(ui):
    'Replace pull with a decorator to provide --rebase option'
    entry = extensions.wrapcommand(commands.table, 'pull', pullrebase)
    entry[1].append(('', 'rebase', None,
                     _("rebase working directory to branch head")))
    entry[1].append(('t', 'tool', '',
                     _("specify merge tool for rebase")))
