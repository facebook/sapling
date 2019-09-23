# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import json
import re
import time

from edenscm.mercurial import (
    cmdutil,
    error,
    extensions,
    graphmod,
    lock as lockmod,
    node as nodemod,
    progress,
    registrar,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _, _n

from . import (
    background,
    backup,
    backupbookmarks,
    backuplock,
    backupstate,
    dependencies,
    error as ccerror,
    interactivehistory,
    service,
    subscription,
    sync,
    syncstate,
    token as tokenmod,
    util as ccutil,
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

remoteopts = [("", "dest", "", _("remote that is used for backups"))]


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
        ("Connect to a cloud workspace", ["authenticate", "join", "leave", "rejoin"]),
        ("Synchronize with the cloud workspace", ["sync"]),
        ("View other cloud workspaces", ["sl", "ssl"]),
        (
            "Back up commits",
            ["backup", "check", "listbackups", "restorebackup", "deletebackup"],
        ),
        ("Manage automatic backup or sync", ["disable", "enable"]),
    ]
)


@subcmd("join|connect", [] + workspace.workspaceopts + pullopts + remoteopts)
def cloudjoin(ui, repo, **opts):
    """connect the local repository to commit cloud

    Commits and bookmarks will be synchronized between all repositories that
    have been connected to the service.

    Use `hg cloud sync` to trigger a new synchronization.
    """

    tokenlocator = tokenmod.TokenLocator(ui)
    checkauthenticated(ui, repo, tokenlocator)

    workspacename = workspace.parseworkspace(ui, opts)
    if workspacename is None:
        workspacename = workspace.defaultworkspace(ui)
    if workspace.currentworkspace(repo):
        subscription.remove(repo)
    workspace.setworkspace(repo, workspacename)

    ui.status(
        _("this repository is now connected to the '%s' workspace for the '%s' repo\n")
        % (workspacename, ccutil.getreponame(repo)),
        component="commitcloud",
    )
    cloudsync(ui, repo, **opts)


@subcmd("rejoin|reconnect", [] + workspace.workspaceopts + pullopts + remoteopts)
def cloudrejoin(ui, repo, **opts):
    """reconnect the local repository to commit cloud

    If the local repository is not connected to commit cloud, attempt to connect
    it.  If the repository cannot be connected, then display a message
    describing how to connect to commit cloud.

    If connection is successful, then commits and bookmarks will be synchronized
    between all repositories that have been connected to the service.

    Use `hg cloud sync` to trigger a new synchronization.
    """
    if workspace.currentworkspace(repo):
        return

    workspacename = workspace.parseworkspace(ui, opts)
    if workspacename is None:
        workspacename = workspace.defaultworkspace(ui)
    ui.status(
        _("attempting to connect to the '%s' workspace for the '%s' repo\n")
        % (workspacename, ccutil.getreponame(repo)),
        component="commitcloud",
    )
    try:
        cloudjoin(ui, repo, **opts)
    except ccerror.RegistrationError:
        ui.status(
            _("unable to connect: not authenticated with Commit Cloud on this host\n"),
            component="commitcloud",
        )
        educationpage = ui.config("commitcloud", "education_page")
        if educationpage:
            ui.status(_("learn more about Commit Cloud at %s\n") % educationpage)


@subcmd("leave|disconnect")
def cloudleave(ui, repo, **opts):
    """disconnect the local repository from commit cloud

    Commits and bookmarks will no longer be synchronized with other
    repositories.
    """
    oldworkspacename = workspace.currentworkspace(repo)
    subscription.remove(repo)
    workspace.clearworkspace(repo)
    if oldworkspacename:
        ui.status(
            _("this repository is now disconnected from Commit Cloud Sync\n"),
            component="commitcloud",
        )
    else:
        ui.status(
            _("this repository is not connected to Commit Cloud Sync\n"),
            component="commitcloud",
        )


