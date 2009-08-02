# rebase.py - rebasing feature for mercurial
#
# Copyright 2008 Stefano Tortarolo <stefano.tortarolo at gmail dot com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

'''command to move sets of revisions to a different ancestor

This extension lets you rebase changesets in an existing Mercurial
repository.

For more information:
http://mercurial.selenic.com/wiki/RebaseExtension
'''

from mercurial import util, repair, merge, cmdutil, commands, error
from mercurial import extensions, ancestor, copies, patch
from mercurial.commands import templateopts
from mercurial.node import nullrev
from mercurial.lock import release
from mercurial.i18n import _
import os, errno

def rebasemerge(repo, rev, first=False):
    'return the correct ancestor'
    oldancestor = ancestor.ancestor

    def newancestor(a, b, pfunc):
        ancestor.ancestor = oldancestor
        if b == rev:
            return repo[rev].parents()[0].rev()
        return ancestor.ancestor(a, b, pfunc)

    if not first:
        ancestor.ancestor = newancestor
    else:
        repo.ui.debug(_("first revision, do not change ancestor\n"))
    stats = merge.update(repo, rev, True, True, False)
    return stats

def rebase(ui, repo, **opts):
    """move changeset (and descendants) to a different branch

    Rebase uses repeated merging to graft changesets from one part of
    history onto another. This can be useful for linearizing local
    changes relative to a master development tree.

    If a rebase is interrupted to manually resolve a merge, it can be
    continued with --continue/-c or aborted with --abort/-a.
    """
    originalwd = target = None
    external = nullrev
    state = {}
    skipped = set()

    lock = wlock = None
    try:
        lock = repo.lock()
        wlock = repo.wlock()

        # Validate input and define rebasing points
        destf = opts.get('dest', None)
        srcf = opts.get('source', None)
        basef = opts.get('base', None)
        contf = opts.get('continue')
        abortf = opts.get('abort')
        collapsef = opts.get('collapse', False)
        extrafn = opts.get('extrafn')
        keepf = opts.get('keep', False)
        keepbranchesf = opts.get('keepbranches', False)

        if contf or abortf:
            if contf and abortf:
                raise error.ParseError('rebase',
                                       _('cannot use both abort and continue'))
            if collapsef:
                raise error.ParseError(
                    'rebase', _('cannot use collapse with continue or abort'))

            if srcf or basef or destf:
                raise error.ParseError('rebase',
                    _('abort and continue do not allow specifying revisions'))

            (originalwd, target, state, collapsef, keepf,
                                keepbranchesf, external) = restorestatus(repo)
            if abortf:
                abort(repo, originalwd, target, state)
                return
        else:
            if srcf and basef:
                raise error.ParseError('rebase', _('cannot specify both a '
                                                   'revision and a base'))
            cmdutil.bail_if_changed(repo)
            result = buildstate(repo, destf, srcf, basef, collapsef)
            if result:
                originalwd, target, state, external = result
            else: # Empty state built, nothing to rebase
                ui.status(_('nothing to rebase\n'))
                return

        if keepbranchesf:
            if extrafn:
                raise error.ParseError(
                    'rebase', _('cannot use both keepbranches and extrafn'))
            def extrafn(ctx, extra):
                extra['branch'] = ctx.branch()

        # Rebase
        targetancestors = list(repo.changelog.ancestors(target))
        targetancestors.append(target)

        for rev in sorted(state):
            if state[rev] == -1:
                storestatus(repo, originalwd, target, state, collapsef, keepf,
                                                    keepbranchesf, external)
                rebasenode(repo, rev, target, state, skipped, targetancestors,
                                                       collapsef, extrafn)
        ui.note(_('rebase merging completed\n'))

        if collapsef:
            p1, p2 = defineparents(repo, min(state), target,
                                                        state, targetancestors)
            concludenode(repo, rev, p1, external, state, collapsef,
                         last=True, skipped=skipped, extrafn=extrafn)

        if 'qtip' in repo.tags():
            updatemq(repo, state, skipped, **opts)

        if not keepf:
            # Remove no more useful revisions
            if set(repo.changelog.descendants(min(state))) - set(state):
                ui.warn(_("warning: new changesets detected on source branch, "
                                                        "not stripping\n"))
            else:
                repair.strip(ui, repo, repo[min(state)].node(), "strip")

        clearstatus(repo)
        ui.status(_("rebase completed\n"))
        if os.path.exists(repo.sjoin('undo')):
            util.unlink(repo.sjoin('undo'))
        if skipped:
            ui.note(_("%d revisions have been skipped\n") % len(skipped))
    finally:
        release(lock, wlock)

