# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial.edenapi_upload import (
    getreponame,
    filetypefromfile,
    parentsfromctx,
)


def createremote(ui, repo, **opts):
    # Current working context
    wctx = repo[None]

    (time, tz) = wctx.date()

    untracked = [f for f in wctx.status(listunknown=True).unknown]
    removed = []
    for f in wctx.removed():
        # If a file is marked as removed but still exists, it means it was hg rm'ed
        # but then new content was written to it, in which case we consider it as
        # untracked changes.
        if wctx[f].exists():
            untracked.append(f)
        else:
            removed.append(f)

    response = repo.edenapi.uploadsnapshot(
        getreponame(repo),
        {
            "files": {
                "root": repo.root,
                "modified": [(f, filetypefromfile(wctx[f])) for f in wctx.modified()],
                "added": [(f, filetypefromfile(wctx[f])) for f in wctx.added()],
                "untracked": [(f, filetypefromfile(wctx[f])) for f in untracked],
                "removed": removed,
                "missing": [f for f in wctx.deleted()],
            },
            "author": wctx.user(),
            "time": int(time),
            "tz": tz,
            "hg_parents": parentsfromctx(wctx),
        },
    )

    csid = bytes(response["changeset_token"]["data"]["id"]["BonsaiChangesetId"]).hex()

    ui.status(f"Snapshot created with id {csid}\n", component="snapshot")
