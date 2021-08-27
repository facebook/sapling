# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial.edenapi_upload import (
    getreponame,
)


def restore(ui, repo, csid, **opts):
    ui.status(f"Will restore snapshot {csid}\n", component="snapshot")

    snapshot = repo.edenapi.fetchsnapshot(
        getreponame(repo),
        {
            "cs_id": bytes.fromhex(csid),
            # TODO(yancouto): Find bubble id from snapshot id
            "bubble_id": 1,
        },
    )

    ui.status(
        f"Snapshot info: parent {snapshot['hg_parents'].hex()}\n", component="snapshot"
    )
