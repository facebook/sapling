# restack.py - rebase to make a stack connected again
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.hgext import rebase
from edenscm.mercurial import commands, revsetlang
from edenscm.mercurial.i18n import _


def restack(ui, repo, **rebaseopts):
    """Repair a situation in which one or more commits in a stack
       have been obsoleted (thereby leaving their descendants in the stack
       orphaned) by finding any such commits and rebasing their descendants
       onto the latest version of each respective commit.

    """
    rebaseopts = rebaseopts.copy()

    with repo.wlock(), repo.lock():
        # Find drafts connected to the current stack via either changelog or
        # obsolete graph. Note: "draft() & ::." is optimized by D441.

        if not rebaseopts["rev"]:
            # 1. Connect drafts via changelog
            revs = list(repo.revs("(draft() & ::.)::"))
            if not revs:
                # "." is probably public. Check its direct children.
                revs = repo.revs("draft() & children(.)")
                if not revs:
                    ui.status(_("nothing to restack\n"))
                    return 1
            # 2. Connect revs via obsolete graph
            revs = list(repo.revs("successors(%ld)+predecessors(%ld)", revs, revs))
            # 3. Connect revs via changelog again to cover missing revs
            revs = list(repo.revs("(draft() & ::%ld)::", revs))

            rebaseopts["rev"] = [ctx.hex() for ctx in repo.set("%ld", revs)]

        rebaseopts["dest"] = "_destrestack(SRC)"

        rebase.rebase(ui, repo, **rebaseopts)

        # Ensure that we always end up on the latest version of the
        # current changeset. Usually, this will be taken care of
        # by the rebase operation. However, in some cases (such as
        # if we are on the precursor of the base changeset) the
        # rebase will not update to the latest version, so we need
        # to do this manually.
        successor = repo.revs("successors(.) - .").last()
        if successor is not None:
            commands.update(ui, repo, rev=repo[successor].hex())
