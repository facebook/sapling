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

"""uncommit some or all of a local changeset

This command undoes the effect of a local commit, returning the affected
files to their uncommitted state. This means that files modified or
deleted in the changeset will be left unchanged, and so will remain modified in
the working directory.
"""

from mercurial import (
    cmdutil,
    phases,
    obsolete,
    commands,
    error,
    scmutil,
    copies,
    context,
    node
)
from mercurial.i18n import _
from contextlib import nested

cmdtable = {}
command = cmdutil.command(cmdtable)

testedwith = 'internal'

def _updatebookmarks(repo, oldid, newid, tr):
    oldbookmarks = repo.nodebookmarks(oldid)
    if oldbookmarks:
        for b in oldbookmarks:
            repo._bookmarks[b] = newid
        repo._bookmarks.recordchange(tr)

def _commitfiltered(repo, ctx, match):
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

    files = (initialfiles - exclude)
    if not files:
        return ctx.parents()[0].node()

    # Filter copies
    copied = copies.pathcopies(base, ctx)
    copied = dict((src, dst) for src, dst in copied.iteritems()
                  if dst in files)
    def filectxfn(repo, memctx, path, contentctx=ctx, redirect=()):
        if path not in contentctx:
            return None
        fctx = contentctx[path]
        mctx = context.memfilectx(repo, fctx.path(), fctx.data(),
                                  fctx.islink(),
                                  fctx.isexec(),
                                  copied=copied.get(path))
        return mctx

    new = context.memctx(repo,
                         parents=[base.node(), node.nullid],
                         text=ctx.description(),
                         files=files,
                         filectxfn=filectxfn,
                         user=ctx.user(),
                         date=ctx.date(),
                         extra=ctx.extra())
    newid = repo.commitctx(new)
    return newid

def _uncommitdirstate(repo, oldctx, match):
    """Fix the dirstate after switching the working directory from
    oldctx to a copy of oldctx not containing changed files matched by
    match.
    """
    ctx = repo['.']
    ds = repo.dirstate
    copies = dict(ds.copies())
    m, a, r = repo.status(oldctx.p1(), oldctx, match=match)[:3]
    for f in m:
        if ds[f] == 'r':
            # modified + removed -> removed
            continue
        ds.normallookup(f)

    for f in a:
        if ds[f] == 'r':
            # added + removed -> unknown
            ds.drop(f)
        elif ds[f] != 'a':
            ds.add(f)

    for f in r:
        if ds[f] == 'a':
            # removed + added -> normal
            ds.normallookup(f)
        elif ds[f] != 'r':
            ds.remove(f)

    # Merge old parent and old working dir copies
    oldcopies = {}
    for f in (m + a):
        src = oldctx[f].renamed()
        if src:
            oldcopies[f] = src[0]
    oldcopies.update(copies)
    copies = dict((dst, oldcopies.get(src, src))
                  for dst, src in oldcopies.iteritems())
    # Adjust the dirstate copies
    for dst, src in copies.iteritems():
        if (src not in ctx or dst in ctx or ds[dst] != 'a'):
            src = None
        ds.copy(src, dst)

@command('uncommit',
    commands.walkopts,
    _('[OPTION]... [FILE]...'))
def uncommit(ui, repo, *pats, **opts):
    """uncommit some or all of a local changeset

    This command undoes the effect of a local commit, returning the affected
    files to their uncommitted state. This means that files modified or
    deleted in the changeset will be left unchanged, and so will remain
    modified in the working directory.
    """

    with nested(repo.wlock(), repo.lock()):
        wctx = repo[None]
        wm = wctx.manifest()

        if len(wctx.parents()) <= 0 or not wctx.parents()[0]:
            raise error.Abort(_("cannot uncommit null changeset"))
        if len(wctx.parents()) > 1:
            raise error.Abort(_("cannot uncommit while merging"))
        old = repo['.']
        oldphase = old.phase()
        if oldphase == phases.public:
            raise error.Abort(_("cannot rewrite immutable changeset"))
        if len(old.parents()) > 1:
            raise error.Abort(_("cannot uncommit merge changeset"))

        with repo.transaction('uncommit') as tr:
            match = scmutil.match(old, pats, opts)
            newid = _commitfiltered(repo, old, match)
            if newid is None:
                raise error.Abort(_('nothing to uncommit'))

            # Move local changes on filtered changeset
            obsolete.createmarkers(repo, [(old, (repo[newid],))])
            phases.retractboundary(repo, tr, oldphase, [newid])

            repo.dirstate.beginparentchange()
            repo.dirstate.setparents(newid, node.nullid)
            _uncommitdirstate(repo, old, match)
            repo.dirstate.endparentchange()

            _updatebookmarks(repo, old.node(), newid, tr)
