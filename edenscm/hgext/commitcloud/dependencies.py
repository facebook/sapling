# Copyright 2018-2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from edenscm.mercurial import error, extensions, hintutil
from edenscm.mercurial.i18n import _

from . import commitcloudcommon, commitcloudutil, workspace


infinitepush = None
infinitepushbackup = None


def extsetup(ui):
    global infinitepush
    global infinitepushbackup
    try:
        infinitepush = extensions.find("infinitepush")
    except KeyError:
        msg = _("The commitcloud extension requires the infinitepush extension")
        raise error.Abort(msg)
    try:
        infinitepushbackup = extensions.find("infinitepushbackup")
    except KeyError:
        infinitepushbackup = None

    if infinitepushbackup is not None:
        extensions.wrapfunction(
            infinitepushbackup, "_dobackgroundbackup", _dobackgroundcloudsync
        )
        extensions.wrapfunction(
            infinitepushbackup, "_smartlogbackupsuggestion", _smartlogbackupsuggestion
        )
        extensions.wrapfunction(
            infinitepushbackup, "_smartlogbackupmessagemap", _smartlogbackupmessagemap
        )
        extensions.wrapfunction(
            infinitepushbackup,
            "_smartlogbackuphealthcheckmsg",
            _smartlogbackuphealthcheckmsg,
        )


def _smartlogbackupmessagemap(orig, ui, repo):
    if workspace.currentworkspace(repo):
        return {
            "inprogress": "syncing",
            "pending": "sync pending",
            "failed": "not synced",
        }
    else:
        return orig(ui, repo)


def _dobackgroundcloudsync(orig, ui, repo, dest=None, command=None, **opts):
    if command:
        return orig(ui, repo, dest, command, **opts)
    elif workspace.currentworkspace(repo):
        return orig(ui, repo, dest, ["hg", "cloud", "sync"], **opts)
    elif ui.configbool("commitcloud", "autocloudjoin") and not workspace.disconnected(
        repo
    ):
        # Only auto-join if the user has never connected before.  If they
        # deliberately disconnected, don't automatically rejoin.
        return orig(ui, repo, dest, ["hg", "cloud", "join"], **opts)
    else:
        return orig(ui, repo, dest, **opts)


def _smartlogbackuphealthcheckmsg(orig, ui, repo, **opts):
    if workspace.currentworkspace(repo):
        commitcloudutil.SubscriptionManager(repo).checksubscription()
        commitcloudutil.backuplockcheck(ui, repo)
        hintutil.trigger("commitcloud-old-commits", repo)
    else:
        return orig(ui, repo, **opts)


def _smartlogbackupsuggestion(orig, ui, repo):
    if workspace.currentworkspace(repo):
        ui.status(
            _(
                "run 'hg cloud sync' to synchronize your workspace.\n"
                "(if this fails, please report to %s)\n"
            )
            % commitcloudcommon.getownerteam(ui),
            component="commitcloud",
        )
    else:
        orig(ui, repo)
