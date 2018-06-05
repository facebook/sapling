# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import errno
import itertools
import re
import socket
import time

from mercurial import (
    commands,
    discovery,
    error,
    hg,
    hintutil,
    lock as lockmod,
    node,
    obsutil,
    registrar,
)

# Mercurial
from mercurial.i18n import _

from . import commitcloudcommon, commitcloudutil, service, state
from .. import shareutil


cmdtable = {}
command = registrar.command(cmdtable)
highlightdebug = commitcloudcommon.highlightdebug
highlightstatus = commitcloudcommon.highlightstatus
infinitepush = None
infinitepushbackup = None

# This must match the name from infinitepushbackup in order to maintain
# mutual exclusivity with infinitepushbackups.
_backuplockname = "infinitepushbackup.lock"


@command("cloud", [], "SUBCOMMAND ...", subonly=True)
def cloud(ui, repo, **opts):
    """synchronise commits via commit cloud

    Commit cloud lets you synchronize commits and bookmarks between
    different copies of the same repository.  This may be useful, for
    example, to keep your laptop and desktop computers in sync.

    Use 'hg cloud join' to connect your repository to the commit cloud
    service and begin synchronizing commits.

    Use 'hg cloud sync' to trigger a new synchronization.  Synchronizations
    also happen automatically in the background as you create and modify
    commits.

    Use 'hg cloud leave' to disconnect your repository from commit cloud.
    """
    pass


subcmd = cloud.subcommand()


@subcmd("join|connect", [])
def cloudjoin(ui, repo, **opts):
    """connect the local repository to commit cloud

    Commits and bookmarks will be synchronized between all repositories that
    have been connected to the service.

    Use `hg cloud sync` to trigger a new synchronization.
    """

    tokenlocator = commitcloudutil.TokenLocator(ui)
    checkauthenticated(ui, repo, tokenlocator)

    workspacemanager = commitcloudutil.WorkspaceManager(repo)
    workspacemanager.setworkspace()

    highlightstatus(
        ui,
        _(
            "this repository is now connected to the '%s' "
            "workspace for the '%s' repo\n"
        )
        % (workspacemanager.workspace, workspacemanager.reponame),
    )
    cloudsync(ui, repo, **opts)


@subcmd("leave|disconnect")
def cloudleave(ui, repo, **opts):
    """disconnect the local repository from commit cloud

    Commits and bookmarks will no londer be synchronized with other
    repositories.
    """
    # do no crash on run cloud leave multiple times
    if not commitcloudutil.getworkspacename(repo):
        highlightstatus(
            ui, _("this repository has been already disconnected from commit cloud\n")
        )
        return
    commitcloudutil.SubscriptionManager(repo).removesubscription()
    commitcloudutil.WorkspaceManager(repo).clearworkspace()
    highlightstatus(ui, _("this repository is now disconnected from commit cloud\n"))


@subcmd("authenticate", [("t", "token", "", _("set or update token"))])
def cloudauth(ui, repo, **opts):
    """authenticate this host with the commit cloud service
    """
    tokenlocator = commitcloudutil.TokenLocator(ui)

    token = opts.get("token")
    if token:
        # The user has provided a token, so just store it.
        if tokenlocator.token:
            ui.status(_("updating authentication token\n"))
        else:
            ui.status(_("setting authentication token\n"))
        # check token actually works
        service.get(ui, token).check()
        tokenlocator.settoken(token)
        ui.status(_("authentication successful\n"))
    else:
        token = tokenlocator.token
        if token:
            try:
                service.get(ui, token).check()
            except commitcloudcommon.RegistrationError:
                token = None
            else:
                ui.status(_("using existing authentication token\n"))
        if token:
            ui.status(_("authentication successful\n"))
        else:
            # Run through interactive authentication
            authenticate(ui, repo, tokenlocator)


def authenticate(ui, repo, tokenlocator):
    """interactive authentication"""
    if not ui.interactive():
        msg = _("authentication with commit cloud required")
        hint = _("use 'hg cloud auth --token TOKEN' to set a token")
        raise commitcloudcommon.RegistrationError(ui, msg, hint=hint)

    authhelp = ui.config("commitcloud", "auth_help")
    if authhelp:
        ui.status(authhelp + "\n")
    # ui.prompt doesn't set up the prompt correctly, so pasting long lines
    # wraps incorrectly in the terminal.  Print the prompt on its own line
    # to avoid this.
    prompt = _("paste your commit cloud authentication token below:\n")
    ui.write(ui.label(prompt, "ui.prompt"))
    token = ui.prompt("", default="").strip()
    if token:
        service.get(ui, token).check()
        tokenlocator.settoken(token)
        ui.status(_("authentication successful\n"))


