# uncommit - undo the actions of a commit
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

from mercurial import (
    cmdutil,
    commands,
    context,
    copies,
    error,
    node,
    obsutil,
    pycompat,
    registrar,
    rewriteutil,
    scmutil,
    treestate,
)
from mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem("experimental", "uncommitondirtywdir", default=True)

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = "ships-with-hg-core"


def _commitfiltered(repo, ctx, match, allowempty):
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
        parents=[base.node(), node.nullid],
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


def _fixdirstate(repo, oldctx, newctx, status):
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


@command(
    "uncommit",
    [("", "keep", False, _("allow an empty commit after uncommiting"))]
    + commands.walkopts,
    _("[OPTION]... [FILE]..."),
)
def uncommit(ui, repo, *pats, **opts):
    """uncommit part or all of the current commit

    This command undoes the effect of running commit, returning the affected
    files to their uncommitted state. This means that files modified or
    deleted in the changeset will be left unchanged, and so will remain
    modified in the working directory.
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
            newid = _commitfiltered(repo, old, match, opts.get("keep"))
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
                repo.dirstate.setparents(newid, node.nullid)
                s = repo.status(old.p1(), old, match=match)
                _fixdirstate(repo, old, repo[newid], s)
