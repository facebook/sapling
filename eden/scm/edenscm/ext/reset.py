# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""reset the active bookmark and working copy to a desired revision"""

from edenscm import (
    error,
    extensions,
    lock as lockmod,
    merge,
    pycompat,
    registrar,
    scmutil,
    visibility,
)
from edenscm.i18n import _, _n


cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-ext"


@command(
    "reset",
    [
        ("C", "clean", None, _("wipe the working copy clean when resetting")),
        ("k", "keep", None, _("keeps the old changesets the bookmark pointed" " to")),
        ("r", "rev", "", _("revision to reset to")),
    ],
    _("@prog@ reset [REV]"),
)
def reset(ui, repo, *args, **opts):
    """moves the active bookmark and working copy parent to the desired rev

    The reset command is for moving your active bookmark and working copy to a
    different location. This is useful for undoing commits, amends, etc.

    By default, the working copy content is not touched, so you will have
    pending changes after the reset. If --clean/-C is specified, the working
    copy contents will be overwritten to match the destination revision, and you
    will not have any pending changes.

    After your bookmark and working copy have been moved, the command will
    delete any changesets that belonged only to that bookmark. Use --keep/-k to
    avoid deleting any changesets.
    """
    if args and args[0] and opts.get("rev"):
        e = _("do not use both --rev and positional argument for revision")
        raise error.Abort(e)

    rev = opts.get("rev") or (args[0] if args else ".")
    oldctx = repo["."]

    wlock = None
    try:
        wlock = repo.wlock()
        bookmark = repo._activebookmark
        ctx = _revive(repo, rev)
        _moveto(repo, bookmark, ctx, clean=opts.get("clean"))
        if not opts.get("keep"):
            _deleteunreachable(repo, oldctx)
    finally:
        wlock.release()


def _revive(repo, rev):
    """Brings the given rev back into the repository. Finding it in backup
    bundles if necessary.
    """
    unfi = repo
    try:
        ctx = unfi[rev]
    except error.RepoLookupError:
        # It could either be a revset or a stripped commit.
        pass
    else:
        visibility.add(repo, [ctx.node()])

    revs = scmutil.revrange(repo, [rev])
    if len(revs) > 1:
        raise error.Abort(_("exactly one revision must be specified"))
    if len(revs) == 1:
        return repo[revs.first()]


def _moveto(repo, bookmark, ctx, clean=False):
    """Moves the given bookmark and the working copy to the given revision.
    By default it does not overwrite the working copy contents unless clean is
    True.

    Assumes the wlock is already taken.
    """
    # Move working copy over
    if clean:
        merge.update(
            repo,
            ctx.node(),
            False,  # not a branchmerge
            True,  # force overwriting files
            None,
        )  # not a partial update
    else:
        # Mark any files that are different between the two as normal-lookup
        # so they show up correctly in hg status afterwards.
        wctx = repo[None]
        m1 = wctx.manifest()
        m2 = ctx.manifest()
        diff = m1.diff(m2)

        changedfiles = []
        changedfiles.extend(pycompat.iterkeys(diff))

        dirstate = repo.dirstate
        dirchanges = [f for f in dirstate if dirstate[f] != "n"]
        changedfiles.extend(dirchanges)

        if changedfiles or ctx.node() != repo["."].node():
            with dirstate.parentchange():
                dirstate.rebuild(ctx.node(), m2, changedfiles)

    # Move bookmark over
    if bookmark:
        lock = tr = None
        try:
            lock = repo.lock()
            tr = repo.transaction("reset")
            changes = [(bookmark, ctx.node())]
            repo._bookmarks.applychanges(repo, tr, changes)
            tr.close()
        finally:
            lockmod.release(lock, tr)


def _deleteunreachable(repo, ctx):
    """Deletes all ancestor and descendant commits of the given revision that
    aren't reachable from another bookmark.
    """
    keepheads = "bookmark() + ."
    try:
        extensions.find("remotenames")
        keepheads += " + remotenames()"
    except KeyError:
        pass
    hidenodes = list(repo.nodes("(draft() & ::%n) - ::(%r)", ctx.node(), keepheads))
    if hidenodes:
        with repo.lock():
            scmutil.cleanupnodes(repo, hidenodes, "reset")
        repo.ui.status(
            _n("%d changeset hidden\n", "%d changesets hidden\n", len(hidenodes))
            % len(hidenodes)
        )
