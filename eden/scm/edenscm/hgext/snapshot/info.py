# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import error
from edenscm.mercurial.edenapi_upload import (
    getreponame,
)
from edenscm.mercurial.i18n import _


def info(ui, repo, csid, **opts):
    try:
        repo.edenapi.fetchsnapshot(
            getreponame(repo),
            {
                "cs_id": bytes.fromhex(csid),
            },
        )
    except Exception:
        raise error.Abort(_("snapshot doesn't exist"))
    else:
        ui.status(_("snapshot exists\n"))
