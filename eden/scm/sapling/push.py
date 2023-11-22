# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from collections import defaultdict
from typing import Optional

from . import edenapi_upload, error, mutation
from .bookmarks import readremotenames, saveremotenames
from .i18n import _
from .node import bin, hex, nullhex, short


def get_edenapi_for_dest(repo, _dest):
    """Get an EdenApi instance for the given destination."""
    if not repo.ui.configbool("push", "edenapi"):
        return None

    # We are focusing the prod case for now, which means we assume the
    # default push dest is the same as edenapi.url config.
    try:
        edenapi = repo.edenapi
        if edenapi.url().startswith("eager:"):
            # todo (zhaolong): implement push related EdenAPIs for eagerepo
            return None

        return edenapi
    except Exception:
        return None


def push(repo, dest, head_node, remote_bookmark, opargs=None):
    """Push via EdenApi (HTTP)"""
    ui = repo.ui
    edenapi = get_edenapi_for_dest(repo, dest)

    ui.status_err(
        _("pushing rev %s to destination %s bookmark %s\n")
        % (short(head_node), edenapi.url(), remote_bookmark)
    )

    # push revs via EdenApi
    uploaded, failed = edenapi_upload.uploadhgchangesets(repo, [head_node])
    if failed:
        raise error.Abort(
            _("failed to upload commits to server: {}").format(
                [repo[node].hex() for node in failed]
            )
        )
    ui.debug(f"uploaded {len(uploaded)} new commits\n")

    bookmark_node = get_remote_bookmark_node(ui, edenapi, remote_bookmark)
    if bookmark_node is None:
        if opargs.get("create"):
            create_remote_bookmark(ui, edenapi, remote_bookmark, head_node)
            ui.debug("remote bookmark %s created\n" % remote_bookmark)
            record_remote_bookmark(repo, remote_bookmark, head_node)
            return 0
        else:
            raise error.Abort(
                _("could not find remote bookmark '%s', use '--create' to create it")
                % remote_bookmark
            )

    # update the exiting bookmark with push rebase
    return push_rebase(repo, dest, head_node, remote_bookmark, opargs)


def push_rebase(repo, dest, head_node, remote_bookmark, opargs=None):
    """Update the remote bookmark with server side rebase.

    For updating the existing remote bookmark, push_rebase allows the server to
    rebase incoming commits as part of the push process. This helps solve the
    problem of push contention where many clients try to push at once and
    all but one fail. Instead of failing, it will rebase the incoming commit
    onto the target bookmark (i.e. @ or master) as long as the commit doesn't touch
    any files that have been modified in the target bookmark. Put another way,
    push_rebase will not perform any file content merges. It only performs the
    rebase when there is no chance of a file merge.
    """
    ui, edenapi = repo.ui, repo.edenapi
    bookmark = remote_bookmark
    ui.write(_("updating remote bookmark %s\n") % bookmark)

    # todo (zhaolong): handle public head_node case, which should be BookmarkOnlyPushRebase?

    # according to the Mononoke API (D23813368), base is the parent of the bottom of the stack
    # that is to be landed.
    draft_nodes = repo.dageval(lambda: roots(ancestors([head_node]) & draft()))
    if len(draft_nodes) > 1:
        # todo (zhaolong): handle merge commit
        raise error.Abort(_("multiple roots found for stack %s") % short(head_node))

    parents = repo[draft_nodes[0]].parents()
    if len(parents) != 1:
        raise error.Abort(
            _("{%d} parents found for commit %s")
            % (len(parents), short(draft_nodes[0]))
        )
    base = parents[0].node()

    # todo (zhaolong): support pushvars
    land_response = edenapi.landstack(
        bookmark,
        head=head_node,
        base=base,
        pushvars=[],
    )
    new_head = land_response["new_head"]
    old_to_new_hgids = land_response["old_to_new_hgids"]

    repo.pull(source=dest, headnodes=(new_head,))
    entries = [
        mutation.createsyntheticentry(repo, [node], new_node, "pushrebase")
        for (node, new_node) in old_to_new_hgids.items()
    ]
    mutation.recordentries(repo, entries, skipexisting=False)
    record_remote_bookmark(repo, bookmark, new_head)
    ui.write(_("updated remote bookmark %s to %s\n") % (bookmark, short(new_head)))
    return 0


def get_remote_bookmark_node(ui, edenapi, bookmark) -> Optional[bytes]:
    ui.debug("getting remote bookmark %s\n" % bookmark)
    response = edenapi.bookmarks([bookmark])
    hexnode = response.get(bookmark)
    return bin(hexnode) if hexnode else None


def create_remote_bookmark(ui, edenapi, bookmark, node) -> None:
    ui.write(_("creating remote bookmark %s\n") % bookmark)
    succeed = edenapi.setbookmark(bookmark, node, None, pushvars=[])
    if not succeed:
        # todo (zhaolong): add more details about why create bookmark failed.
        # In order to do that, we need to make `setbookmark` API return the
        # server error.
        raise error.Abort(_("could not create remote bookmark %s") % bookmark)


def record_remote_bookmark(repo, bookmark, new_node) -> None:
    """Record a remote bookmark in vfs.

    * bookmark - the name of the remote bookmark to update, e.g. "main"
    """
    with repo.wlock(), repo.lock(), repo.transaction("recordremotebookmark"):
        data = defaultdict(dict)  # {'remote': {'master': '<commit hash>'}}
        for hexnode, _nametype, remote, name in readremotenames(repo):
            data[remote][name] = hexnode
        remote = repo.ui.config("remotenames", "hoist")
        data.setdefault(remote, {})[bookmark] = hex(new_node)
        saveremotenames(repo, data)


def delete_remote_bookmark(repo, edenapi, bookmark) -> None:
    ui = repo.ui
    node = get_remote_bookmark_node(ui, edenapi, bookmark)
    if node is None:
        raise error.Abort(_("remote bookmark %s does not exist") % bookmark)

    # delete remote bookmark from server
    ui.write(_("deleting remote bookmark %s\n") % bookmark)
    edenapi.setbookmark(bookmark, None, node, pushvars=[])

    # delete remote bookmark from repo
    remote = repo.ui.config("remotenames", "hoist")
    remotenamechanges = {bookmark: nullhex}
    saveremotenames(repo, {remote: remotenamechanges}, override=False)