@subcmd("authenticate", [("t", "token", "", _("set or update token"))])
def cloudauth(ui, repo, **opts):
    """authenticate this host with the commit cloud service
    """
    tokenlocator = tokenmod.TokenLocator(ui)

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
            except ccerror.RegistrationError:
                token = None
            else:
                ui.status(_("using existing authentication token\n"))
        if token:
            ui.status(_("authentication successful\n"))
        else:
            # Run through interactive authentication
            authenticate(ui, repo, tokenlocator)


@subcmd(
    "smartlog|sl",
    [
        (
            "d",
            "date",
            "",
            _("show version of the smartlog on date specified"),
            _("DATE"),
        ),
        (
            "",
            "workspace-version",
            "",
            "show the specified version of the smartlog",
            _("NUM"),
        ),
        (
            "H",
            "history",
            None,
            "show interactive view for historical versions of smartlog",
        ),
        ("", "all", None, "show all history in interactive history view"),
    ]
    + workspace.workspaceopts,
)
def cloudsmartlog(ui, repo, template="sl_cloud", **opts):
    """get smartlog view for the default workspace of the given user

    If the requested template is not defined in the config
    the command provides a simple view as a list of draft commits.
    """

    if opts.get("history"):
        interactivehistory.showhistory(ui, repo, **opts)
        return

    reponame = ccutil.getreponame(repo)
    workspacename = workspace.parseworkspace(ui, opts)
    if workspacename is None:
        workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        workspacename = workspace.defaultworkspace(ui)
    date = opts.get("date")
    version = opts.get("workspace_version")
    if date:
        parseddate = util.parsedate(date)
    else:
        parseddate = None

    ui.status(
        _("searching draft commits for the '%s' workspace for the '%s' repo\n")
        % (workspacename, reponame),
        component="commitcloud",
    )
    serv = service.get(ui, tokenmod.TokenLocator(ui).token)
    if parseddate is None and not version:
        with progress.spinner(ui, _("fetching")):
            revdag = serv.getsmartlog(reponame, workspacename, repo, 0)
    else:
        with progress.spinner(ui, _("fetching")):
            revdag, slversion, sltimestamp = serv.getsmartlogbyversion(
                reponame, workspacename, repo, parseddate, version, 0
            )
    if parseddate or version:
        formatteddate = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(sltimestamp))
        ui.status(
            _("Smartlog version %d \nsynced at %s\n\n") % (slversion, formatteddate)
        )
    else:
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
        raise ccerror.RegistrationError(ui, msg, hint=hint)

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
        except ccerror.RegistrationError:
            pass
        else:
            return
    authenticate(ui, repo, tokenlocator)


@subcmd(
    "backup",
    [
        ("r", "rev", [], _("revisions to back up")),
        ("", "background", None, "run backup in background"),
    ]
    + remoteopts,
    _("[-r REV...]"),
)
def cloudbackup(ui, repo, *revs, **opts):
    """back up commits to commit cloud

    Commits that have already been backed up will be skipped.

    If no revision is specified, backs up all visible commits.
    """
    inbackground = opts.get("background")
    revs = revs + tuple(opts.get("rev", ()))
    if revs:
        if inbackground:
            raise error.Abort("'--background' cannot be used with specific revisions")
        revs = scmutil.revrange(repo, revs)
    else:
        revs = None

    dest = opts.get("dest")

    if inbackground:
        background.backgroundbackup(repo, dest=dest)
        return 0

    backupsnapshots = False
    try:
        extensions.find("snapshot")
        backupsnapshots = True
    except KeyError:
        pass

    remotepath = ccutil.getremotepath(repo, dest)
    getconnection = lambda: repo.connectionpool.get(remotepath, opts)

    with backuplock.lock(repo):
        # Load the backup state under the repo lock to ensure a consistent view.
        with repo.lock():
            state = backupstate.BackupState(repo, remotepath)
        backedup, failed = backup.backup(
            repo,
            state,
            remotepath,
            getconnection,
            revs,
            backupsnapshots=backupsnapshots,
        )

        if revs is None:
            # For a full backup, also update the backup bookmarks.
            backupbookmarks.pushbackupbookmarks(repo, remotepath, getconnection, state)

    if backedup:
        repo.ui.status(
            _n("backed up %d commit\n", "backed up %d commits\n", len(backedup))
            % len(backedup),
            component="commitcloud",
        )
    if failed:
        repo.ui.warn(
            _n(
                "failed to back up %d commit\n",
                "failed to back up %d commits\n",
                len(failed),
            )
            % len(failed),
            component="commitcloud",
        )
    if not backedup and not failed:
        repo.ui.status(_("nothing to back up\n"))
    return 0 if not failed else 2


