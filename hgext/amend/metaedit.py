# metaedit.py - edit changeset metadata
#
# Copyright 2011 Peter Arrenbrecht <peter.arrenbrecht@gmail.com>
#                Logilab SA        <contact@logilab.fr>
#                Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Patrick Mezard <patrick@mezard.eu>
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import cmdutil, commands, error, hg, phases, registrar, scmutil
from mercurial.i18n import _

from . import common, fold


cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "^metaedit",
    [
        ("r", "rev", [], _("revision to edit")),
        ("", "fold", False, _("fold specified revisions into one")),
    ]
    + commands.commitopts
    + commands.commitopts2
    + cmdutil.formatteropts,
    _("[OPTION]... [-r] [REV]"),
    cmdtemplate=True,
)
def metaedit(ui, repo, templ, *revs, **opts):
    """edit commit information

    Edits the commit information for the specified revisions. By default, edits
    commit information for the working directory parent.

    With --fold, also folds multiple revisions into one if necessary. In this
    case, the given revisions must form a linear unbroken chain.

    .. container:: verbose

     Some examples:

     - Edit the commit message for the working directory parent::

         hg metaedit

     - Change the username for the working directory parent::

         hg metaedit --user 'New User <new-email@example.com>'

     - Combine all draft revisions that are ancestors of foo but not of @ into
       one::

         hg metaedit --fold 'draft() and only(foo,@)'

       See :hg:`help phases` for more about draft revisions, and
       :hg:`help revsets` for more about the `draft()` and `only()` keywords.
    """
    revs = list(revs)
    revs.extend(opts["rev"])
    if not revs:
        if opts["fold"]:
            raise error.Abort(_("revisions must be specified with --fold"))
        revs = ["."]

    with repo.wlock(), repo.lock():
        revs = scmutil.revrange(repo, revs)

        if opts["fold"]:
            root, head = fold._foldcheck(repo, revs)
        else:
            if repo.revs("%ld and public()", revs):
                raise error.Abort(
                    _("cannot edit commit information for public " "revisions")
                )
            root = head = repo[revs.first()]

        wctx = repo[None]
        p1 = wctx.p1()
        tr = repo.transaction("metaedit")
        newp1 = None
        try:
            commitopts = opts.copy()
            allctx = [repo[r] for r in revs]

            if commitopts.get("message") or commitopts.get("logfile"):
                commitopts["edit"] = False
            else:
                if opts["fold"]:
                    msgs = [_("HG: This is a fold of %d changesets.") % len(allctx)]
                    msgs += [
                        _("HG: Commit message of changeset %s.\n\n%s\n")
                        % (c.rev(), c.description())
                        for c in allctx
                    ]
                else:
                    msgs = [head.description()]
                commitopts["message"] = "\n".join(msgs)
                commitopts["edit"] = True

            if root == head:
                # fast path: use metarewrite
                replacemap = {}
                # adding commitopts to the revisions to metaedit
                allctxopt = [{"ctx": ctx, "commitopts": commitopts} for ctx in allctx]
                # all descendats that can be safely rewritten
                newunstable = common.newunstable(repo, revs)
                newunstableopt = [
                    {"ctx": ctx} for ctx in [repo[r] for r in newunstable]
                ]
                # we need to edit descendants with the given revisions to not to
                # corrupt the stacks
                if _histediting(repo):
                    ui.note(
                        _(
                            "during histedit, the descendants of "
                            "the edited commit weren't auto-rebased\n"
                        )
                    )
                else:
                    allctxopt += newunstableopt
                # we need topological order for all
                allctxopt = sorted(allctxopt, key=lambda copt: copt["ctx"].rev())

                def _rewritesingle(c, _commitopts):
                    if _commitopts.get("edit", False):
                        _commitopts["message"] = (
                            "HG: Commit message of changeset %s\n%s"
                            % (str(c), c.description())
                        )
                    bases = [
                        replacemap.get(c.p1().node(), c.p1().node()),
                        replacemap.get(c.p2().node(), c.p2().node()),
                    ]

                    newid, created = common.metarewrite(
                        repo, c, bases, commitopts=_commitopts
                    )
                    if created:
                        replacemap[c.node()] = newid

                for copt in allctxopt:
                    _rewritesingle(
                        copt["ctx"],
                        copt.get(
                            "commitopts", {"date": commitopts.get("date") or None}
                        ),
                    )

                if p1.node() in replacemap:
                    repo.setparents(replacemap[p1.node()])
                if len(replacemap) > 0:
                    mapping = dict(
                        map(
                            lambda oldnew: (oldnew[0], [oldnew[1]]),
                            replacemap.iteritems(),
                        )
                    )
                    templ.setprop("nodereplacements", mapping)
                    scmutil.cleanupnodes(repo, mapping, "metaedit")
                    # TODO: set poroper phase boundaries (affects secret
                    # phase only)
                else:
                    ui.status(_("nothing changed\n"))
                    return 1
            else:
                # slow path: create a new commit
                targetphase = max(c.phase() for c in allctx)

                # TODO: if the author and message are the same, don't create a
                # new hash. Right now we create a new hash because the date can
                # be different.
                newid, created = common.rewrite(
                    repo,
                    root,
                    allctx,
                    head,
                    [root.p1().node(), root.p2().node()],
                    commitopts=commitopts,
                )
                if created:
                    if p1.rev() in revs:
                        newp1 = newid
                    phases.retractboundary(repo, tr, targetphase, [newid])
                    mapping = dict([(repo[rev].node(), [newid]) for rev in revs])
                    templ.setprop("nodereplacements", mapping)
                    scmutil.cleanupnodes(repo, mapping, "metaedit")
                else:
                    ui.status(_("nothing changed\n"))
                    return 1
            tr.close()
        finally:
            tr.release()

        if opts["fold"]:
            ui.status(_("%i changesets folded\n") % len(revs))
        if newp1 is not None:
            hg.update(repo, newp1)


def _histediting(repo):
    return repo.localvfs.exists("histedit-state")
