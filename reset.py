# Copyright 2014 Facebook Inc.
#
"""FBONLY: reset the active bookmark and working copy to a desired revision"""

from mercurial.i18n import _
from mercurial.node import short, hex
from mercurial import extensions, merge, dicthelpers, scmutil, hg
from mercurial import cmdutil, obsolete, repair, util, bundlerepo, error
from mercurial import exchange
import struct, os, glob

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

@command("reset", [
        ('C', 'clean', None, _('wipe the working copy clean when reseting')),
        ('k', 'keep', None, _('keeps the old commits the bookmark pointed to')),
    ], _('hg reset [REV]'))
def reset(ui, repo, *args, **opts):
    """moves the active bookmark and working copy parent to the desired rev

    The reset command is for moving your active bookmark and working copy to a
    different location. This is useful for undoing commits, amends, etc.

    By default, the working copy content is not touched, so you will have
    pending changes after the reset. If --clean/-C is specified, the working
    copy contents will be overwritten to match the destination revision, and you
    will not have any pending changes.

    After your bookmark and working copy have been moved, the command will
    delete any commits that belonged only to that bookmark. Use --keep/-k to
    avoid deleting any commits.
    """
    repo = repo.unfiltered()
    rev = args[0] if args else '.'
    try:
        revs = repo.revs(rev)
        if len(revs) > 1:
            raise util.Abort(_('exactly one revision must be specified'))
        rev = revs.first()
    except error.RepoLookupError:
        # `rev` can be anything, a hash, a partial hash, a revset, etc. If it's
        # a revset, repo.revs() will convert it.  If it's a hash that only
        # exists in a bundle, repo.revs() will throw an exception, but it's
        # still valid input since we'll recover the commit shortly.
        pass
    oldctx = repo['.']

    wlock = None
    try:
        wlock = repo.wlock()
        # Ensure we have an active bookmark
        bookmark = repo._bookmarkcurrent
        if not bookmark:
            ui.warn(_('reseting without an active bookmark\n'))

        ctx = _revive(repo, rev)
        if ctx.node() == oldctx.node() and not opts.get('clean'):
            ui.status(_('reseting without any arguments does nothing\n'))
            return

        _moveto(repo, bookmark, ctx, clean=opts.get('clean'))
        if not opts.get('keep'):
            _deleteunreachable(repo, oldctx)
    finally:
        wlock.release()

def _revive(repo, rev):
    """Brings the given rev back into the repository. Finding it in backup
    bundles if necessary.
    """
    if rev not in repo:
        other, rev = _findbundle(repo, rev)
        if not other:
            raise util.Abort("could not find '%s' in the repo or the backup"
                             " bundles" % rev)
        exchange.pull(repo, other, heads=[rev])

        if rev not in repo:
            raise util.Abort("unable to get rev %s from repo" % rev)

    return repo[rev]

def _findbundle(repo, rev):
    """Returns the backup bundle that contains the given rev. If found, it
    returns the bundle peer and the full rev hash. If not found, it return None
    and the given rev value.
    """
    ui = repo.ui
    backuppath = repo.join("strip-backup")
    backups = filter(os.path.isfile, glob.glob(backuppath + "/*.hg"))
    backups.sort(key=lambda x: os.path.getmtime(x), reverse=True)
    for backup in backups:
        # Much of this is copied from the hg incoming logic
        source = os.path.relpath(backup, os.getcwd())
        source = ui.expandpath(source)
        source, branches = hg.parseurl(source)
        other = hg.peer(repo, {}, source)

        quiet = ui.quiet
        try:
            ui.quiet = True
            localother, chlist, cleanupfn = bundlerepo.getremotechanges(ui, repo, other,
                                        None, None, None)
            for node in chlist:
                if hex(node).startswith(rev):
                    return other, node
        except error.LookupError:
            continue
        finally:
            ui.quiet = quiet

    return None, rev

def _moveto(repo, bookmark, ctx, clean=False):
    """Moves the given bookmark and the working copy to the given revision.
    By default it does not overwrite the working copy contents unless clean is
    True.

    Assumes the wlock is already taken.
    """
    # Move working copy over
    if clean:
        merge.update(repo, ctx.node(),
                     False, # not a branchmerge
                     True, # force overwriting files
                     None) # not a partial update
    else:
        # Mark any files that are different between the two as normal-lookup
        # so they show up correctly in hg status afterwards.
        wctx = repo[None]
        m1 = wctx.manifest()
        m2 = ctx.manifest()
        diff = m1.diff(m2)

        changedfiles = []
        changedfiles.extend(diff.iterkeys())

        dirstate = repo.dirstate
        dirchanges = [f for f in dirstate if dirstate[f] != 'n']
        changedfiles.extend(dirchanges)

        dirstate.beginparentchange()
        dirstate.rebuild(ctx.node(), m2, changedfiles)
        dirstate.endparentchange()

    # Move bookmark over
    if bookmark:
        repo._bookmarks[bookmark] = ctx.node()
        repo._bookmarks.write()

def _deleteunreachable(repo, ctx):
    """Deletes all ancestor and descendant commits of the given revision that
    aren't reachable from another bookmark.
    """
    hiderevs = repo.revs('::%s - ::(bookmark() + .)', ctx.rev())
    if hiderevs:
        repair.strip(repo.ui, repo, [repo.changelog.node(r) for r in hiderevs])
