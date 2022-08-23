# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import cmdutil, scmutil, visibility
from edenscm.mercurial.context import memctx
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex

from . import backup, backuplock
from .commands import command


@command(
    "debughiddencommit",
    [
        (
            "",
            "ignored-files",
            True,
            _("include ignored files"),
        ),
    ]
    + cmdutil.walkopts,
)
def debughiddencommit(ui, repo, *pats, **opts):
    """
    commit to commit cloud

    This command adds a commit to the commit cloud by committing
    locally, sending to commit cloud, then hiding it.

    Files in the working copy will not be changed.

    Commit hash is printed as a result of this command.
    """
    with backuplock.lock(repo), repo.wlock():
        status = repo.status()
        files = status.modified + status.added + status.removed + status.deleted
        removed = set(status.removed + status.deleted)
        user = ui.username()
        extra = {}
        date = None
        wctx = repo[None]

        matcher = scmutil.match(wctx, pats, opts, emptyalways=False)
        ignored = bool(opts.get("ignored_files"))
        includefiles = [
            x for ff in repo.dirstate.status(matcher, ignored, False, True) for x in ff
        ]
        files = list(set(files).union(set(includefiles)))

        def getfilectx(repo, memctx, path):
            if path in removed:
                return None

            return wctx[path]

        node = memctx(
            repo,
            [wctx.p1()],
            "Ephemeral commit",
            sorted(files),
            getfilectx,
            user,
            date,
            extra,
        ).commit()

        try:
            uploaded, failed = backup.backupwithlockheld(repo, [int(repo[node])])
        finally:
            # Be sure to hide the commit, even if the backup fails
            visibility.remove(repo, [node])

    if failed:
        return 2

    ui.write(_("%s\n") % hex(node))