@subcmd(
    "listbackups",
    [
        ("a", "all", None, _("list all backups, not just the most recent")),
        ("", "user", "", _("username, defaults to current user")),
        ("", "json", None, _("print available backups in json format")),
    ],
)
def cloudlistbackups(ui, repo, dest=None, **opts):
    """list backups that are available on the server"""

    remotepath = ccutil.getremotepath(repo, dest)
    getconnection = lambda: repo.connectionpool.get(remotepath, opts)

    sourceusername = opts.get("user")
    if not sourceusername:
        sourceusername = util.shortuser(repo.ui.username())
    backupinfo = backupbookmarks.downloadbackupbookmarks(
        repo, remotepath, getconnection, sourceusername
    )

    if opts.get("json"):
        jsondict = util.sortdict()
        for hostname, reporoot in backupinfo.keys():
            jsondict.setdefault(hostname, []).append(reporoot)
        ui.write("%s\n" % json.dumps(jsondict, indent=4))
    elif not backupinfo:
        ui.write(_("no backups available for %s\n") % sourceusername)
    else:
        backupbookmarks.printbackupbookmarks(
            ui, sourceusername, backupinfo, all=bool(opts.get("all"))
        )


@subcmd(
    "restorebackup",
    [
        ("", "reporoot", "", "root of the repo to restore"),
        ("", "user", "", "user who ran the backup"),
        ("", "hostname", "", "hostname of the repo to restore"),
    ]
    + remoteopts,
)
def cloudrestorebackup(ui, repo, dest=None, **opts):
    """restore commits that were previously backed up with 'hg cloud backup'

    If you have only one backup for the repo on the backup server then it will be restored.

    If you have backed up multiple clones of the same repo, then the
    '--reporoot', '--hostname' and '--user' options may be used to disambiguate
    which backup to restore.

    Use 'hg cloud listbackups' to list available backups.
    """

    remotepath = ccutil.getremotepath(repo, dest)
    getconnection = lambda: repo.connectionpool.get(remotepath, opts)

    sourceusername = opts.get("user")
    if not sourceusername:
        sourceusername = util.shortuser(repo.ui.username())
    sourcereporoot = opts.get("reporoot")
    sourcehostname = opts.get("hostname")
    backupinfo = backupbookmarks.downloadbackupbookmarks(
        repo, remotepath, getconnection, sourceusername, sourcehostname, sourcereporoot
    )

    if not backupinfo:
        ui.warn(_("no backups found!"))
        return 1
    if len(backupinfo) > 1:
        backupbookmarks.printbackupbookmarks(ui, sourceusername, backupinfo)
        raise error.Abort(
            _("multiple backups found"),
            hint=_("set --hostname and --reporoot to pick a backup"),
        )

    (restorehostname, restorereporoot), restorestate = backupinfo.popitem()
    repo.ui.status(
        _("restoring backup for %s from %s on %s\n")
        % (sourceusername, restorereporoot, restorehostname)
    )

    pullcmd, pullopts = ccutil.getcommandandoptions("^pull")
    # Pull the heads and the nodes that were pointed to by the bookmarks.
    # Note that we are avoiding the use of set() because we want to pull
    # revisions in the same order
    heads = restorestate.get("heads", [])
    bookmarks = restorestate.get("bookmarks", {})
    bookmarknodes = [hexnode for hexnode in bookmarks.values() if hexnode not in heads]
    pullopts["rev"] = heads + bookmarknodes
    if dest:
        pullopts["source"] = dest

    with backuplock.lock(repo), repo.wlock(), repo.lock(), repo.transaction(
        "backuprestore"
    ) as tr:

        maxrevbeforepull = len(repo.changelog)
        result = pullcmd(ui, repo, **pullopts)
        maxrevafterpull = len(repo.changelog)

        if ui.config("infinitepushbackup", "createlandedasmarkers", False):
            pullcreatemarkers = extensions.find("pullcreatemarkers")
            pullcreatemarkers.createmarkers(
                result, repo, maxrevbeforepull, maxrevafterpull, fromdrafts=False
            )

        changes = []
        for name, hexnode in bookmarks.iteritems():
            if hexnode in repo:
                changes.append((name, nodemod.bin(hexnode)))
            else:
                ui.warn(_("%s not found, not creating %s bookmark") % (hexnode, name))
        repo._bookmarks.applychanges(repo, tr, changes)

        # Update local backup state and flag to not autobackup just after we
        # restored, which would be pointless.
        state = backupstate.BackupState(repo, remotepath)
        state.update([nodemod.bin(hexnode) for hexnode in heads + bookmarknodes])
        backupbookmarks._writelocalbackupstate(
            repo, ccutil.getremotepath(repo, dest), heads, bookmarks
        )
        repo.ignoreautobackup = True

    return result


