# Commit cloud
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""back up and sync changesets via the cloud

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
    # File containing trusted CA's to validate server certificates
    # path may contain ${USER} or %i placeholders to substitute with the user identity
    # should be defined if location is not default
    # see OPENSSLDIR in `openssl version -a` for the default location
    tls.ca_certs = /etc/pki/tls/certs/ca.pem

    # TLS CN/hostname check (true is the default)
    # may be required to disable if non standard format is used, for example,
    # CN field in x509 server cert contains different information rather than server's hostname.
    # TODO: It is also possible to implement a wrapper for ssl.match_hostname method instead of disabling the check.
    tls.check_hostname = False

    # set true if TLS authentication is enough for the https endpoint port serving commit cloud requests
    # this option definetely require the path to a valid client certificate in tls.client_certs option
    # Try openssl to verify your TLS handshake
    # Example: `openssl s_client -connect host:port -cert <tls.client_certs> -CAfile <tls.ca_certs>
    tls.notoken = False

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

    # max number of heads allowed to push without checking what the server has backed up
    # cloud sync can partially fail and manage to push some of the stacks
    # this will prevent repush of that stacks
    backuplimitnocheck = 4

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

    # Maximum age (in days) of commits to pull when syncing
    max_sync_age = 14

    # Connect repos to commit cloud during automigration (at the end of pull).
    automigrate = True

    # When connecting during automigration, connect to a workspace named
    # after the host, rather than the default workspace
    automigratehostworkspace = True

    [infinitepushbackup]
    # Whether to enable automatic backups. If this option is True then a backup
    # process will be started after every mercurial command that modifies the
    # repo, for example, commit, amend, histedit, rebase etc.
    autobackup = False

    # path to the directory where background backup logs should be stored
    logdir = path/to/dir

    # Backup at most maxheadstobackup heads, other heads are ignored.
    # Negative number means backup everything.
    maxheadstobackup = -1

    # Nodes that should not be backed up.  Descendants of these nodes won't be
    # backed up either
    dontbackupnodes = badbadbad1 badbadbad2

    # Hostname value to use. If not specified then socket.gethostname() will
    # be used
    hostname = myhost

    # Enable reporting of background backup status as a summary at the end
    # of smartlog.
    enablestatus = False

    # Enable creating obsolete markers when backup is restored.
    createlandedasmarkers = False

    # Number of backups to list by default in getavailablebackups
    backuplistlimit = 10
