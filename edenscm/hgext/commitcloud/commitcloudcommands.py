# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno

from edenscm.mercurial import (
    cmdutil,
    error,
    graphmod,
    lock as lockmod,
    node as nodemod,
    progress,
    registrar,
    scmutil,
)
from edenscm.mercurial.i18n import _

from . import (
    commitcloudcommon,
    commitcloudutil,
    dependencies,
    service,
    sync,
    syncstate,
    workspace,
)


cmdtable = {}
command = registrar.command(cmdtable)

pullopts = [
    (
        "",
        "full",
        None,
        _(
            "pull all workspace commits into the local repository, don't omit old ones. (ADVANCED)"
        ),
    )
]

pushopts = []


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


subcmd = cloud.subcommand(
    categories=[
        ("Connect to a cloud workspace", ["authenticate", "join"]),
        ("Synchronize with the cloud workspace", ["sync"]),
        ("View other cloud workspaces", ["sl", "ssl"]),
    ]
)


@subcmd("join|connect", [] + workspace.workspaceopts + pullopts + pushopts)
def cloudjoin(ui, repo, **opts):
    """connect the local repository to commit cloud

    Commits and bookmarks will be synchronized between all repositories that
    have been connected to the service.

    Use `hg cloud sync` to trigger a new synchronization.
    """

    tokenlocator = commitcloudutil.TokenLocator(ui)
    checkauthenticated(ui, repo, tokenlocator)

    workspacename = workspace.parseworkspace(ui, repo, **opts)
    if workspacename is None:
        workspacename = workspace.defaultworkspace(ui)
    if workspace.currentworkspace(repo):
        commitcloudutil.SubscriptionManager(repo).removesubscription()
    workspace.setworkspace(repo, workspacename)

    ui.status(
        _("this repository is now connected to the '%s' workspace for the '%s' repo\n")
        % (workspacename, commitcloudutil.getreponame(repo)),
        component="commitcloud",
    )
    cloudsync(ui, repo, **opts)


@subcmd("rejoin|reconnect", [] + workspace.workspaceopts + pullopts + pushopts)
def cloudrejoin(ui, repo, **opts):
    """reconnect the local repository to commit cloud

    Reconnect only happens if the machine has been registered with Commit Cloud,
    and the workspace has been already used for this repo

    Use `hg cloud sync` to trigger a new synchronization.

    Use `hg cloud connect` to connect to commit cloud for the first time.
    """

    educationpage = ui.config("commitcloud", "education_page")
    token = commitcloudutil.TokenLocator(ui).token
    if token:
        try:
            serv = service.get(ui, token)
            serv.check()
            reponame = commitcloudutil.getreponame(repo)
            workspacename = workspace.parseworkspace(ui, repo, **opts)
            if workspacename is None:
                workspacename = workspace.currentworkspace(repo)
            if workspacename is None:
                workspacename = workspace.defaultworkspace(ui)
            ui.status(
                _("trying to reconnect to the '%s' workspace for the '%s' repo\n")
                % (workspacename, reponame),
                component="commitcloud",
            )
            cloudrefs = serv.getreferences(reponame, workspacename, 0)
            if cloudrefs.version == 0:
                ui.status(
                    _(
                        "unable to reconnect: this workspace has been never connected to Commit Cloud for this repo\n"
                    ),
                    component="commitcloud",
                )
                if educationpage:
                    ui.status(
                        _("learn more about Commit Cloud at %s\n") % educationpage
                    )
            else:
                workspace.setworkspace(repo, workspacename)
                ui.status(
                    _("the repository is now reconnected\n"), component="commitcloud"
                )
                cloudsync(ui, repo, cloudrefs=cloudrefs, **opts)
            return
        except commitcloudcommon.RegistrationError:
            pass

    ui.status(
        _("unable to reconnect: not authenticated with Commit Cloud on this host\n"),
        component="commitcloud",
    )
    if educationpage:
        ui.status(_("learn more about Commit Cloud at %s\n") % educationpage)


@subcmd("leave|disconnect")
def cloudleave(ui, repo, **opts):
    """disconnect the local repository from commit cloud

    Commits and bookmarks will no londer be synchronized with other
    repositories.
    """
    # do no crash on run cloud leave multiple times
    if not workspace.currentworkspace(repo):
        ui.status(
            _("this repository has been already disconnected from commit cloud\n"),
            component="commitcloud",
        )
        return
    commitcloudutil.SubscriptionManager(repo).removesubscription()
    workspace.clearworkspace(repo)
    ui.status(
        _("this repository is now disconnected from commit cloud\n"),
        component="commitcloud",
    )


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


@subcmd("smartlog|sl", workspace.workspaceopts)
def cloudsmartlog(ui, repo, template="sl_cloud", **opts):
    """get smartlog view for the default workspace of the given user

    If the requested template is not defined in the config
    the command provides a simple view as a list of draft commits.
    """

    reponame = commitcloudutil.getreponame(repo)
    workspacename = workspace.parseworkspace(ui, repo, **opts)
    if workspacename is None:
        workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        workspacename = workspace.defaultworkspace(ui)

    ui.status(
        _("searching draft commits for the '%s' workspace for the '%s' repo\n")
        % (workspacename, reponame),
        component="commitcloud",
    )

    serv = service.get(ui, commitcloudutil.TokenLocator(ui).token)

    with progress.spinner(ui, _("fetching")):
        revdag = serv.getsmartlog(reponame, workspacename, repo)

    ui.status(_("Smartlog:\n\n"))

    # set up pager
    ui.pager("smartlog")

    smartlogstyle = ui.config("templatealias", template)
    # if style is defined in templatealias section of config apply that style
    if smartlogstyle:
        opts["template"] = "{%s}" % smartlogstyle
    else:
        ui.debug(
            _("style %s is not defined, skipping") % smartlogstyle,
            component="commitcloud",
        )

    # show all the nodes
    displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
    cmdutil.displaygraph(ui, repo, revdag, displayer, graphmod.asciiedges)


