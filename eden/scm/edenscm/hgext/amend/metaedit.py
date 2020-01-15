# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# metaedit.py - edit changeset metadata
#
# Copyright 2011 Peter Arrenbrecht <peter.arrenbrecht@gmail.com>
#                Logilab SA        <contact@logilab.fr>
#                Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Patrick Mezard <patrick@mezard.eu>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import (
    cmdutil,
    commands,
    error,
    hg,
    mutation,
    node as nodemod,
    phases,
    registrar,
    scmutil,
)
from edenscm.mercurial.i18n import _

from . import common, fold


cmdtable = {}
command = registrar.command(cmdtable)


def editmessages(repo, revs):
    """Invoke editor to edit messages in batch. Return {node: new message}"""
    nodebanners = []
    editortext = ""

    for rev in revs:
        ctx = repo[rev]
        message = ctx.description()
        short = nodemod.short(ctx.node())
        bannerstart = cmdutil.hgprefix(_("Begin of commit %s") % short)
        bannerend = cmdutil.hgprefix(_("End of commit %s") % short)
        nodebanners.append((ctx.node(), bannerstart, bannerend))
        if editortext:
            editortext += cmdutil.hgprefix("-" * 77) + "\n"
        else:
            editortext += (
                cmdutil.hgprefix(
                    _(
                        "Editing %s commits in batch. Do not change lines starting with 'HG:'."
                    )
                    % len(revs)
                )
                + "\n"
            )

        editortext += "%s\n%s\n%s\n" % (bannerstart, message, bannerend)

    result = {}
    ui = repo.ui
    newtext = ui.edit(editortext, ui.username(), action="metaedit", repopath=repo.path)
    for node, bannerstart, bannerend in nodebanners:
        if bannerstart in newtext and bannerend in newtext:
            newmessage = newtext.split(bannerstart, 1)[1].split(bannerend, 1)[0]
            result[node] = newmessage

    return result


@command(
    "metaedit|met|meta|metae|metaed|metaedi",
    [
        ("r", "rev", [], _("revision to edit")),
        ("", "fold", False, _("fold specified revisions into one")),
        (
            "",
            "batch",
            False,
            _("edit messages of multiple commits in one editor invocation"),
        ),
        ("M", "reuse-message", "", _("reuse commit message from another commit")),
    ]
    + commands.commitopts
    + commands.commitopts2
    + cmdutil.formatteropts,
    _("[OPTION]... [-r] [REV]"),
    cmdtemplate=True,
)
def metaedit(ui, repo, templ, *revs, **opts):
    """edit commit message and other metadata

    Edit commit message for the current commit. By default, opens your default
    editor so that you can edit the commit message interactively. Specify -m
    to specify the commit message on the command line.

    To edit the message for a different commit, specify -r. To edit the
    messages of multiple commits, specify --batch.

    You can edit other pieces of commit metadata, namely the user or date,
    by specifying -u or -d, respectively. The expected format for user is
    'Full Name <user@example.com>'.

    .. note::

        You can specify --fold to fold multiple revisions into one when the
        given revisions form a linear unbroken chain. However, :hg:`fold` is
        the preferred command for this purpose. See :hg:`help fold` for more
        information.

    .. container:: verbose

     Some examples:

     - Edit the commit message for the current commit::

         hg metaedit

     - Change the username for the current commit::

         hg metaedit --user 'New User <new-email@example.com>'

    """
    revs = list(revs)
    revs.extend(opts["rev"])
    if not revs:
        if opts["fold"]:
            raise error.Abort(_("revisions must be specified with --fold"))
        revs = ["."]

    with repo.wlock(), repo.lock():
        revs = scmutil.revrange(repo, revs)
        msgmap = {}  # {node: message}, predefined messages, currently used by --batch

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

            if any(
                commitopts.get(name) for name in ["message", "logfile", "reuse_message"]
            ):
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
                    if opts["batch"] and len(revs) > 1:
                        msgmap = editmessages(repo, revs)
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
                if mutation.enabled(repo):
                    allctxopt = mutation.toposort(
                        repo, allctxopt, nodefn=lambda copt: copt["ctx"].node()
                    )
                else:
                    allctxopt = sorted(allctxopt, key=lambda copt: copt["ctx"].rev())

                def _rewritesingle(c, _commitopts):
                    # Predefined message overrides other message editing choices.
                    msg = msgmap.get(c.node())
                    if msg is not None:
                        _commitopts["message"] = msg
                        _commitopts["edit"] = False
                    if _commitopts.get("edit", False):
                        _commitopts["message"] = (
                            "HG: Commit message of changeset %s\n%s"
                            % (str(c), c.description())
                        )
                    bases = [
                        replacemap.get(c.p1().node(), c.p1().node()),
                        replacemap.get(c.p2().node(), c.p2().node()),
                    ]
                    if mutation.enabled(repo):
                        preds = [
                            replacemap[p]
                            for p in mutation.predecessorsset(
                                repo, c.node(), closest=True
                            )
                            if p in replacemap
                        ]
                    else:
                        preds = []

                    newid, created = common.metarewrite(
                        repo, c, bases, commitopts=_commitopts, copypreds=preds
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
                    mutop="metaedit",
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
            with repo.wlock(), repo.lock(), repo.transaction("metaedit-checkout"):
                hg.update(repo, newp1)


def _histediting(repo):
    return repo.localvfs.exists("histedit-state")
