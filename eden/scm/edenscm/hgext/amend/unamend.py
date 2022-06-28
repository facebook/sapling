# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# unamend.py - undo an amend operation

from __future__ import absolute_import

from edenscm.mercurial import (
    autopull,
    error,
    mutation,
    node as nodemod,
    pycompat,
    registrar,
    visibility,
)
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)


@command("unamend|una|unam|uname|unamen", [])
def unamend(ui, repo, **opts):
    """undo the last amend operation on the current commit

    Reverse the effects of an :hg:`amend` operation. Hides the current commit
    and checks out the previous version of the commit. :hg:`unamend` does not
    revert the state of the working copy, so changes that were added to the
    commit in the last amend operation become pending changes in the working
    copy.

    :hg:`unamend` cannot be run on amended commits that have children. In
    other words, you cannot unamend an amended commit in the middle of a
    stack.

    .. note::

        Running :hg:`unamend` is similar to running :hg:`undo --keep`
        immediately after :hg:`amend`. However, unlike :hg:`undo`, which can
        only undo an amend if it was the last operation you performed,
        :hg:`unamend` can unamend any draft amended commit in the graph that
        does not have children.

    .. container:: verbose

      Although :hg:`unamend` is typically used to reverse the effects of
      :hg:`amend`, it actually rolls back the current commit to its previous
      version, regardless of whether the changes resulted from an :hg:`amend`
      operation or from another operation, such as :hg:`rebase`.
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
        changedfiles.extend(pycompat.iterkeys(diff))

        tr = repo.transaction("unamend")
        with dirstate.parentchange():
            dirstate.rebuild(prednode, cm, changedfiles)
            # we want added and removed files to be shown
            # properly, not with ? and ! prefixes
            for filename, data in pycompat.iteritems(diff):
                if data[0][0] is None:
                    dirstate.add(filename)
                if data[1][0] is None:
                    dirstate.remove(filename)
        changes = []
        for book in ctxbookmarks:
            changes.append((book, prednode))
        repo._bookmarks.applychanges(repo, tr, changes)
        visibility.remove(repo, [curctx.node()])
        visibility.add(repo, [predctx.node()])
        tr.close()
