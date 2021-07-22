# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm.mercurial import node as nodemod, edenapi_upload
from edenscm.mercurial.i18n import _, _n

from . import util as ccutil


def upload(repo, revs, force=False):
    """Upload draft commits using EdenApi Uploads

    Commits that have already been uploaded will be skipped.
    If no revision is specified, uploads all visible commits.
    """
    ui = repo.ui

    if revs is None:
        heads = [ctx.node() for ctx in repo.set("heads(not public())")]
    else:
        heads = [
            ctx.node()
            for ctx in repo.set(
                "heads((not public() & ::%ld))",
                revs,
            )
        ]
    if not heads:
        ui.status(_("nothing to upload\n"), component="commitcloud")
        return [], []

    # Check what heads have been already uploaded and what heads are missing
    missingheads = heads if force else edenapi_upload._filtercommits(repo, heads)

    if not missingheads:
        ui.status(_("nothing to upload\n"), component="commitcloud")
        return [], []

    # Print the heads missing on the server
    _maxoutput = 20
    for counter, node in enumerate(missingheads):
        if counter == _maxoutput:
            left = len(missingheads) - counter
            repo.ui.status(
                _n(
                    "  and %d more head...\n",
                    "  and %d more heads...\n",
                    left,
                )
                % left
            )
            break
        ui.status(
            _("head '%s' hasn't been uploaded yet\n") % nodemod.hex(node)[:12],
            component="commitcloud",
        )

    draftrevs = repo.changelog.torevset(
        repo.dageval(lambda: ancestors(missingheads) & draft())
    )

    return edenapi_upload.uploadhgchangesets(repo, draftrevs, force)
