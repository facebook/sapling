# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import visibility
from edenscm.mercurial.context import memctx
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex, nullid

from . import backup
from .commands import command


@command(
    "debugephemeralcommit",
    [],
)
def debugephemeralcommit(ui, repo, **opts):
    """
    commit to commit cloud

    This command adds a commit to the commit cloud by committing
    locally, sending to commit cloud, then hiding it.

    Files in the working copy will not be changed.

    Commit hash is printed as a result of this command.
    """
    with repo.wlock():
        status = repo.status()
        files = status.modified + status.added + status.removed + status.deleted
        removed = set(status.removed + status.deleted)
        user = ui.username()
        extra = {}
        date = None

        def getfilectx(repo, memctx, path):
            if path in removed:
                return None

            return wctx[path]

        wctx = repo[None]

        node = memctx(
            repo,
            [wctx.p1().node(), nullid],
            "Ephemeral commit",
            sorted(files),
            getfilectx,
            user,
            date,
            extra,
        ).commit()

        visibility.remove(repo, [node])

    backup.backup(repo, [int(repo[node])])

    ui.write(_("%s\n") % hex(node))