def checkauthenticated(ui, repo, tokenlocator):
    """check if authentication is needed"""
    token = tokenlocator.token
    if token:
        try:
            service.get(ui, token).check()
        except commitcloudcommon.RegistrationError:
            pass
        else:
            return
    authenticate(ui, repo, tokenlocator)


def getrepoworkspace(ui, repo):
    """get the workspace identity for the currently joined workspace"""
    workspacemanager = commitcloudutil.WorkspaceManager(repo)
    reponame = workspacemanager.reponame
    if not reponame:
        raise commitcloudcommon.ConfigurationError(ui, _("unknown repo"))

    workspace = workspacemanager.workspace
    if not workspace:
        raise commitcloudcommon.WorkspaceError(ui, _("undefined workspace"))

    return reponame, workspace


@subcmd(
    "sync",
    [
        (
            "",
            "workspace-version",
            "",
            _(
                "(EXPERIMENTAL) latest version "
                "(some external services can receive notifications and "
                "know the latest version)"
            ),
        ),
        (
            "",
            "check-autosync-enabled",
            None,
            _(
                "(EXPERIMENTAL) check that "
                "automatic synchronization is enabled "
                "(some external services can run `cloud sync` on behalf of the user)"
            ),
        ),
    ],
)
def cloudsync(ui, repo, **opts):
    """synchronize commits with the commit cloud service"""
    try:
        # Wait at most 30 seconds, because that's the average backup time
        timeout = 30
        srcrepo = shareutil.getsrcrepo(repo)
        with lockmod.lock(srcrepo.vfs, _backuplockname, timeout=timeout):
            currentnode = repo["."].node()
            _docloudsync(ui, repo, **opts)
            return _maybeupdateworkingcopy(ui, repo, currentnode)
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            ui.warn(_("timeout waiting on backup lock\n"))
            return 2
        else:
            raise


def _docloudsync(ui, repo, **opts):
    start = time.time()

    # external services can run cloud sync and require to check if
    # auto sync is enabled
    if opts.get("check_autosync_enabled") and not autosyncenabled(ui, repo):
        highlightstatus(
            ui, _("automatic backup and synchronization " "is currently disabled\n")
        )
        return 0

    tokenlocator = commitcloudutil.TokenLocator(ui)
    reponame, workspace = getrepoworkspace(ui, repo)
    serv = service.get(ui, tokenlocator.token)
    highlightstatus(ui, _("synchronizing '%s' with '%s'\n") % (reponame, workspace))

    lastsyncstate = state.SyncState(repo)

    # external services can run cloud sync and know the lasest version
    version = opts.get("workspace_version")
    if version and version.isdigit() and int(version) <= lastsyncstate.version:
        highlightstatus(ui, _("this version has been already synchronized\n"))
        return 0

    cloudrefs = serv.getreferences(reponame, workspace, lastsyncstate.version)

    synced = False
    pushfailures = set()
    while not synced:
        if cloudrefs.version != lastsyncstate.version:
            _applycloudchanges(ui, repo, lastsyncstate, cloudrefs)

        localheads = _getheads(repo)
        localbookmarks = _getbookmarks(repo)

        if (
            set(localheads) == set(lastsyncstate.heads)
            and localbookmarks == lastsyncstate.bookmarks
            and lastsyncstate.version != 0
        ):
            synced = True

        if not synced:
            # The local repo has changed.  We must send these changes to the
            # cloud.
            path = commitcloudutil.getremotepath(repo, ui, None)

            def getconnection():
                return repo.connectionpool.get(path, opts)

            # Push commits that the server doesn't have.
            newheads = list(set(localheads) - set(lastsyncstate.heads))
            newheads, failedheads = infinitepush.pushbackupbundlestacks(
                ui, repo, getconnection, newheads
            )

            if failedheads:
                pushfailures |= set(failedheads)
                # Some heads failed to be pushed.  Work out what is actually
                # available on the server
                unfi = repo.unfiltered()
                localheads = [
                    ctx.hex()
                    for ctx in unfi.set(
                        "heads((draft() & ::%ls) + (draft() & ::%ls & not obsolete()))",
                        newheads,
                        lastsyncstate.heads,
                    )
                ]
                failedcommits = {
                    ctx.hex()
                    for ctx in repo.set(
                        "(draft() & ::%ls) - (draft() & ::%ls)", failedheads, localheads
                    )
                }
                # Revert any bookmark updates that refer to failed commits to
                # the available commits.
                for name, bookmarknode in localbookmarks.items():
                    if bookmarknode in failedcommits:
                        if name in lastsyncstate.bookmarks:
                            localbookmarks[name] = lastsyncstate.bookmarks[name]
                        else:
                            del localbookmarks[name]

            # Update the infinitepush backup bookmarks to point to the new
            # local heads and bookmarks.  This must be done after all
            # referenced commits have been pushed to the server.
            pushbackupbookmarks(
                ui, repo, getconnection, localheads, localbookmarks, **opts
            )

            # Update the cloud heads, bookmarks and obsmarkers.
            obsmarkers = commitcloudutil.getsyncingobsmarkers(repo)
            synced, cloudrefs = serv.updatereferences(
                reponame,
                workspace,
                lastsyncstate.version,
                lastsyncstate.heads,
                localheads,
                lastsyncstate.bookmarks.keys(),
                localbookmarks,
                obsmarkers,
            )
            if synced:
                lastsyncstate.update(cloudrefs.version, localheads, localbookmarks)
                if obsmarkers:
                    commitcloudutil.clearsyncingobsmarkers(repo)

    elapsed = time.time() - start
    highlightdebug(ui, _("cloudsync completed in %0.2f sec\n") % elapsed)
    if pushfailures:
        raise commitcloudcommon.SynchronizationError(
            ui, _("%d heads could not be pushed") % len(pushfailures)
        )
    highlightstatus(ui, _("commits synchronized\n"))
    # check that Scm Service is running and a subscription exists
    commitcloudutil.SubscriptionManager(repo).checksubscription()


