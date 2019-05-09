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

    # Migrate repository from older pushbackup system to Commit Cloud Sync
    autocloudjoin = True
"""

from __future__ import absolute_import

from edenscm.mercurial import (
    error,
    extensions,
    hintutil,
    localrepo,
    node as nodemod,
    registrar,
    revset,
    smartset,
    util,
)
from edenscm.mercurial.i18n import _

from . import (
    commitcloudcommands,
    commitcloudcommon,
    commitcloudutil,
    dependencies,
    syncstate,
    workspace,
)


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
configitem("commitcloud", "backuplimitnocheck", default=4)
configitem("commitcloud", "autocloudjoin", default=False)


@hint("commitcloud-old-commits")
def _smartlogomittedcommitsmsg(repo):
    workspacename = workspace.currentworkspace(repo)
    lastsyncstate = syncstate.SyncState(repo, workspacename)
    if lastsyncstate.omittedheads or lastsyncstate.omittedbookmarks:
        return _(
            "some older commits or bookmarks have not been synced to this repo\n"
            "(run `hg cloud sl` to see all of the commits in your workspace)\n"
            "(run `hg pull -r HASH` to fetch commits by hash)\n"
            "(run `hg cloud sync --full` to fetch everything - this may be slow)\n"
        )


def extsetup(ui):
    dependencies.extsetup(ui)

    localrepo.localrepository._wlockfreeprefix.add(commitcloudutil._obsmarkerssyncing)
    localrepo.localrepository._wlockfreeprefix.add(commitcloudutil._syncprogress)
    localrepo.localrepository._lockfreeprefix.add(syncstate.SyncState.prefix)


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
        hexnodespulled = missingcloudrevspull(
            repo, [nodemod.bin(nodehex) for nodehex in args]
        )
        return subset & repo.unfiltered().revs("%ls", hexnodespulled)
    except Exception as e:
        commitcloudcommon.highlightstatus(
            repo.ui, _("unable to pull all changesets from the remote store\n%s\n") % e
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
        pullcmd, pullopts = commitcloudutil.getcommandandoptions("^pull")
        pullopts["rev"] = [nodemod.hex(node) for node in nodes]
        pullcmd(repo.ui, unfi, **pullopts)

    return nodes
