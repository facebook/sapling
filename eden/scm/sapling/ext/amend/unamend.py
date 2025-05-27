# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# unamend.py - undo an amend operation


from sapling import autopull, error, mutation, node as nodemod, registrar, visibility
from sapling.i18n import _

cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "unamend|una",
    [],
    _("@prog@ unamend"),
    legacyaliases=["unam", "uname", "unamen"],
)
def unamend(ui, repo, **opts):
    """undo the last amend operation on the current commit

    Reverse the effects of an :prog:`amend` operation. Hides the current commit
    and checks out the previous version of the commit. :prog:`unamend` does not
    revert the state of the working copy, so changes that were added to the
    commit in the last amend operation become pending changes in the working
    copy.

    :prog:`unamend` cannot be run on amended commits that have children. In
    other words, you cannot unamend an amended commit in the middle of a
    stack.

    .. note::

        Running :prog:`unamend` is similar to running :prog:`undo --keep`
        immediately after :prog:`amend`. However, unlike :prog:`undo`, which can
        only undo an amend if it was the last operation you performed,
        :prog:`unamend` can unamend any draft amended commit in the graph that
        does not have children.

    .. container:: verbose

      Although :prog:`unamend` is typically used to reverse the effects of
      :prog:`amend`, it actually rolls back the current commit to its previous
      version, regardless of whether the changes resulted from an :prog:`amend`
      operation or from another operation. We disallow :prog:`unamend` if the
      predecessor's parents don't match the current commit's parents to avoid
      unexpected behavior after, for example, :prog:`rebase`.
    """
    unfi = repo

    # identify the commit from which to unamend
    curctx = repo["."]

    # identify the commit to which to unamend
    if mutation.enabled(repo):
        prednodes = curctx.mutationpredecessors()
        if not prednodes:
            prednodes = []
    else:
        prednodes = []

    if repo.ui.configbool("experimental", "unamend-siblings-only", True):
        autopull.trypull(unfi, [nodemod.hex(n) for n in prednodes])

        # Filters to predecessors with the same parents as the current commit.
        # This avoids accidentally resetting across a rebase.
        siblingpredctxs = [
            ctx
            for ctx in (unfi[n] for n in prednodes)
            if ctx.parents() == curctx.parents()
        ]
        if not siblingpredctxs:
            if prednodes:
                raise error.Abort(
                    _("commit was not amended"),
                    hint=_(
                        """use "hg undo" to undo the last command, or "hg reset COMMIT" to reset to a previous commit, or see "hg journal" to view commit mutations"""
                    ),
                )
            else:
                raise error.Abort(_("commit has no predecessors"))
        elif len(siblingpredctxs) > 1:
            raise error.Abort(
                _("commit has too many predecessors (%i)") % len(siblingpredctxs)
            )
        else:
            predctx = siblingpredctxs[0]
    else:
        if len(prednodes) != 1:
            e = _("changeset must have one predecessor, found %i predecessors")
            raise error.Abort(e % len(prednodes))
        prednode = prednodes[0]

        if prednode not in unfi:
            # Trigger autopull.
            autopull.trypull(unfi, [nodemod.hex(prednode)])

        predctx = unfi[prednode]

    if curctx.children():
        raise error.Abort(_("cannot unamend in the middle of a stack"))

    with repo.wlock(), repo.lock():
        ctxbookmarks = curctx.bookmarks()
        changedfiles = []
        wctx = repo[None]
        wm = wctx.manifest()
        cm = predctx.manifest()
        dirstate = repo.dirstate
        diff = cm.diff(wm)
        changedfiles.extend(diff.keys())

        tr = repo.transaction("unamend")
        with dirstate.parentchange():
            dirstate.rebuild(predctx.node(), cm, changedfiles)
            # we want added and removed files to be shown
            # properly, not with ? and ! prefixes
            for filename, data in diff.items():
                if data[0][0] is None:
                    dirstate.add(filename)
                if data[1][0] is None:
                    dirstate.remove(filename)
        changes = []
        for book in ctxbookmarks:
            changes.append((book, predctx.node()))
        repo._bookmarks.applychanges(repo, tr, changes)
        visibility.remove(repo, [curctx.node()])
        visibility.add(repo, [predctx.node()])
        tr.close()