def _maybeupdateworkingcopy(ui, repo, currentnode):
    if repo["."].node() != currentnode:
        return 0

    destination = finddestinationnode(repo, currentnode)

    if destination == currentnode:
        return 0

    if destination and destination in repo:
        highlightstatus(
            ui,
            _("current revision %s has been moved remotely to %s\n")
            % (node.short(currentnode), node.short(destination)),
        )
        if ui.configbool("commitcloud", "updateonmove"):
            return _update(ui, repo, destination)
        else:
            hintutil.trigger("commitcloud-update-on-move")
            return 0
    else:
        highlightstatus(
            ui,
            _(
                "current revision %s has been replaced remotely "
                "with multiple revisions\n"
                "Please run `hg update` to go to the desired revision\n"
            )
            % node.short(currentnode),
        )
        return 0


@subcmd("recover")
def cloudrecover(ui, repo, **opts):
    """perform recovery for commit cloud

    Clear the local cache of commit cloud service state, and resynchronize
    the repository from scratch.
    """
    highlightstatus(ui, "clearing local commit cloud cache\n")
    state.SyncState.erasestate(repo)
    cloudsync(ui, repo, **opts)


def _applycloudchanges(ui, repo, lastsyncstate, cloudrefs):
    pullcmd, pullopts = _getcommandandoptions("^pull")

    # Pull all the new heads
    # so we need to filter cloudrefs before pull
    # pull does't check if a rev is present locally
    unfi = repo.unfiltered()
    newheads = filter(lambda rev: rev not in unfi, cloudrefs.heads)
    if newheads:
        pullopts["rev"] = newheads
        pullcmd(ui, repo, **pullopts)

    # Merge cloud bookmarks into the repo
    _mergebookmarks(ui, repo, cloudrefs.bookmarks, lastsyncstate.bookmarks)

    # Merge obsmarkers
    _mergeobsmarkers(ui, repo, cloudrefs.obsmarkers)

    # We have now synced the repo to the cloud version.  Store this.
    lastsyncstate.update(cloudrefs.version, cloudrefs.heads, cloudrefs.bookmarks)

    # Also update infinitepush state.  These new heads are already backed up,
    # otherwise the server wouldn't have told us about them.
    recordbackup(ui, repo, cloudrefs.heads)


def _update(ui, repo, destination):
    # update to new head with merging local uncommited changes
    ui.status(_("updating to %s\n") % node.short(destination))
    updatecheck = "none"
    return hg.updatetotally(ui, repo, destination, destination, updatecheck=updatecheck)