"""

from __future__ import absolute_import

import socket

from edenscm.mercurial import (
    extensions,
    localrepo,
    node as nodemod,
    registrar,
    revset,
    smartset,
    util as hgutil,
)
from edenscm.mercurial.i18n import _

from . import (
    background,
    backupbookmarks,
    backuplock,
    backupstate,
    checkoutlocations,
    commands as cccommands,
    dependencies,
    obsmarkers,
    status,
    syncstate,
    util as ccutil,
    workspace,
)


cmdtable = cccommands.cmdtable

colortable = {"commitcloud.tag": "yellow", "commitcloud.team": "bold"}

hint = registrar.hint()
revsetpredicate = registrar.revsetpredicate()
templatekeyword = registrar.templatekeyword()

configtable = {}
configitem = registrar.configitem(configtable)

configitem("commitcloud", "servicetype", default="remote")
configitem("commitcloud", "remote_port", default=443)
configitem("commitcloud", "tls.check_hostname", default=True)
configitem("commitcloud", "scm_daemon_tcp_port", default=15432)
configitem("commitcloud", "backuplimitnocheck", default=4)
configitem("commitcloud", "automigrate", default=False)
configitem("commitcloud", "automigratehostworkspace", default=False)
configitem("commitcloud", "synccheckoutlocations", default=False)
configitem("infinitepushbackup", "backuplistlimit", default=5)
configitem("infinitepushbackup", "enablestatus", default=True)
configitem("infinitepushbackup", "maxheadstobackup", default=-1)


def extsetup(ui):
    background.extsetup(ui)
    dependencies.extsetup(ui)

    localrepo.localrepository._wlockfreeprefix.add(obsmarkers._obsmarkerssyncing)
    localrepo.localrepository._wlockfreeprefix.add(backuplock.progressfilename)
    localrepo.localrepository._wlockfreeprefix.add(backupbookmarks._backupstateprefix)
    localrepo.localrepository._wlockfreeprefix.add(backupstate.BackupState.prefix)
    localrepo.localrepository._wlockfreeprefix.add(background._autobackupstatefile)
    localrepo.localrepository._lockfreeprefix.add(syncstate.SyncState.prefix)

    def wrapsmartlog(loaded):
        if not loaded:
            return
        smartlogmod = extensions.find("smartlog")
        extensions.wrapcommand(smartlogmod.cmdtable, "smartlog", _smartlog)

    extensions.afterloaded("smartlog", wrapsmartlog)


def reposetup(ui, repo):
    synccheckout = ui.configbool("commitcloud", "synccheckoutlocations")

    def _sendlocation(orig, self, ui, prefix, *args, **kwargs):
        if prefix == "post":
            parents = [nodemod.hex(p) if p != nodemod.nullid else "" for p in self._pl]
            p1 = parents[0]
            # TODO(T52387128): do it asynchronously in the background
            checkoutlocations.send(ui, repo, p1, **kwargs)
            return orig(self, ui, prefix)

    if synccheckout:
        extensions.wrapfunction(localrepo.dirstate.dirstate, "loginfo", _sendlocation)

    class commitcloudrepo(repo.__class__):
        def transaction(self, *args, **kwargs):
            def finalize(tr):
                for obj in tr, self:
                    if (
                        hgutil.safehasattr(obj, "_commitcloudskippendingobsmarkers")
                        and obj._commitcloudskippendingobsmarkers
                    ):
                        return

                markers = tr.changes["obsmarkers"]
                if markers:
                    obsmarkers.addpendingobsmarkers(self, markers)

            tr = super(commitcloudrepo, self).transaction(*args, **kwargs)
            tr.addfinalize("commitcloudobsmarkers", finalize)
            return tr

        def automigratefinish(self):
            super(commitcloudrepo, self).automigratefinish()
            automigrate = self.ui.configbool("commitcloud", "automigrate")
            if automigrate and not workspace.disconnected(self):
                workspacename = None
                if self.ui.configbool("commitcloud", "automigratehostworkspace"):
                    workspacename = self.ui.config(
                        "commitcloud", "hostname", socket.gethostname()
                    )
                cccommands.cloudrejoin(self.ui, self, workspace=workspacename)

    repo.__class__ = commitcloudrepo


def _smartlog(orig, ui, repo, **opts):
    res = orig(ui, repo, **opts)
    status.summary(repo)
    return res


@hint("commitcloud-old-commits")
def _smartlogomittedcommitsmsg(repo):
    return _(
        "some older commits or bookmarks have not been synced to this repo\n"
        "(run 'hg cloud sl' to see all of the commits in your workspace)\n"
        "(run 'hg pull -r HASH' to fetch commits by hash)\n"
        "(run 'hg cloud sync --full' to fetch everything - this may be slow)\n"
    )


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
        hexnodespulled = missingcloudrevspull(
            repo, [nodemod.bin(nodehex) for nodehex in args]
        )
        return subset & repo.unfiltered().revs("%ls", hexnodespulled)
    except Exception as e:
        repo.ui.status(
            repo.ui,
            _("unable to pull all changesets from the remote store\n%s\n") % e,
            component="commitcloud",
        )
    return smartset.baseset([])


def missingcloudrevspull(repo, nodes):
    """pull wrapper for changesets that are known to the obstore and unknown for the repo

    This is, for example, the case for all hidden revs on new clone + cloud sync.
    """
    unfi = repo.unfiltered()

    def obscontains(nodebin):
        return bool(unfi.obsstore.successors.get(nodebin, None))

    nodes = [node for node in nodes if node not in unfi and obscontains(node)]
    if nodes:
        pullcmd, pullopts = ccutil.getcommandandoptions("pull|pul")
        pullopts["rev"] = [nodemod.hex(node) for node in nodes]
        pullcmd(repo.ui, unfi, **pullopts)

    return nodes


@revsetpredicate("backedup")
def backedup(repo, subset, x):
    """draft changesets that have been backed up to Commit Cloud"""
    unfi = repo.unfiltered()
    state = backupstate.BackupState(repo, ccutil.getremotepath(repo, None))
    backedup = unfi.revs("not public() and ::%ln", state.heads)
    return smartset.filteredset(subset & repo.revs("draft()"), lambda r: r in backedup)


@revsetpredicate("notbackedup")
def notbackedup(repo, subset, x):
    """changesets that have not yet been backed up to Commit Cloud"""
    unfi = repo.unfiltered()
    state = backupstate.BackupState(repo, ccutil.getremotepath(repo, None))
    backedup = unfi.revs("not public() and ::%ln", state.heads)
    return smartset.filteredset(
        subset & repo.revs("not public() - hidden()"), lambda r: r not in backedup
    )


@templatekeyword("backingup")
def backingup(repo, **args):
    """whether commit cloud is currently backing up commits."""
    # If the backup lock exists then a backup should be in progress.
    return backuplock.islocked(repo)
