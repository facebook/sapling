# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import time

from edenscm.mercurial import hintutil, node as nodemod, util
from edenscm.mercurial.i18n import _

from . import (
    background,
    backuplock,
    error as ccerror,
    subscription,
    syncstate,
    workspace,
)


def summary(repo):
    ui = repo.ui
    if not ui.configbool("infinitepushbackup", "enablestatus"):
        return

    # Output backup status if enablestatus is on
    if not background.autobackupenabled(repo):
        timestamp = background.autobackupdisableduntil(repo)
        if timestamp is not None:
            ui.write(
                _(
                    "background backup is currently disabled until %s\n"
                    "so your commits are not being backed up.\n"
                    "(run 'hg cloud enable' to turn automatic backups back on)\n"
                )
                % util.datestr(util.makedate(int(timestamp))),
                notice=_("note"),
            )
        else:
            ui.write(
                _(
                    "background backup is currently disabled so your commits are not being backed up.\n"
                ),
                notice=_("note"),
            )

    workspacename = workspace.currentworkspace(repo)
    if workspacename:
        subscription.check(repo)
        backuplock.status(repo)
        lastsyncstate = syncstate.SyncState(repo, workspacename)
        if lastsyncstate.omittedheads or lastsyncstate.omittedbookmarks:
            hintutil.trigger("commitcloud-old-commits", repo)

    # Don't output the summary if a backup is currently in progress.
    if backuplock.islocked(repo):
        return

    unbackeduprevs = repo.revs("notbackedup()")

    # Count the number of changesets that haven't been backed up for 10 minutes.
    # If there is only one, also print out its hash.
    backuptime = time.time() - 10 * 60  # 10 minutes ago
    count = 0
    singleunbackeduprev = None
    for rev in unbackeduprevs:
        if repo[rev].date()[0] <= backuptime:
            singleunbackeduprev = rev
            count += 1
    if count > 0:
        if count > 1:
            ui.warn(_("%d changesets are not backed up.\n") % count, notice=_("note"))
        else:
            ui.warn(
                _("changeset %s is not backed up.\n")
                % nodemod.short(repo[singleunbackeduprev].node()),
                notice=_("note"),
            )
        if workspacename:
            ui.warn(_("(run 'hg cloud sync' to synchronize your workspace)\n"))
        else:
            ui.warn(_("(run 'hg cloud backup' to perform a backup)\n"))
        ui.warn(_("(if this fails, please report to %s)\n") % ccerror.getownerteam(ui))
