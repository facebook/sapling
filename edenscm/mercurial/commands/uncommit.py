# uncommit help functions
#
# Copyright 2011 Peter Arrenbrecht <peter.arrenbrecht@gmail.com>
#                Logilab SA        <contact@logilab.fr>
#                Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Patrick Mezard <patrick@mezard.eu>
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""uncommit part or all of a local changeset (EXPERIMENTAL)

This command undoes the effect of a local commit, returning the affected
files to their uncommitted state. This means that files modified, added or
removed in the changeset will be left unchanged, and so will remain modified,
added and removed in the working directory.
"""

from __future__ import absolute_import

from .. import (
    cmdutil,
    context,
    copies,
    error,
    pycompat,
    rewriteutil,
    scmutil,
    treestate,
)
from ..i18n import _
from ..node import nullid
from .cmdtable import command


@command(
    "uncommit",
    [("", "keep", False, _("allow an empty commit after uncommiting"))]
    + cmdutil.walkopts,
    _("[OPTION]... [FILE]..."),
)
def uncommit(ui, repo, *pats, **opts):
    """uncommit part or all of the current commit

    Reverse the effects of an :hg:`commit` operation. When run with no
    arguments, hides the current commit and checks out the parent commit,
    but does not revert the state of the working copy. Changes that were
    contained in the uncommitted commit become pending changes in the
    working copy.

    :hg:`uncommit` cannot be run on commits that have children. In other words,
    you cannot uncommit a commit in the middle of a stack. Similarly, by
    default you cannot run :hg:`uncommit` if there are pending changes in the
    working copy.

    You can selectively uncommit files from the current commit by optionally
    specifying a list of files to remove. The specified files are removed from
    the list of changed files in the current commit, but are not modified on
    disk, so they appear as pending changes in the working copy.

    .. note::

       Running :hg:`uncommit` is similar to running :hg:`undo --keep`
       immediately after :hg:`commit`. However, unlike :hg:`undo`, which can
       only undo a commit if it was the last operation you performed,
       :hg:`uncommit` can uncommit any draft commit in the graph that does
       not have children.
    """
    opts = pycompat.byteskwargs(opts)

    with repo.wlock(), repo.lock():

        if not pats and not repo.ui.configbool("experimental", "uncommitondirtywdir"):
            cmdutil.bailifchanged(repo)
        old = repo["."]
        rewriteutil.precheck(repo, [old.rev()], "uncommit")
        if len(old.parents()) > 1:
            raise error.Abort(_("cannot uncommit merge changeset"))

        with repo.transaction("uncommit"):
            match = scmutil.match(old, pats, opts)
            newid = commitfilteredctx(repo, old, match, opts.get("keep"))
            if newid is None:
                ui.status(_("nothing to uncommit\n"))
                return 1

            mapping = {}
            if newid != old.p1().node():
                # Move local changes on filtered changeset
                mapping[old.node()] = (newid,)
            else:
                # Fully removed the old commit
                mapping[old.node()] = ()

            scmutil.cleanupnodes(repo, mapping, "uncommit")

            with repo.dirstate.parentchange():
                repo.dirstate.setparents(newid, nullid)
                s = repo.status(old.p1(), old, match=match)
                fixdirstate(repo, old, repo[newid], s)


def commitfilteredctx(repo, ctx, match, allowempty):
    """Recommit ctx with changed files not in match. Return the new
    node identifier, or None if nothing changed.
    """
    base = ctx.p1()
    # ctx
    initialfiles = set(ctx.files())
    exclude = set(f for f in initialfiles if match(f))

    # No files matched commit, so nothing excluded
    if not exclude:
        return None

    files = initialfiles - exclude
    # return the p1 so that we don't create an obsmarker later
    if not files and not allowempty:
        return ctx.parents()[0].node()

    # Filter copies
    copied = copies.pathcopies(base, ctx)
    copied = dict((dst, src) for dst, src in copied.iteritems() if dst in files)

    def filectxfn(repo, memctx, path, contentctx=ctx, redirect=()):
        if path not in contentctx:
            return None
        fctx = contentctx[path]
        mctx = context.memfilectx(
            repo,
            memctx,
            fctx.path(),
            fctx.data(),
            fctx.islink(),
            fctx.isexec(),
            copied=copied.get(path),
        )
        return mctx

    new = context.memctx(
        repo,
        parents=[base.node(), nullid],
        text=ctx.description(),
        files=files,
        filectxfn=filectxfn,
        user=ctx.user(),
        date=ctx.date(),
        extra=ctx.extra(),
    )
    # phase handling
    commitphase = ctx.phase()
    overrides = {("phases", "new-commit"): commitphase}
    with repo.ui.configoverride(overrides, "uncommit"):
        newid = repo.commitctx(new)
    return newid


def fixdirstate(repo, oldctx, newctx, status):
    """ fix the dirstate after switching the working directory from oldctx to
    newctx which can be result of either unamend or uncommit.
    """
    ds = repo.dirstate
    copies = dict(ds.copies())
    s = status
    for f in s.modified:
        if ds[f] == "r":
            # modified + removed -> removed
            continue
        ds.normallookup(f)

    for f in s.added:
        if ds[f] == "r":
            # added + removed -> unknown
            ds.untrack(f)
        elif ds[f] != "a":
            ds.add(f)

    p1 = list(repo.set("p1()"))
    p2 = list(repo.set("p2()"))
    for f in s.removed:
        if ds[f] == "a":
            # removed + added -> normal
            ds.normallookup(f)
        elif ds[f] != "r":
            # For treestate, ds.remove is a no-op if f is not tracked.
            # In that case, we need to manually "correct" the state by
            # using low-level treestate API.
            if ds._istreestate and f not in ds:
                state = 0
                if any(f in ctx for ctx in p1):
                    state |= treestate.treestate.EXIST_P1
                if any(f in ctx for ctx in p2):
                    state |= treestate.treestate.EXIST_P2
                mode = 0
                mtime = -1
                size = 0
                ds._map._tree.insert(f, state, mode, size, mtime, None)
            else:
                ds.remove(f)

    # Merge old parent and old working dir copies
    oldcopies = {}
    for f in s.modified + s.added:
        src = oldctx[f].renamed()
        if src:
            oldcopies[f] = src[0]
    oldcopies.update(copies)
    copies = dict((dst, oldcopies.get(src, src)) for dst, src in oldcopies.iteritems())
    # Adjust the dirstate copies
    for dst, src in copies.iteritems():
        if src not in newctx or dst in newctx or ds[dst] != "a":
            src = None
        ds.copy(src, dst)