@subcmd("supersmartlog|ssl", workspace.workspaceopts)
def cloudsupersmartlog(ui, repo, **opts):
    """get super smartlog view for the given workspace"""
    cloudsmartlog(ui, repo, "ssl_cloud", **opts)


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


@subcmd("backup", [("r", "rev", [], _("revisions to back up"))], _("[-r] REV..."))
def cloudbackup(ui, repo, *revs, **opts):
    """backs up given commits if they are not already backed up

    Backs up working copy parent if no revision is provided.
    """
    if not revs:
        revs = ["."]

    nodes = [repo[r].hex() for r in scmutil.revrange(repo, revs)]

    remotepath = commitcloudutil.getremotepath(repo, ui, None)

    def getconnection():
        return repo.connectionpool.get(remotepath, opts)

    notbackedup = {
        node
        for node, backedup in zip(
            nodes, dependencies.infinitepush.isbackedupnodes(getconnection, nodes)
        )
        if not backedup
    }

    if notbackedup:
        backingup = list(notbackedup)
        sync._backingupsyncprogress(repo, backingup)
        repo.ui.status(_("pushing to %s\n") % remotepath)
        dependencies.infinitepush.pushbackupbundlestacks(
            repo.ui, repo, getconnection, backingup
        )
        sync.recordbackup(repo.ui, repo, remotepath, backingup)

        commitcloudutil.writesyncprogress(repo)
    else:
        repo.ui.write(_("nothing to back up\n"))


@subcmd(
    "sync",
    [
        (
            "",
            "workspace-version",
            "",
            _(
                "target workspace version to sync to "
                "(skip `cloud sync` if the current version is greater or equal than the given one) (EXPERIMENTAL)"
            ),
        ),
        (
            "",
            "check-autosync-enabled",
            None,
            _(
                "check automatic synchronization settings "
                "(skip `cloud sync` if automatic synchronization is disabled) (EXPERIMENTAL)"
            ),
        ),
        (
            "",
            "use-bgssh",
            None,
            _(
                "try to use the password-less login for ssh if defined in the cconfig "
                "(option requires infinitepush.bgssh config) (EXPERIMENTAL)"
            ),
        ),
    ]
    + pullopts
    + pushopts,
)
def cloudsync(ui, repo, cloudrefs=None, **opts):
    """synchronize commits with the commit cloud service
    """
    # external services can run cloud sync and require to check if
    # auto sync is enabled
    if opts.get("check_autosync_enabled") and (
        dependencies.infinitepushbackup is None
        or not dependencies.infinitepushbackup.autobackupenabled(ui)
    ):
        ui.status(
            _("automatic backup and synchronization is currently disabled\n"),
            component="commitcloud",
        )
        return 0

    repo.ignoreautobackup = True
    if opts.get("use_bgssh"):
        bgssh = ui.config("infinitepush", "bgssh")
        if bgssh:
            ui.setconfig("ui", "ssh", bgssh)

    lock = None
    # check if the background sync is running and provide all the details
    if ui.interactive():
        try:
            lock = lockmod.lock(repo.sharedvfs, commitcloudcommon.backuplockname, 0)
        except error.LockHeld as e:
            if e.errno == errno.ETIMEDOUT and e.lockinfo.isrunning():
                etimemsg = ""
                etime = commitcloudutil.getprocessetime(e.lockinfo)
                if etime:
                    etimemsg = _(", running for %d min %d sec") % divmod(etime, 60)
                ui.status(
                    _("background cloud sync is already in progress (pid %s on %s%s)\n")
                    % (e.lockinfo.uniqueid, e.lockinfo.namespace, etimemsg),
                    component="commitcloud",
                )
                ui.flush()

    # run cloud sync with waiting for background process to complete
    try:
        # wait at most 120 seconds, because cloud sync can take a while
        timeout = 120
        with lock or lockmod.lock(
            repo.sharedvfs,
            commitcloudcommon.backuplockname,
            timeout=timeout,
            ui=ui,
            showspinner=True,
            spinnermsg=_("waiting for background process to complete"),
        ):
            currentnode = repo["."].node()
            sync.docloudsync(ui, repo, cloudrefs, **opts)
            ret = sync.maybeupdateworkingcopy(ui, repo, currentnode)
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            ui.warn(_("timeout waiting %d sec on backup lock expired\n") % timeout)
            return 2
        else:
            raise

    if dependencies.infinitepushbackup:
        dependencies.infinitepushbackup._dobackgroundbackupother(
            ui, repo, command=["hg", "pushbackup"], **opts
        )
    return ret


@subcmd("recover", [] + pullopts + pushopts)
def cloudrecover(ui, repo, **opts):
    """perform recovery for commit cloud

    Clear the local cache of commit cloud service state, and resynchronize
    the repository from scratch.
    """
    ui.status(_("clearing local commit cloud cache\n"), component="commitcloud")
    workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        raise commitcloudcommon.WorkspaceError(ui, _("undefined workspace"))
    syncstate.SyncState.erasestate(repo, workspacename)
    cloudsync(ui, repo, **opts)
