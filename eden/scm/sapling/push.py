# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from . import edenapi_upload, error
from .i18n import _


def push_rebase(repo, remote, force=False, revs=None, bookmarks=(), opargs=None):
    """rebases commits during push

    push_rebase allows the server to rebase incoming commits as part of
    the push process. This helps solve the problem of push contention where many
    clients try to push at once and all but one fail. Instead of failing,
    it will rebase the incoming commit onto the target bookmark (i.e. @ or master)
    as long as the commit doesn't touch any files that have been
    modified in the target bookmark. Put another way, push_rebase will not perform
    any file content merges. It only performs the rebase when there is no chance of
    a file merge.
    """
    # todo:
    #   1. push to changesets
    #   2. landstack (rebase)
    #   3. push rebase state as return value

    ui = repo.ui

    # push revs via EdenApi
    uploaded, failed = edenapi_upload.uploadhgchangesets(repo, revs, force=force)
    if failed:
        raise error.Abort(
            _("failed to upload commits to server: {}").format(
                [repo[node].hex() for node in failed]
            )
        )

    ui.debug(f"uploaded {len(uploaded)} new commits\n")
