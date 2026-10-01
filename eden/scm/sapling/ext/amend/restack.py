# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# restack.py - rebase to make a stack connected again


from sapling import commands, merge as mergemod, scmutil
from sapling.ext import rebase
from sapling.i18n import _


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
            revs = list(repo.revs("draft() & ((draft() & %ld)::)", revs))

            rebaseopts["rev"] = [ctx.hex() for ctx in repo.set("%ld", revs)]

        rebaseopts["dest"] = ["_destrestack(SRC)"]

        _notelandedmergesides(ui, repo, rebaseopts["rev"])
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


def _notelandedmergesides(ui, repo, revs):
    """Explain why a conflict-free merge with a landed side is left alone."""
    revs = scmutil.revrange(repo, revs)
    for ctx in repo.set("%ld & merge()", revs):
        if not mergemod.is_noconflict_merge(ctx) or repo.revs(
            "_destrestack(%d)", ctx.rev()
        ):
            continue
        for parent in ctx.parents():
            landed = mergemod.landed_successor(repo, parent)
            if landed is not None:
                ui.status(
                    _(
                        "note: not restacking conflict-free merge %s, its parent %s landed as %s\n"
                    )
                    % (ctx, parent, repo[landed])
                )
                ui.status(
                    _("(rebase the stack past the landed commit to drop the merge)\n")
                )
