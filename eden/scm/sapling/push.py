# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from . import edenapi_upload, error
from .i18n import _


def push(repo, dest, head_node, remote_bookmark, opargs=None):
    """Push via EdenApi (HTTP)"""
    ui = repo.ui

    # push revs via EdenApi
    uploaded, failed = edenapi_upload.uploadhgchangesets(repo, [head_node])
    if failed:
        raise error.Abort(
            _("failed to upload commits to server: {}").format(
                [repo[node].hex() for node in failed]
            )
        )

    ui.debug(f"uploaded {len(uploaded)} new commits\n")