@subcmd(
    "deletebackup",
    [
        ("", "reporoot", "", "root of the repo to delete the backup for"),
        ("", "hostname", "", "hostname of the repo to delete the backup for"),
    ]
    + remoteopts,
)
def clouddeletebackup(ui, repo, dest=None, **opts):
    """delete a backup from the server

    Removes all heads and bookmarks associated with a backup from the server.
    The commits themselves are not removed, so you can still update to them
    using 'hg update HASH'.
    """

    remotepath = ccutil.getremotepath(repo, dest)
    getconnection = lambda: repo.connectionpool.get(remotepath, opts)

    sourceusername = util.shortuser(repo.ui.username())
    sourcereporoot = opts.get("reporoot")
    sourcehostname = opts.get("hostname")
    if not sourcereporoot or not sourcehostname:
        msg = _("you must specify a reporoot and hostname to delete a backup")
        hint = _("use 'hg cloud listbackups' to find which backups exist")
        raise error.Abort(msg, hint=hint)

    # Do some sanity checking on the names
    if not re.match(r"^[-a-zA-Z0-9._/]+$", sourcereporoot):
        msg = _("repo root contains unexpected characters")
        raise error.Abort(msg)
    if not re.match(r"^[-a-zA-Z0-9.]+$", sourcehostname):
        msg = _("hostname contains unexpected characters")
        raise error.Abort(msg)
    if (
        sourcereporoot == repo.sharedroot
        and sourcehostname == backupbookmarks.backuphostname(repo)
    ):
        ui.warn(_("this backup matches the current repo\n"), notice=_("warning"))

    backupinfo = backupbookmarks.downloadbackupbookmarks(
        repo, remotepath, getconnection, sourceusername
    )
    deletestate = backupinfo.get((sourcehostname, sourcereporoot))
    if deletestate is None:
        raise error.Abort(
            _("no backup found for %s on %s") % (sourcereporoot, sourcehostname)
        )
    ui.write(_("%s on %s:\n") % (sourcereporoot, sourcehostname))
    ui.write(_("    heads:\n"))
    for head in deletestate.get("heads", []):
        ui.write(("        %s\n") % head)
    ui.write(_("    bookmarks:\n"))
    for bookname, booknode in sorted(deletestate.get("bookmarks", {}).items()):
        ui.write(("        %-20s %s\n") % (bookname + ":", booknode))
    if ui.promptchoice(_("delete this backup (yn)? $$ &Yes $$ &No"), 1) == 0:
        ui.status(
            _("deleting backup for %s on %s\n") % (sourcereporoot, sourcehostname)
        )
        backupbookmarks.deletebackupbookmarks(
            repo,
            remotepath,
            getconnection,
            sourceusername,
            sourcehostname,
            sourcereporoot,
        )
        ui.status(_("backup deleted\n"))
        ui.status(_("(you can still access the commits directly using their hashes)\n"))
    return 0


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
                "try to use the password-less login for ssh if defined in the config "
                "(option requires infinitepush.bgssh config) (EXPERIMENTAL)"
            ),
        ),
    ]
    + pullopts
    + remoteopts,
)
def cloudsync(ui, repo, cloudrefs=None, dest=None, **opts):
    """synchronize commits with the commit cloud service
    """
    # external services can run cloud sync and require to check if
    # auto sync is enabled
    if opts.get("check_autosync_enabled") and not background.autobackupenabled(repo):
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

    full = opts.get("full")

    version = None
    versionstr = opts.get("workspace_version")
    if versionstr:
        try:
            version = int(versionstr)
        except ValueError:
            raise error.Abort(
                _("error: argument 'workspace-version' should be a number")
            )

    remotepath = ccutil.getremotepath(repo, dest)
    getconnection = lambda: repo.connectionpool.get(remotepath, opts)

    with backuplock.lock(repo):
        ret = sync.sync(repo, remotepath, getconnection, cloudrefs, full, version)
    background.backgroundbackupother(repo, dest=dest)
    return ret


