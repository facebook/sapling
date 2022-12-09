# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""accelerated @prog@ functionality in Eden checkouts

This overrides the dirstate to check with the eden daemon for modifications,
instead of doing a normal scan of the filesystem.
"""

from . import (
    EdenThriftClient as thrift,
    error,
    localrepo,
    merge as mergemod,
    progress,
    pycompat,
    util,
)
from .i18n import _


CheckoutMode = thrift.CheckoutMode
ConflictType = thrift.ConflictType

_repoclass = localrepo.localrepository


# This function is called by merge.update() in the fast path
# to ask the eden daemon to perform the update operation.
@util.timefunction("edenupdate", 0, "ui")
def update(
    repo,
    node,
    branchmerge,
    force,
    ancestor=None,
    mergeancestor=False,
    labels=None,
    matcher=None,
    mergeforce=False,
    updatecheck=None,
    wc=None,
):
    repo.ui.debug("using eden update code path\n")

    with repo.wlock():
        wctx = repo[None]
        parents = wctx.parents()

        p1ctx = parents[0]
        destctx = repo[node]
        deststr = str(destctx)

        if not force:
            # Make sure there isn't an outstanding merge or unresolved files.
            if len(parents) > 1:
                raise error.Abort(_("outstanding uncommitted merge"))
            ms = mergemod.mergestate.read(repo)
            if list(ms.unresolved()):
                raise error.Abort(_("outstanding merge conflicts"))

            # The vanilla merge code disallows updating between two unrelated
            # branches if the working directory is dirty.  I don't really see a
            # good reason to disallow this; it should be treated the same as if
            # we committed the changes, checked out the other branch then tried
            # to graft the changes here.

            if p1ctx == destctx:
                # No update to perform.
                # Just invoke the hooks and return.
                repo.hook("preupdate", throw=True, parent1=deststr, parent2="")
                repo.hook("update", parent1=deststr, parent2="", error=0)
                return 0, 0, 0, 0

            # If we are in noconflict mode, then we must do a DRY_RUN first to
            # see if there are any conflicts that should prevent us from
            # attempting the update.
            if updatecheck == "noconflict":
                with progress.spinner(repo.ui, _("conflict check")):
                    conflicts = repo.dirstate.eden_client.checkout(
                        destctx.node(),
                        CheckoutMode.DRY_RUN,
                        manifest=destctx.manifestnode(),
                    )
                if conflicts:
                    actions = _determine_actions_for_conflicts(repo, p1ctx, conflicts)
                    _check_actions_and_raise_if_there_are_conflicts(actions)

        # Invoke the preupdate hook
        repo.hook("preupdate", throw=True, parent1=deststr, parent2="")
        # Record that we're in the middle of an update
        try:
            vfs = repo.localvfs
        except AttributeError:
            vfs = repo.vfs
        vfs.writeutf8("updatestate", destctx.hex())

        # Ask eden to perform the checkout
        if force:
            # eden_client.checkout() returns the list of conflicts here,
            # but since this is a force update it will have already replaced
            # the conflicts with the destination file state, so we don't have
            # to do anything with them here.
            with progress.spinner(repo.ui, _("updating")):
                conflicts = repo.dirstate.eden_client.checkout(
                    destctx.node(),
                    CheckoutMode.FORCE,
                    manifest=destctx.manifestnode(),
                )
            # We do still need to make sure to update the merge state though.
            # In the non-force code path the merge state is updated in
            # _handle_update_conflicts().
            ms = mergemod.mergestate.clean(repo, p1ctx.node(), destctx.node(), labels)
            ms.commit()

            stats = 0, 0, 0, 0
            actions = {}
        else:
            with progress.spinner(repo.ui, _("updating")):
                conflicts = repo.dirstate.eden_client.checkout(
                    destctx.node(),
                    CheckoutMode.NORMAL,
                    manifest=destctx.manifestnode(),
                )
            # TODO(mbolin): Add a warning if we did a DRY_RUN and the conflicts
            # we get here do not match. Only in the event of a race would we
            # expect them to differ from when the DRY_RUN was done (or if we
            # decide that DIRECTORY_NOT_EMPTY conflicts do not need to be
            # reported during a DRY_RUN).

            stats, actions = _handle_update_conflicts(
                repo, wctx, p1ctx, destctx, labels, conflicts, force
            )

        with repo.dirstate.parentchange():
            if force:
                # If the user has done an `goto --clean`, then we should
                # remove all entries from the dirstate. Note this call to
                # clear() will also remove the parents, but we set them on the
                # next line, so we'll be OK.
                repo.dirstate.clear()
            # TODO(mbolin): Set the second parent, if appropriate.
            repo.setparents(destctx.node())
            mergemod.recordupdates(repo, actions, branchmerge)

            # Clear the update state
            util.unlink(vfs.join("updatestate"))

    # Invoke the update hook
    repo.hook("edenfs-update", parent1=deststr, parent2="", error=stats[3])
    repo.hook("update", parent1=deststr, parent2="", error=stats[3])

    return stats


def _handle_update_conflicts(repo, wctx, src, dest, labels, conflicts, force):
    # When resolving conflicts during an update operation, the working
    # directory (wctx) is one side of the merge, the destination commit (dest)
    # is the other side of the merge, and the source commit (src) is treated as
    # the common ancestor.
    #
    # This is what we want with respect to the graph topology.  If we are
    # updating from commit A (src) to B (dest), and the real ancestor is C, we
    # effectively treat the update operation as reverting all commits from A to
    # C, then applying the commits from C to B.  We are then trying to re-apply
    # the local changes in the working directory (against A) to the new
    # location B.  Using A as the common ancestor in this operation is the
    # desired behavior.
    actions = _determine_actions_for_conflicts(repo, src, conflicts)
    return _applyupdates(repo, actions, wctx, dest, labels, conflicts)


def _determine_actions_for_conflicts(repo, src, conflicts):
    """Calculate the actions for _applyupdates()."""
    # Build a list of actions to pass to mergemod.applyupdates()
    actions = dict(
        (m, [])
        for m in [
            "a",
            "am",
            "cd",
            "dc",
            "dg",
            "dm",
            "e",
            "f",
            "g",  # create or modify
            "k",
            "m",
            "p",  # path conflicts
            "pr",  # files to rename
            "r",
            "rg",
        ]
    )

    for conflict in conflicts:
        # The action tuple is:
        # - path_in_1, path_in_2, path_in_ancestor, move, ancestor_node
        path = pycompat.decodeutf8(conflict.path)

        if conflict.type == ConflictType.ERROR:
            # We don't record this as a conflict for now.
            # We will report the error, but the file will show modified in
            # the working directory status after the update returns.
            repo.ui.write_err(_("error updating %s: %s\n") % (path, conflict.message))
            continue
        elif conflict.type == ConflictType.MODIFIED_REMOVED:
            action_type = "cd"
            action = (path, None, path, False, src.node())
            prompt = "prompt changed/deleted"
        elif conflict.type == ConflictType.UNTRACKED_ADDED:
            # In core Mercurial, this is the case where the file does not exist
            # in the manifest of the common ancestor for the merge.
            # TODO(mbolin): Check for the "both renamed from " case in
            # manifestmerge(), which is the other possibility when the file
            # does not exist in the manifest of the common ancestor for the
            # merge.
            action_type = "m"
            action = (path, path, None, False, src.node())
            prompt = "both created"
        elif conflict.type == ConflictType.REMOVED_MODIFIED:
            action_type = "dc"
            action = (None, path, path, False, src.node())
            prompt = "prompt deleted/changed"
        elif conflict.type == ConflictType.MISSING_REMOVED:
            # Nothing to do here really.  The file was already removed
            # locally in the working directory before, and it was removed
            # in the new commit.
            continue
        elif conflict.type == ConflictType.MODIFIED_MODIFIED:
            action_type = "m"
            action = (path, path, path, False, src.node())
            prompt = "versions differ"
        elif conflict.type == ConflictType.DIRECTORY_NOT_EMPTY:
            # This is a file in a directory that Eden would have normally
            # removed as part of the checkout, but it could not because this
            # untracked file was here. Just leave it be.
            continue
        else:
            raise RuntimeError(
                "unknown conflict type received from eden: "
                "%r, %r, %r" % (conflict.type, path, conflict.message)
            )

        actions[action_type].append((path, action, prompt))

    return actions


def _check_actions_and_raise_if_there_are_conflicts(actions):
    # In stock Hg, update() performs this check once it gets the set of actions.
    conflict_paths = []
    for action_type, list_of_tuples in pycompat.iteritems(actions):
        if len(list_of_tuples) == 0:
            continue  # Note `actions` defaults to [] for all keys.
        if action_type not in ("g", "k", "e", "r", "rg", "pr"):
            conflict_paths.extend(t[0] for t in list_of_tuples)

    # Report the exact files with conflicts.
    # There can be conflicts even when `hg status` reports no modifications if
    # the conflicts are between ignored files that exist in the destination
    # commit.
    if conflict_paths:
        # Only show 10 lines worth of conflicts
        conflict_paths.sort()
        max_to_show = 10
        if len(conflict_paths) > max_to_show:
            # If there are more than 10 conflicts, show the first 9
            # and make the last line report how many other conflicts there are
            total_conflicts = len(conflict_paths)
            conflict_paths = conflict_paths[: max_to_show - 1]
            num_remaining = total_conflicts - len(conflict_paths)
            conflict_paths.append("... (%d more conflicts)" % num_remaining)
        msg = _("conflicting changes:\n  ") + "\n  ".join(conflict_paths)
        hint = _("commit or goto --clean to discard changes")
        raise error.Abort(msg, hint=hint)


def _applyupdates(repo, actions, wctx, dest, labels, conflicts):
    numerrors = sum(1 for c in conflicts if c.type == ConflictType.ERROR)

    # Call applyupdates
    # Note that applyupdates may mutate actions.
    stats = mergemod.applyupdates(
        repo, actions, wctx, dest, overwrite=False, labels=labels
    )

    # Add the error count to the number of unresolved files.
    # This ensures we exit unsuccessfully if there were any errors
    return (stats[0], stats[1], stats[2], stats[3] + numerrors), actions
