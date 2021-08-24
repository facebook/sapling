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

    response = repo.edenapi.uploadsnapshot(
        getreponame(repo),
        {
            "files": {
                "modified": [(f, filetypefromfile(wctx[f])) for f in wctx.modified()],
                "added": [(f, filetypefromfile(wctx[f])) for f in wctx.added()],
                "untracked": [
                    (f, filetypefromfile(wctx[f]))
                    for f in wctx.status(listunknown=True).unknown
                ],
                # TODO(yancouto): Files that are deleted and then have untracked modification
                # are still returned here. Ideally they would be in "untracked", which is a
                # bit confusing but more correct.
                "removed": [f for f in wctx.removed()],
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