@subcmd("recover", [] + pullopts + remoteopts)
def cloudrecover(ui, repo, **opts):
    """perform recovery for commit cloud

    Clear the local cache of commit cloud service state, and resynchronize
    the repository from scratch.
    """
    ui.status(_("clearing local commit cloud cache\n"), component="commitcloud")
    workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        raise ccerror.WorkspaceError(ui, _("undefined workspace"))
    syncstate.SyncState.erasestate(repo, workspacename)
    cloudsync(ui, repo, **opts)


@subcmd(
    "check|isbackedup",
    [
        ("r", "rev", [], _("show the specified revision or revset"), _("REV")),
        ("", "remote", None, _("check on the remote server")),
    ]
    + remoteopts,
)
def cloudcheck(ui, repo, dest=None, **opts):
    """check if commits have been backed up

    If no revision are specified then it checks working copy parent.
    """

    revs = opts.get("rev")
    remote = opts.get("remote")
    if not revs:
        revs = ["."]

    remotepath = ccutil.getremotepath(repo, dest)
    unfi = repo.unfiltered()
    revs = scmutil.revrange(repo, revs)
    nodestocheck = [repo[r].hex() for r in revs]

    if remote:
        getconnection = lambda: repo.connectionpool.get(remotepath, opts)
        isbackedup = {
            nodestocheck[i]: res
            for i, res in enumerate(
                dependencies.infinitepush.isbackedupnodes(getconnection, nodestocheck)
            )
        }
    else:
        state = backupstate.BackupState(repo, remotepath)
        backeduprevs = unfi.revs("not public() and ::%ln", state.heads)
        isbackedup = {node: unfi[node].rev() in backeduprevs for node in nodestocheck}

    for n in nodestocheck:
        ui.write((n + " "))
        ui.write(_("backed up") if isbackedup[n] else _("not backed up"))
        ui.write(_("\n"))


@subcmd("enable")
def cloudenable(ui, repo, **opts):
    """enable automatic backup or sync

    Enables backup or sync that has previously been disabled by ``hg cloud disable``.
    """

    if background.autobackupenabled(repo):
        ui.write(_("background backup is already enabled\n"))
        return 0

    background.disableautobackup(repo, None)

    if background.autobackupenabled(repo):
        ui.write(_("background backup is enabled\n"))
    else:
        ui.write(_("background backup is disabled by configuration\n"))
    return 0


