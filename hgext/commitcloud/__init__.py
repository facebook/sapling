# Commit cloud
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
""" sync changesets via the cloud

Configs::

    [commitcloud]
    # type of commit cloud service to connect to
    # local or remote
    servicetype = local

    # location of the commit cloud service to connect to (for servicetype = local)
    servicelocation = /path/to/dir

    # hostname to use for the system
    hostname = myhost

    # Https endpoint host serving commit cloud requests (for servicetype = remote)
    remote_host = example.commitcloud.com

    # Https endpoint port serving commit cloud requests (for servicetype = remote)
    # Default is the standard HTTPS port
    remote_port = 443

    # TLS client certificate (optional)
    # path may contain ${USER} or %i placeholders to substitute with the user identity
    # should be defined if mutual TLS is needed
    # mutual TLS is vital for TLS based user authentication
    tls.client_certs = /etc/pki/tls/certs/client_certs.pem

    # TLS CA certificate (optional)
    # path may contain ${USER} or %i placeholders to substitute with the user identity
    # should be defined if location is not default
    # see OPENSSLDIR in `openssl version -a` for the default location
    tls.ca_certs = /etc/pki/tls/certs/ca.pem

    # TLS CN/hostname check (true is the default)
    # may be required to disable if non standard format is used, for example,
    # CN field in x509 server cert contains different information rather than server's hostname.
    # TODO: It is also possible to implement a wrapper for ssl.match_hostname method instead of disabling the check.
    tls.check_hostname = False

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

    # number of heads (stacks) allowed to push without checking what the server has backuped up
    # cloud sync can partially fail and manage to push some of the stacks
    # this will prevent repush of that stacks
    # this will not be needed with mononoke as server should optimize repush attempts
    nocheckbackeduplimit = 4

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
    # path can contains ${USER} or %i to substitute with the user identity
    scm_daemon_log_path = /path/to/%i/logfile

    # Use secrets_tool for token backup between machines
    use_secrets_tool = true

    # Default workspace only! Take into account user's only commits
    # This includes all the stacks where at least one commit is user's own
    # Caution: using this option may leave some local commits unbacked up
    user_commits_only = true

    # Default workspace only! Take into account only custom set of commits
    # Caution: using this option may leave some local commits unbacked up
    custom_push_revs = draft()

    # Direct infinitepush bundle fetching command
    # the command has to be a formatted string with params: filename and handle
    get_command = bundlefetcher -h {handle} -o {filename}

    # Use direct infinitepush bundle fetching if the commands is specified
    use_direct_bundle_fetching = true
"""

from __future__ import absolute_import

from mercurial import (
    error,
    extensions,
    localrepo,
    node as nodemod,
    registrar,
    revset,
    smartset,
    util,
)
from mercurial.i18n import _

from . import commitcloudcommands, commitcloudcommon, commitcloudutil, state


cmdtable = commitcloudcommands.cmdtable

colortable = {"commitcloud.tag": "yellow", "commitcloud.team": "bold"}

hint = registrar.hint()
revsetpredicate = registrar.revsetpredicate()

configtable = {}
configitem = registrar.configitem(configtable)

configitem("commitcloud", "servicetype", default="remote")
configitem("commitcloud", "remote_port", default=443)
configitem("commitcloud", "tls.check_hostname", default=True)
configitem("commitcloud", "scm_daemon_tcp_port", default=15432)
configitem("commitcloud", "nocheckbackeduplimit", default=4)


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
                for obj in tr, self:
                    if (
                        util.safehasattr(obj, "_commitcloudskippendingobsmarkers")
                        and obj._commitcloudskippendingobsmarkers
                    ):
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


@revsetpredicate("cloudremote([set])")
def cloudremote(repo, subset, x):
    """pull missing known changesets from the remote store

    Currently only for obsoleted commits, can be extended for any commit.
    """

    args = revset.getargs(x, 1, 50, _("cloudremote takes from 1 to up to 50 hex revs"))
    args = [n[1] for n in args]

    try:
        hexnodespulled = commitcloudcommands.missingcloudrevspull(
            repo, [nodemod.bin(nodehex) for nodehex in args]
        )
        return subset & repo.unfiltered().revs("%ls", hexnodespulled)
    except Exception as e:
        commitcloudcommon.highlightstatus(
            repo.ui, _("unable to pull all changesets from the remote store\n%s\n") % e
        )
    return smartset.baseset([])
