# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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

    # url of the endpoint serving commit cloud requests (for servicetype = remote)
    url = https://example.commitcloud.com

    # set false if TLS certs should be used to authenticate with the commit cloud service instead
    token_enforced = True

    # help message to provide instruction on registration process
    auth_help = please obtain an authentication token from https://example.com/

    # custom path to store authentication token (may be used for testing)
    # the path should exist
    user_token_path = /tmp

    # education page
    education_page = https://someurl.com/wiki/CommitCloud

    # list of email domains to drop from email addresses for default users
    email_domains = example.com

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
    # path can contains ${USER} or %i to substitute with the user identity
    scm_daemon_log_path = /path/to/%i/logfile

    # Maximum age (in days) of commits to pull when syncing
    max_sync_age = 14

    # Connect repos to commit cloud during automigration (at the end of pull).
    automigrate = True

    # When connecting during automigration, connect to a workspace named
    # after the host, rather than the default workspace
    automigratehostworkspace = True

    # Enable reporting of background sync status in the smartlog.
    enablestatus = True

    # Enable reporting of background sync progress in the smartlog.
    enableprogress = True

    # Limit for number of commits in a group when pull (if unhydratedcommits enabled)
    unhydratedpullsizelimit = 5000

    # Show remotebookmarks in Commit Cloud Smartlog (ask the server to send them).
    # By default only remote bookmarks that belong to draft commits (scratch bookmarks) or their public roots are returned.
    sl_showremotebookmarks = False

    # Show all local bookmarks in Commit Cloud Smartlog (ask the server to send them).
    # By default only local bookmarks that belong to draft commits or their public roots are returned.
    sl_showallbookmarks = False

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

    # Use EdenApi Uploads for uploading commit cloud commits during sync
    usehttpupload = True

    # The command to download bundles from a backup bundle store
    # the command has to be a formatted string with params: 'filename' and 'handle'
    get_command = bundlefetcher -h {handle} -o {filename}
"""

from __future__ import absolute_import

from edenscm.mercurial import (
    extensions,
    localrepo,
    node as nodemod,
    registrar,
    smartset,
)
from edenscm.mercurial.i18n import _

from . import (
    background,
    backuplock,
    backupstate,
    checkoutlocations,
    commands as cccommands,
    debughiddencommit,
    dependencies,
    status,
    sync,
    syncstate,
    util as ccutil,
    workspace,
)

debughiddencommit.command  # Suppressing "unused import" lint

cmdtable = cccommands.cmdtable

colortable = {"commitcloud.tag": "yellow", "commitcloud.team": "bold"}

hint = registrar.hint()
revsetpredicate = registrar.revsetpredicate()
templatekeyword = registrar.templatekeyword()

configtable = {}
configitem = registrar.configitem(configtable)

configitem("commitcloud", "servicetype", default="remote")
configitem("commitcloud", "token_enforced", default=True)
configitem("commitcloud", "scm_daemon_tcp_port", default=15432)
configitem("commitcloud", "automigrate", default=False)
configitem("commitcloud", "automigratehostworkspace", default=False)
configitem("commitcloud", "synccheckoutlocations", default=False)
configitem("commitcloud", "enablestatus", default=True)
configitem("commitcloud", "enableprogress", default=True)
configitem("commitcloud", "unhydratedpullsizelimit", 5000)
configitem("commitcloud", "sl_showremotebookmarks", False)
configitem("commitcloud", "sl_showallbookmarks", False)
configitem("commitcloud", "usehttpupload", False)
configitem(
    "commitcloud", "get_command", default="jf download --filepath {filename} {handle}"
)
configitem("infinitepushbackup", "enablestatus", default=True)
configitem("infinitepushbackup", "maxheadstobackup", default=-1)


def extsetup(ui):
    background.extsetup(ui)
    dependencies.extsetup(ui)

    localrepo.localrepository._wlockfreeprefix.add(backuplock.progressfilename)
    localrepo.localrepository._wlockfreeprefix.add(backupstate.BackupState.directory)
    localrepo.localrepository._wlockfreeprefix.add(background._autobackupstatefile)
    localrepo.localrepository._lockfreeprefix.add(syncstate.SyncState.prefix)
    localrepo.localrepository._lockfreeprefix.add(sync._syncstatusfile)

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
        def automigratefinish(self):
            super(commitcloudrepo, self).automigratefinish()
            # Do not auto rejoin if the repo has "broken" (incomplete) commit
            # graph.
            automigrate = self.ui.configbool("commitcloud", "automigrate") and (
                "emergencychangelog" not in self.storerequirements
            )
            if (
                automigrate
                and not workspace.disconnected(self)
                and background.autobackupenabled(self)
            ):
                workspacename = None
                if self.ui.configbool("commitcloud", "automigratehostworkspace"):
                    workspacename = workspace.hostnameworkspace(self.ui)
                try:
                    cccommands.cloudrejoin(self.ui, self, raw_workspace=workspacename)
                except Exception as ex:
                    self.ui.warn(
                        _("warning: failed to auto-join cloud workspace: '%s'\n") % ex
                    )

    repo.__class__ = commitcloudrepo


def _smartlog(orig, ui, repo, **opts):
    res = orig(ui, repo, **opts)
    status.summary(repo)
    return res


@hint("commitcloud-username-migration")
def _smartlogusernamemigrationmsg(repo):
    return _(
        "username configuration has been changed\n"
        "please, run `hg cloud reclaim` to migrate your commit cloud workspaces\n"
    )


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


@hint("commitcloud-switch")
def hintcommitcloudswitch(ui, active):
    wliststr = "\n".join([winfo.name for winfo in active])
    return ui.label(
        _(
            "the following commitcloud workspaces (backups) are available for this repo:\n%s\n"
        )
        % wliststr,
        "bold",
    ) + _(
        "run `hg cloud list` inside the repo to see all your workspaces,\n"
        "find the one the repo is connected to and learn how to switch between them\n"
    )


@revsetpredicate("backedup")
def backedup(repo, subset, x):
    """draft changesets that have been backed up to Commit Cloud"""
    path = ccutil.getnullableremotepath(repo.ui)
    if not path:
        return smartset.baseset(repo=repo)
    heads = backupstate.BackupState(repo, path).heads
    cl = repo.changelog
    if cl.algorithmbackend == "segments":
        backedup = repo.dageval(lambda: draft() & ancestors(heads))
        return subset & cl.torevset(backedup)
    backedup = repo.revs("not public() and ::%ln", heads)
    return smartset.filteredset(subset & repo.revs("draft()"), lambda r: r in backedup)


@revsetpredicate("notbackedup")
def notbackedup(repo, subset, x):
    """changesets that have not yet been backed up to Commit Cloud"""
    path = ccutil.getnullableremotepath(repo.ui)
    if not path:
        # arguably this should return draft(). However, since there is no
        # remote, and no way to do backup, returning an empty set avoids
        # upsetting users with "not backed up" warnings.
        return smartset.baseset(repo=repo)
    heads = backupstate.BackupState(repo, path).heads
    cl = repo.changelog
    if cl.algorithmbackend == "segments":
        notbackedup = repo.dageval(lambda: draft() - ancestors(heads))
        return subset & cl.torevset(notbackedup)
    backedup = repo.revs("not public() and ::%ln", heads)
    return smartset.filteredset(
        subset & repo.revs("not public() - hidden()"), lambda r: r not in backedup
    )


@templatekeyword("backingup")
def backingup(repo, **args):
    """whether commit cloud is currently backing up commits."""
    # If the backup lock exists then a backup should be in progress.
    return backuplock.islocked(repo)