@subcmd("disable", [("", "hours", "1", "duration to disable backup or sync for")])
def backupdisable(ui, repo, **opts):
    """temporarily disable automatic backup or sync

    Disables automatic background backup or sync for the specified duration.
    """

    if not background.autobackupenabled(repo):
        ui.write(_("background backup was already disabled\n"), notice=_("note"))

    try:
        duration = int(opts.get("hours", 1)) * 60 * 60
    except ValueError:
        raise error.Abort(
            _(
                "error: argument 'hours': invalid int value: '{value}'\n".format(
                    value=opts.get("hours")
                )
            )
        )

    timestamp = int(time.time()) + duration
    background.disableautobackup(repo, timestamp)
    ui.write(
        _("background backup is now disabled until %s\n")
        % util.datestr(util.makedate(timestamp)),
        component="commitcloud",
    )

    try:
        with backuplock.trylock(repo):
            pass
    except error.LockHeld as e:
        if e.lockinfo.isrunning():
            ui.warn(
                _(
                    "'@PROG@ cloud disable' does not affect running backup processes\n"
                    "(kill the background process - pid %s on %s - gracefully if needed)\n"
                )
                % (e.lockinfo.uniqueid, e.lockinfo.namespace),
                notice=_("warning"),
            )
    return 0


@subcmd("status")
def cloudstatus(ui, repo, **opts):
    """Shows information about the state of the user's workspace"""

    workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        ui.write(_("You are not connected to any workspace\n"))
        return

    autosync = "ON" if background.autobackupenabled(repo) else "OFF"
    currentsyncstate = syncstate.SyncState(repo, workspacename)
    syncupdatetime = time.ctime(currentsyncstate.lastupdatetime)
    if repo.localvfs.isfile("lastsync.log"):
        state = repo.localvfs.read("lastsync.log")
    else:
        state = "Not logged"
    ui.write(
        _(
            "Workspace: %s\n"
            "Automatic Sync: %s\n"
            "Last Sync: %s\n"
            "Last Sync State: %s\n"
        )
        % (workspacename, autosync, syncupdatetime, state)
    )


@command("debugwaitbackup", [("", "timeout", "", "timeout value")])
def waitbackup(ui, repo, timeout):
    """wait for backup operations to complete"""
    try:
        if timeout:
            timeout = int(timeout)
        else:
            timeout = -1
    except ValueError:
        raise error.Abort("timeout should be integer")

    try:
        with lockmod.lock(repo.sharedvfs, backuplock.lockfilename, timeout=timeout):
            pass
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            raise error.Abort(_("timeout while waiting for backup"))
        raise


@command(
    "pushbackup",
    [
        ("r", "rev", [], _("revisions to back up")),
        ("", "background", None, "run backup in background"),
    ]
    + remoteopts,
    _("[-r REV...]"),
)
def pushbackup(ui, repo, *revs, **opts):
    """back up commits to commit cloud (DEPRECATED)

    Commits that have already been backed up will be skipped.

    If no revision is specified, backs up all visible commits.

    'hg pushbackup' is deprecated in favour of 'hg cloud backup'.
    """
    return cloudbackup(ui, repo, *revs, **opts)


@command(
    "isbackedup",
    [
        ("r", "rev", [], _("show the specified revision or revset"), _("REV")),
        ("", "remote", None, _("check on the remote server")),
    ]
    + remoteopts,
)
def isbackedup(ui, repo, dest=None, **opts):
    """check if commits have been backed up (DEPRECATED)

    If no revision are specified then it checks working copy parent.

    'hg isbackedup' is deprecated in favour of 'hg cloud check'.
    """
    return cloudcheck(ui, repo, dest, **opts)
