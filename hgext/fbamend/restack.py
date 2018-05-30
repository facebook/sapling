# restack.py - rebase to make a stack connected again
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial.i18n import _
from mercurial import commands, revsetlang

from hgext import rebase


def restack(ui, repo, rebaseopts=None):
    """Repair a situation in which one or more changesets in a stack
       have been obsoleted (thereby leaving their descendants in the stack
       unstable) by finding any such changesets and rebasing their descendants
       onto the latest version of each respective changeset.
    """
    rebaseopts = (rebaseopts or {}).copy()

    # TODO: Remove config override after https://phab.mercurial-scm.org/D1063
    config = {("experimental", "rebase.multidest"): True}

    with ui.configoverride(config), repo.wlock(), repo.lock():
        # Find drafts connected to the current stack via either changelog or
        # obsolete graph. Note: "draft() & ::." is optimized by D441.

        # 1. Connect drafts via changelog
        revs = list(repo.revs("(draft() & ::.)::"))
        if not revs:
            # "." is probably public. Check its direct children.
            revs = repo.revs("draft() & children(.)")
            if not revs:
                ui.status(_("nothing to restack\n"))
                return 1
        # 2. Connect revs via obsolete graph
        revs = list(repo.revs("successors(%ld)+allpredecessors(%ld)", revs, revs))
        # 3. Connect revs via changelog again to cover missing revs
        revs = list(repo.revs("(draft() & ::%ld)::", revs))

        rebaseopts["rev"] = [revsetlang.formatspec("%ld", revs)]
        rebaseopts["dest"] = "_destrestack(SRC)"

        rebase.rebase(ui, repo, **rebaseopts)

        # Ensure that we always end up on the latest version of the
        # current changeset. Usually, this will be taken care of
        # by the rebase operation. However, in some cases (such as
        # if we are on the precursor of the base changeset) the
        # rebase will not update to the latest version, so we need
        # to do this manually.
        successor = repo.revs("allsuccessors(.)").last()
        if successor is not None:
            commands.update(ui, repo, rev=successor)
