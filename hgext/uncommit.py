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

from mercurial.i18n import _

from mercurial import (
    cmdutil,
    commands,
    context,
    copies,
    error,
    node,
    obsolete,
    registrar,
    scmutil,
)

cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem('experimental', 'uncommitondirtywdir',
    default=False,
)

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

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

    files = (initialfiles - exclude)
    # return the p1 so that we don't create an obsmarker later
    if not files and not allowempty:
        return ctx.parents()[0].node()

    # Filter copies
    copied = copies.pathcopies(base, ctx)
    copied = dict((dst, src) for dst, src in copied.iteritems()
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
    # phase handling
    commitphase = ctx.phase()
    overrides = {('phases', 'new-commit'): commitphase}
    with repo.ui.configoverride(overrides, 'uncommit'):
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
    s = repo.status(oldctx.p1(), oldctx, match=match)
    for f in s.modified:
        if ds[f] == 'r':
            # modified + removed -> removed
            continue
        ds.normallookup(f)

    for f in s.added:
        if ds[f] == 'r':
            # added + removed -> unknown
            ds.drop(f)
        elif ds[f] != 'a':
            ds.add(f)

    for f in s.removed:
        if ds[f] == 'a':
            # removed + added -> normal
            ds.normallookup(f)
        elif ds[f] != 'r':
            ds.remove(f)

    # Merge old parent and old working dir copies
    oldcopies = {}
    for f in (s.modified + s.added):
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
    [('', 'keep', False, _('allow an empty commit after uncommiting')),
    ] + commands.walkopts,
    _('[OPTION]... [FILE]...'))
def uncommit(ui, repo, *pats, **opts):
    """uncommit part or all of a local changeset

    This command undoes the effect of a local commit, returning the affected
    files to their uncommitted state. This means that files modified or
    deleted in the changeset will be left unchanged, and so will remain
    modified in the working directory.
    """

    with repo.wlock(), repo.lock():
        wctx = repo[None]

        if not pats and not repo.ui.configbool('experimental',
                                                'uncommitondirtywdir'):
            cmdutil.bailifchanged(repo)
        if wctx.parents()[0].node() == node.nullid:
            raise error.Abort(_("cannot uncommit null changeset"))
        if len(wctx.parents()) > 1:
            raise error.Abort(_("cannot uncommit while merging"))
        old = repo['.']
        if not old.mutable():
            raise error.Abort(_('cannot uncommit public changesets'))
        if len(old.parents()) > 1:
            raise error.Abort(_("cannot uncommit merge changeset"))
        allowunstable = obsolete.isenabled(repo, obsolete.allowunstableopt)
        if not allowunstable and old.children():
            raise error.Abort(_('cannot uncommit changeset with children'))

        with repo.transaction('uncommit'):
            match = scmutil.match(old, pats, opts)
            newid = _commitfiltered(repo, old, match, opts.get('keep'))
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

            scmutil.cleanupnodes(repo, mapping, 'uncommit')

            with repo.dirstate.parentchange():
                repo.dirstate.setparents(newid, node.nullid)
                _uncommitdirstate(repo, old, match)