def concludenode(repo, rev, p1, p2, state, collapse, last=False, skipped=None,
                 extrafn=None):
    """Skip commit if collapsing has been required and rev is not the last
    revision, commit otherwise
    """
    repo.ui.debug(_(" set parents\n"))
    if collapse and not last:
        repo.dirstate.setparents(repo[p1].node())
        return None

    repo.dirstate.setparents(repo[p1].node(), repo[p2].node())

    if skipped is None:
        skipped = set()

    # Commit, record the old nodeid
    newrev = nullrev
    try:
        if last:
            # we don't translate commit messages
            commitmsg = 'Collapsed revision'
            for rebased in state:
                if rebased not in skipped:
                    commitmsg += '\n* %s' % repo[rebased].description()
            commitmsg = repo.ui.edit(commitmsg, repo.ui.username())
        else:
            commitmsg = repo[rev].description()
        # Commit might fail if unresolved files exist
        extra = {'rebase_source': repo[rev].hex()}
        if extrafn:
            extrafn(repo[rev], extra)
        newrev = repo.commit(text=commitmsg, user=repo[rev].user(),
                             date=repo[rev].date(), extra=extra)
        repo.dirstate.setbranch(repo[newrev].branch())
        return newrev
    except util.Abort:
        # Invalidate the previous setparents
        repo.dirstate.invalidate()
        raise

def rebasenode(repo, rev, target, state, skipped, targetancestors, collapse,
               extrafn):
    'Rebase a single revision'
    repo.ui.debug(_("rebasing %d:%s\n") % (rev, repo[rev]))

    p1, p2 = defineparents(repo, rev, target, state, targetancestors)

    repo.ui.debug(_(" future parents are %d and %d\n") % (repo[p1].rev(),
                                                            repo[p2].rev()))

    # Merge phase
    if len(repo.parents()) != 2:
        # Update to target and merge it with local
        if repo['.'].rev() != repo[p1].rev():
            repo.ui.debug(_(" update to %d:%s\n") % (repo[p1].rev(), repo[p1]))
            merge.update(repo, p1, False, True, False)
        else:
            repo.ui.debug(_(" already in target\n"))
        repo.dirstate.write()
        repo.ui.debug(_(" merge against %d:%s\n") % (repo[rev].rev(), repo[rev]))
        first = repo[rev].rev() == repo[min(state)].rev()
        stats = rebasemerge(repo, rev, first)

        if stats[3] > 0:
            raise util.Abort(_('fix unresolved conflicts with hg resolve then '
                                                'run hg rebase --continue'))
    else: # we have an interrupted rebase
        repo.ui.debug(_('resuming interrupted rebase\n'))

    # Keep track of renamed files in the revision that is going to be rebased
    # Here we simulate the copies and renames in the source changeset
    cop, diver = copies.copies(repo, repo[rev], repo[target], repo[p2], True)
    m1 = repo[rev].manifest()
    m2 = repo[target].manifest()
    for k, v in cop.iteritems():
        if k in m1:
            if v in m1 or v in m2:
                repo.dirstate.copy(v, k)
                if v in m2 and v not in m1:
                    repo.dirstate.remove(v)

    newrev = concludenode(repo, rev, p1, p2, state, collapse,
                          extrafn=extrafn)

    # Update the state
    if newrev is not None:
        state[rev] = repo[newrev].rev()
    else:
        if not collapse:
            repo.ui.note(_('no changes, revision %d skipped\n') % rev)
            repo.ui.debug(_('next revision set to %s\n') % p1)
            skipped.add(rev)
        state[rev] = p1

def defineparents(repo, rev, target, state, targetancestors):
    'Return the new parent relationship of the revision that will be rebased'
    parents = repo[rev].parents()
    p1 = p2 = nullrev

    P1n = parents[0].rev()
    if P1n in targetancestors:
        p1 = target
    elif P1n in state:
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
    for p in repo.mq.applied:
        if repo[p.rev].rev() in state:
            repo.ui.debug(_('revision %d is an mq patch (%s), finalize it.\n') %
                                        (repo[p.rev].rev(), p.name))
            mqrebase[repo[p.rev].rev()] = (p.name, isagitpatch(repo, p.name))

    if mqrebase:
        repo.mq.finish(repo, mqrebase.keys())

        # We must start import from the newest revision
        for rev in sorted(mqrebase, reverse=True):
            if rev not in skipped:
                repo.ui.debug(_('import mq patch %d (%s)\n')
                              % (state[rev], mqrebase[rev][0]))
                repo.mq.qimport(repo, (), patchname=mqrebase[rev][0],
                            git=mqrebase[rev][1],rev=[str(state[rev])])
        repo.mq.save_dirty()

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
        newrev = repo[v].hex()
        f.write("%s:%s\n" % (oldrev, newrev))
    f.close()
    repo.ui.debug(_('rebase status stored\n'))

