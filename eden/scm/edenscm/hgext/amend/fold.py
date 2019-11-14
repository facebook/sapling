# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# fold.py - fold multiple revisions to a single one
#
# Copyright 2011 Peter Arrenbrecht <peter.arrenbrecht@gmail.com>
#                Logilab SA        <contact@logilab.fr>
#                Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Patrick Mezard <patrick@mezard.eu>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import commands, error, hg, node, phases, registrar, scmutil
from edenscm.mercurial.i18n import _

from . import common


cmdtable = {}
command = registrar.command(cmdtable)
hex = node.hex


@command(
    "fold|squash",
    [
        ("r", "rev", [], _("revision to fold")),
        ("", "exact", None, _("only fold specified revisions")),
        (
            "",
            "from",
            None,
            _("fold linearly from current revision to specified revision"),
        ),
        ("", "no-rebase", False, _("don't rebase descendants after split")),
    ]
    + (commands.commitopts + commands.commitopts2 + commands.formatteropts),
    _("hg fold [OPTION]... (--from [-r] REV | --exact [-r] REV...)"),
)
def fold(ui, repo, *revs, **opts):
    """combine multiple commits into a single commit

    With --from, folds all the revisions linearly between the current revision
    and the specified revision.

    With --exact, folds only the specified revisions while ignoring the revision
    currently checked out. The given revisions must form a linear unbroken
    chain.

    .. container:: verbose

     Some examples:

     - Fold from the current revision to its parent::

         hg fold --from .^

     - Fold all draft revisions into the current revision::

         hg fold --from 'draft()'

       See :hg:`help phases` for more about draft revisions and
       :hg:`help revsets` for more about the `draft()` keyword

     - Fold revisions between 3 and 6 into the current revision::

         hg fold --from 3::6

     - Fold revisions 3 and 4:

        hg fold "3 + 4" --exact

     - Only fold revisions linearly between foo and @::

         hg fold foo::@ --exact
    """
    revs = list(revs)
    revs.extend(opts["rev"])
    if not revs:
        raise error.Abort(_("no revisions specified"))

    revs = scmutil.revrange(repo, revs)

    if opts.get("no_rebase"):
        torebase = ()
    else:
        torebase = repo.revs("descendants(%ld) - (%ld)", revs, revs)

    if opts["from"] and opts["exact"]:
        raise error.Abort(_("cannot use both --from and --exact"))
    elif opts["from"]:
        # Try to extend given revision starting from the working directory
        extrevs = repo.revs("(%ld::.) or (.::%ld)", revs, revs)
        discardedrevs = [r for r in revs if r not in extrevs]
        if discardedrevs:
            msg = _("cannot fold non-linear revisions")
            hint = _("given revisions are unrelated to parent of working" " directory")
            raise error.Abort(msg, hint=hint)
        revs = extrevs
    elif opts["exact"]:
        # Nothing to do; "revs" is already set correctly
        pass
    else:
        raise error.Abort(_("must specify either --from or --exact"))

    if not revs:
        raise error.Abort(
            _("specified revisions evaluate to an empty set"),
            hint=_("use different revision arguments"),
        )
    elif len(revs) == 1:
        ui.write_err(_("single revision specified, nothing to fold\n"))
        return 1

    with repo.wlock(), repo.lock(), ui.formatter("fold", opts) as fm:
        fm.startitem()
        root, head = _foldcheck(repo, revs)

        with repo.transaction("fold") as tr:
            commitopts = opts.copy()
            allctx = [repo[r] for r in revs]
            targetphase = max(c.phase() for c in allctx)

            if commitopts.get("message") or commitopts.get("logfile"):
                commitopts["edit"] = False
            else:
                msgs = ["HG: This is a fold of %d changesets." % len(allctx)]
                msgs += [
                    "HG: Commit message of changeset %s.\n\n%s\n"
                    % (c.rev(), c.description())
                    for c in allctx
                ]
                commitopts["message"] = "\n".join(msgs)
                commitopts["edit"] = True

            newid, unusedvariable = common.rewrite(
                repo,
                root,
                allctx,
                head,
                [root.p1().node(), root.p2().node()],
                commitopts=commitopts,
                mutop="fold",
            )
            phases.retractboundary(repo, tr, targetphase, [newid])

            replacements = {ctx.node(): (newid,) for ctx in allctx}
            nodechanges = {
                fm.hexfunc(ctx.node()): [fm.hexfunc(newid)] for ctx in allctx
            }
            fm.data(nodechanges=fm.formatdict(nodechanges))
            scmutil.cleanupnodes(repo, replacements, "fold")
            fm.condwrite(not ui.quiet, "count", "%i changesets folded\n", len(revs))
            if repo["."].rev() in revs:
                hg.update(repo, newid)

            if torebase:
                common.restackonce(ui, repo, repo[newid].rev())


def _foldcheck(repo, revs):
    roots = repo.revs("roots(%ld)", revs)
    if len(roots) > 1:
        raise error.Abort(
            _("cannot fold non-linear revisions " "(multiple roots given)")
        )
    root = repo[roots.first()]
    if root.phase() <= phases.public:
        raise error.Abort(_("cannot fold public revisions"))
    heads = repo.revs("heads(%ld)", revs)
    if len(heads) > 1:
        raise error.Abort(
            _("cannot fold non-linear revisions " "(multiple heads given)")
        )
    head = repo[heads.first()]
    return root, head
