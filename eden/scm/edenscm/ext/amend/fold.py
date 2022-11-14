# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
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

from edenscm import commands, error, hg, identity, node, phases, registrar, scmutil
from edenscm.i18n import _

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
        ("", "no-rebase", False, _("don't rebase descendants after fold")),
        ("M", "reuse-message", "", _("reuse commit message from REV"), _("REV")),
    ]
    + (commands.commitopts + commands.commitopts2 + commands.formatteropts),
    _("@prog@ fold [OPTION]... (--from [-r] REV | --exact [-r] REV...)"),
)
def fold(ui, repo, *revs, **opts):
    """combine multiple commits into a single commit

    With ``--from``, fold all of the commit linearly between the current
    commit and the specified commit.

    With ``--exact``, fold only the specified commits while ignoring the
    current commit. The given commits must form a linear, continuous
    chain.

    .. container:: verbose

     Some examples:

     - Fold from the current commit to its parent::

         @prog@ fold --from .^

     - Fold all draft commits into the current commit::

         @prog@ fold --from 'draft()'

       See :prog:`help phases` for more about draft commits and
       :prog:`help revsets` for more about the `draft()` keyword.

     - Fold commits between e254371c1 and be57079e4 into the current commit::

         @prog@ fold --from e254371c1::be57079e4

     - Fold commits e254371c1 and be57079e4:

        @prog@ fold "e254371c1 + be57079e4" --exact

     - Only fold commits linearly between foo and .::

         @prog@ fold foo::. --exact
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

            if (
                commitopts.get("message")
                or commitopts.get("logfile")
                or commitopts.get("reuse_message")
            ):
                commitopts["edit"] = False
            else:
                msgs = [
                    _("%s: This is a fold of %d changesets.")
                    % (identity.tmplprefix(), len(allctx))
                ]
                msgs += [
                    "%s: Commit message of %s.\n\n%s\n"
                    % (identity.tmplprefix(), node.short(c.node()), c.description())
                    for c in allctx
                ]
                commitopts["message"] = "\n".join(msgs)
                commitopts["edit"] = True

            newid, unusedvariable = common.rewrite(
                repo,
                root,
                allctx,
                head,
                [root.p1(), root.p2()],
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