def clearstatus(repo):
    'Remove the status files'
    if os.path.exists(repo.join("rebasestate")):
        util.unlink(repo.join("rebasestate"))

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
                state[repo[oldrev].rev()] = repo[newrev].rev()
        repo.ui.debug(_('rebase status resumed\n'))
        return originalwd, target, state, collapse, keep, keepbranches, external
    except IOError, err:
        if err.errno != errno.ENOENT:
            raise
        raise util.Abort(_('no rebase in progress'))

def abort(repo, originalwd, target, state):
    'Restore the repository to its original state'
    if set(repo.changelog.descendants(target)) - set(state.values()):
        repo.ui.warn(_("warning: new changesets detected on target branch, "
                                                    "not stripping\n"))
    else:
        # Strip from the first rebased revision
        merge.update(repo, repo[originalwd].rev(), False, True, False)
        rebased = filter(lambda x: x > -1, state.values())
        if rebased:
            strippoint = min(rebased)
            repair.strip(repo.ui, repo, repo[strippoint].node(), "strip")
        clearstatus(repo)
        repo.ui.status(_('rebase aborted\n'))

def buildstate(repo, dest, src, base, collapse):
    'Define which revisions are going to be rebased and where'
    targetancestors = set()

    if not dest:
        # Destination defaults to the latest revision in the current branch
        branch = repo[None].branch()
        dest = repo[branch].rev()
    else:
        if 'qtip' in repo.tags() and (repo[dest].hex() in
                                [s.rev for s in repo.mq.applied]):
            raise util.Abort(_('cannot rebase onto an applied mq patch'))
        dest = repo[dest].rev()

    if src:
        commonbase = repo[src].ancestor(repo[dest])
        if commonbase == repo[src]:
            raise util.Abort(_('cannot rebase an ancestor'))
        if commonbase == repo[dest]:
            raise util.Abort(_('cannot rebase a descendant'))
        source = repo[src].rev()
    else:
        if base:
            cwd = repo[base].rev()
        else:
            cwd = repo['.'].rev()

        if cwd == dest:
            repo.ui.debug(_('already working on current\n'))
            return None

        targetancestors = set(repo.changelog.ancestors(dest))
        if cwd in targetancestors:
            repo.ui.debug(_('already working on the current branch\n'))
            return None

        cwdancestors = set(repo.changelog.ancestors(cwd))
        cwdancestors.add(cwd)
        rebasingbranch = cwdancestors - targetancestors
        source = min(rebasingbranch)

    repo.ui.debug(_('rebase onto %d starting from %d\n') % (dest, source))
    state = dict.fromkeys(repo.changelog.descendants(source), nullrev)
    external = nullrev
    if collapse:
        if not targetancestors:
            targetancestors = set(repo.changelog.ancestors(dest))
        for rev in state:
            # Check externals and fail if there are more than one
            for p in repo[rev].parents():
                if (p.rev() not in state and p.rev() != source
                            and p.rev() not in targetancestors):
                    if external != nullrev:
                        raise util.Abort(_('unable to collapse, there is more '
                                'than one external parent'))
                    external = p.rev()

    state[source] = nullrev
    return repo['.'].rev(), repo[dest].rev(), state, external

def pullrebase(orig, ui, repo, *args, **opts):
    'Call rebase after pull if the latter has been invoked with --rebase'
    if opts.get('rebase'):
        if opts.get('update'):
            del opts['update']
            ui.debug(_('--update and --rebase are not compatible, ignoring '
                                        'the update flag\n'))

        cmdutil.bail_if_changed(repo)
        revsprepull = len(repo)
        orig(ui, repo, *args, **opts)
        revspostpull = len(repo)
        if revspostpull > revsprepull:
            rebase(ui, repo, **opts)
            branch = repo[None].branch()
            dest = repo[branch].rev()
            if dest != repo['.'].rev():
                # there was nothing to rebase we force an update
                merge.update(repo, dest, False, False, False)
    else:
        orig(ui, repo, *args, **opts)

def uisetup(ui):
    'Replace pull with a decorator to provide --rebase option'
    entry = extensions.wrapcommand(commands.table, 'pull', pullrebase)
    entry[1].append(('', 'rebase', None,
                     _("rebase working directory to branch head"))
)

cmdtable = {
"rebase":
        (rebase,
        [
        ('s', 'source', '', _('rebase from a given revision')),
        ('b', 'base', '', _('rebase from the base of a given revision')),
        ('d', 'dest', '', _('rebase onto a given revision')),
        ('', 'collapse', False, _('collapse the rebased revisions')),
        ('', 'keep', False, _('keep original revisions')),
        ('', 'keepbranches', False, _('keep original branches')),
        ('c', 'continue', False, _('continue an interrupted rebase')),
        ('a', 'abort', False, _('abort an interrupted rebase')),] +
         templateopts,
        _('hg rebase [-s REV | -b REV] [-d REV] [--collapse] [--keep] '
                            '[--keepbranches] | [-c] | [-a]')),
}