def _mergebookmarks(ui, repo, cloudbookmarks, lastsyncbookmarks):
    localbookmarks = _getbookmarks(repo)
    with repo.wlock(), repo.lock(), repo.transaction("bookmark") as tr:
        changes = []
        allnames = set(localbookmarks.keys() + cloudbookmarks.keys())
        newnames = set()
        for name in allnames:
            localnode = localbookmarks.get(name)
            cloudnode = cloudbookmarks.get(name)
            lastnode = lastsyncbookmarks.get(name)
            if cloudnode != localnode:
                if (
                    localnode is not None
                    and cloudnode is not None
                    and localnode != lastnode
                    and cloudnode != lastnode
                ):
                    # Changed both locally and remotely, fork the local
                    # bookmark
                    forkname = _forkname(ui, name, allnames | newnames)
                    newnames.add(forkname)
                    changes.append((forkname, node.bin(localnode)))
                    ui.warn(
                        _(
                            "%s changed locally and remotely, "
                            "local bookmark renamed to %s\n"
                        )
                        % (name, forkname)
                    )

                if cloudnode != lastnode:
                    if cloudnode is not None:
                        if cloudnode in repo:
                            changes.append((name, node.bin(cloudnode)))
                        else:
                            ui.warn(
                                _("%s not found, " "not creating %s bookmark\n")
                                % (cloudnode, name)
                            )
                    else:
                        if localnode is not None and localnode != lastnode:
                            # Moved locally, deleted in the cloud, resurrect
                            # at the new location
                            pass
                        else:
                            changes.append((name, None))
        repo._bookmarks.applychanges(repo, tr, changes)


def _mergeobsmarkers(ui, repo, obsmarkers):
    with repo.wlock(), repo.lock(), repo.transaction("commitcloud-obs") as tr:
        tr._commitcloudskippendingobsmarkers = True
        repo.obsstore.add(tr, obsmarkers)


def _forkname(ui, name, othernames):
    hostname = ui.config("commitcloud", "hostname", socket.gethostname())

    # Strip off any old suffix.
    m = re.match("-%s(-[0-9]*)?$" % re.escape(hostname), name)
    if m:
        suffix = "-%s%s" % (hostname, m.group(1) or "")
        name = name[0 : -len(suffix)]

    # Find a new name.
    for n in itertools.count():
        candidate = "%s-%s%s" % (name, hostname, "-%s" % n if n != 0 else "")
        if candidate not in othernames:
            return candidate


def _getheads(repo):
    headsrevset = repo.set("heads(draft()) & not obsolete()")
    return [ctx.hex() for ctx in headsrevset]


def _getbookmarks(repo):
    return {n: node.hex(v) for n, v in repo._bookmarks.items()}


def _getcommandandoptions(command):
    cmd = commands.table[command][0]
    opts = dict(opt[1:3] for opt in commands.table[command][1])
    return cmd, opts


def getsuccessorsnodes(repo, node):
    successors = repo.obsstore.successors.get(node, ())
    for successor in successors:
        m = obsutil.marker(repo, successor)
        for snode in m.succnodes():
            if snode and snode != node:
                yield snode


def finddestinationnode(repo, node):
    nodes = list(getsuccessorsnodes(repo, node))
    if len(nodes) == 1:
        return finddestinationnode(repo, nodes[0])
    if len(nodes) == 0:
        return node
    return None


def pushbackupbookmarks(ui, repo, getconnection, localheads, localbookmarks, **opts):
    """
    Push a backup bundle to the server that updates the infinitepush backup
    bookmarks.

    This keeps the old infinitepush backup bookmarks in sync, which means
    pullbackup still works for users using commit cloud sync.
    """
    # Build a dictionary of infinitepush bookmarks.  We delete
    # all bookmarks and replace them with the full set each time.
    if infinitepushbackup:
        infinitepushbookmarks = {}
        namingmgr = infinitepushbackup.BackupBookmarkNamingManager(
            ui, repo, opts.get("user")
        )
        infinitepushbookmarks[namingmgr.getbackupheadprefix()] = ""
        infinitepushbookmarks[namingmgr.getbackupbookmarkprefix()] = ""
        for bookmark, hexnode in localbookmarks.items():
            name = namingmgr.getbackupbookmarkname(bookmark)
            infinitepushbookmarks[name] = hexnode
        for hexhead in localheads:
            name = namingmgr.getbackupheadname(hexhead)
            infinitepushbookmarks[name] = hexhead

        # Push a bundle containing the new bookmarks to the server.
        with getconnection() as conn:
            infinitepush.pushbackupbundle(
                ui, repo, conn.peer, None, infinitepushbookmarks
            )

        # Update the infinitepush local state.
        srcrepo = shareutil.getsrcrepo(repo)
        infinitepushbackup._writelocalbackupstate(
            srcrepo.vfs, list(localheads), localbookmarks
        )


def recordbackup(ui, repo, newheads):
    """Record that the given heads are already backed up."""
    if infinitepushbackup is None:
        return

    backupstate = infinitepushbackup._readlocalbackupstate(ui, repo)
    backupheads = set(backupstate.heads) | set(newheads)
    srcrepo = shareutil.getsrcrepo(repo)
    infinitepushbackup._writelocalbackupstate(
        srcrepo.vfs, list(backupheads), backupstate.localbookmarks
    )


def autosyncenabled(ui, _repo):
    return infinitepushbackup is not None and infinitepushbackup.autobackupenabled(ui)
