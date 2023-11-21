# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from collections import defaultdict
from typing import Optional

from . import edenapi_upload, error
from .bookmarks import readremotenames, saveremotenames
from .i18n import _
from .node import bin, hex


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
            ui.debug(_("remote bookmark %s created\n") % remote_bookmark)
            record_remote_bookmark(repo, remote_bookmark, head_node)
            return 0
        else:
            raise error.Abort(
                _("could not find remote bookmark '%s', use '--create' to create it")
                % remote_bookmark
            )

    return 0


def get_remote_bookmark_node(ui, edenapi, bookmark) -> Optional[bytes]:
    ui.debug(_("getting remote bookmark %s\n") % bookmark)
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
