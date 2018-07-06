# hide.py - simple and user-friendly commands to hide and unhide commits
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

from mercurial import (
    bookmarks as bookmarksmod,
    cmdutil,
    error,
    extensions,
    hg,
    hintutil,
    obsolete,
    registrar,
    scmutil,
)
from mercurial.i18n import _
from mercurial.node import short


cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "^hide",
    [
        ("r", "rev", [], _("revisions to hide")),
        (
            "c",
            "cleanup",
            None,
            _("cleanup obsolete commits (eg. marked as landed, amended, etc.)"),
        ),
    ],
)
def hide(ui, repo, *revs, **opts):
    """hide changesets and their descendants

    Hidden changesets are still accessible by their hashes which can be found
    in ``hg journal``.

    If a parent of the working directory is hidden, then the working directory
    will automatically be updated to the most recent available ancestor of the
    hidden parent.

    If there is a bookmark pointing to the commit it will be removed.

    --cleanup hides all the draft, obsolete commits that don't have non-obsolete
    descendants.
    """
    if opts.get("cleanup") and len(opts.get("rev") + list(revs)) != 0:
        raise error.Abort(_("--rev and --cleanup are incompatible"))
    elif opts.get("cleanup"):
        # hides all the draft, obsolete commits that
        # don't have non-obsolete descendants
        revs = ["draft() & ::(head() & obsolete()) - ::(not obsolete())"]
    else:
        revs = list(revs) + opts.pop("rev", [])
    revs = set(scmutil.revrange(repo, revs))
    hidectxs = list(repo.set("(%ld)::", revs))

    if not hidectxs:
        raise error.Abort(_("nothing to hide"))

    with repo.wlock(), repo.lock(), repo.transaction("hide") as tr:
        # revs to be hidden
        for ctx in hidectxs:
            if not ctx.mutable():
                raise error.Abort(
                    _("cannot hide immutable changeset: %s") % ctx,
                    hint="see 'hg help phases' for details",
                )
            if not ui.quiet:
                ui.status(
                    _('hiding commit %s "%s"\n')
                    % (ctx, ctx.description().split("\n")[0][:50])
                )

        wdp = repo["."]
        newnode = wdp

        while newnode in hidectxs:
            newnode = newnode.parents()[0]

        if newnode.node() != wdp.node():
            cmdutil.bailifchanged(repo, merge=False)
            hg.update(repo, newnode, False)
            ui.status(
                _("working directory now at %s\n") % ui.label(str(newnode), "node")
            )

        # create markers
        obsolete.createmarkers(repo, [(r, []) for r in hidectxs], operation="hide")
        ui.status(_("%i changesets hidden\n") % len(hidectxs))

        # remove bookmarks pointing to hidden changesets
        hnodes = [r.node() for r in hidectxs]
        bmchanges = []
        for book, node in sorted(bookmarksmod.listbinbookmarks(repo)):
            if node in hnodes:
                bmchanges.append((book, None))
                if not ui.quiet:
                    ui.status(
                        _('removing bookmark "%s (was at: %s)"\n') % (book, short(node))
                    )

        repo._bookmarks.applychanges(repo, tr, bmchanges)

        if len(bmchanges) > 0:
            ui.status(_("%i bookmarks removed\n") % len(bmchanges))
        hintutil.trigger("undo")


@command("^unhide", [("r", "rev", [], _("revisions to unhide"))])
def unhide(ui, repo, *revs, **opts):
    """unhide changesets and their ancestors
    """
    unfi = repo.unfiltered()
    revs = list(revs) + opts.pop("rev", [])
    revs = set(scmutil.revrange(unfi, revs))
    ctxs = unfi.set("::(%ld) & obsolete()", revs)

    with repo.wlock(), repo.lock(), repo.transaction("unhide"):
        try:
            inhibit = extensions.find("inhibit")
            inhibit.revive(ctxs, operation="unhide")
        except KeyError:
            raise error.Abort(_("cannot unhide - inhibit extension " "is not enabled"))
