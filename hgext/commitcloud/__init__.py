# Commit cloud
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
""" sync changesets via the cloud

    [commitcloud]
    # type of commit cloud service to connect to
    # local or remote
    servicetype = local

    # location of the commit cloud service to connect to
    # (for servicetype = local)
    servicelocation = /path/to/dir

    # hostname to use for the system
    hostname = myhost

    # Http endpoint host serving commit cloud requests
    remote_host = example.commitcloud.com

    # SSL certificates
    certs = /etc/pki/tls/certs/fb_certs.pem

    # help message to provide instruction on registration process
    auth_help = please obtain an authentication token from https://example.com/

    # custom path to store authentication token (may be used for testing)
    # the path should exist
    user_token_path = /tmp

    # owner team, used for help messages
    owner_team = "The Source Control Team"

    # education page
    education_page = https://someurl.com/wiki/CommitCloud

    # email domain to drop from email addresses for default users
    email_domain = example.com

    # update to a new revision if the current revision has been moved
    updateonmove = true

    # option to print incoming and outgoing requests to
    # commit cloud http endpoint in json format (with --debug option only)
    debugrequests = true

    # enable subscribing to commit cloud notifications via SCM Daemon
    subscription_enabled = true

    # path where SCM Daemon looks up current connected subscribers
    connected_subscribers_path = /path/to/dir

    # SCM Daemon tcp port
    scm_daemon_tcp_port = 15432

    # SCM Daemon log file (for hg rage)
    # %i is substituted with the user identity
    scm_daemon_log_path = /path/to/%i/logfile

    # Use secrets_tool for token
    use_secrets_tool = true
"""

from __future__ import absolute_import

from mercurial import error, extensions, localrepo, registrar, util
from mercurial.i18n import _

from . import commitcloudcommands, commitcloudcommon, commitcloudutil, state


cmdtable = commitcloudcommands.cmdtable

colortable = {"commitcloud.tag": "yellow", "commitcloud.team": "bold"}

hint = registrar.hint()


def _smartlogbackupmessagemap(orig, ui, repo):
    if commitcloudutil.getworkspacename(repo):
        return {
            "inprogress": "syncing",
            "pending": "sync pending",
            "failed": "not synced",
        }
    else:
        return orig(ui, repo)


def _dobackgroundcloudsync(orig, ui, repo, dest=None, command=None):
    if commitcloudutil.getworkspacename(repo) is not None:
        return orig(ui, repo, dest, ["hg", "cloud", "sync"])
    else:
        return orig(ui, repo, dest, command)


def _smartlogbackuphealthcheckmsg(orig, ui, repo):
    if commitcloudutil.getworkspacename(repo):
        commitcloudutil.SubscriptionManager(repo).checksubscription()
        commitcloudcommands.backuplockcheck(ui, repo)
    else:
        return orig(ui, repo)


def _smartlogbackupsuggestion(orig, ui, repo):
    if commitcloudutil.getworkspacename(repo):
        commitcloudcommon.highlightstatus(
            ui,
            _(
                "Run `hg cloud sync` to synchronize your workspace. "
                "If this fails,\n"
                "please report to %s.\n"
            )
            % commitcloudcommon.getownerteam(ui),
        )
    else:
        orig(ui, repo)


def extsetup(ui):
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

    commitcloudcommands.infinitepush = infinitepush
    commitcloudcommands.infinitepushbackup = infinitepushbackup

    localrepo.localrepository._wlockfreeprefix.add(commitcloudutil._obsmarkerssyncing)
    localrepo.localrepository._lockfreeprefix.add(state.SyncState.prefix)


def reposetup(ui, repo):
    class commitcloudrepo(repo.__class__):
        def transaction(self, *args, **kwargs):
            def finalize(tr):
                if util.safehasattr(tr, "_commitcloudskippendingobsmarkers"):
                    return
                markers = tr.changes["obsmarkers"]
                if markers:
                    commitcloudutil.addpendingobsmarkers(self, markers)

            tr = super(commitcloudrepo, self).transaction(*args, **kwargs)
            tr.addfinalize("commitcloudobsmarkers", finalize)
            return tr

    repo.__class__ = commitcloudrepo


@hint("commitcloud-update-on-move")
def hintupdateonmove():
    return _(
        "if you would like to update to the moved version automatically add\n"
        "[commitcloud]\n"
        "updateonmove = true\n"
        "to your .hgrc config file\n"
    )


@hint("commitcloud-sync-education")
def hintcommitcloudeducation(ui):
    education = ui.config("commitcloud", "education_page")
    if education:
        return (
            _(
                "for syncing your own commits between machines try Commit Cloud Sync\n"
                "read more information at %s"
            )
            % education
        )
