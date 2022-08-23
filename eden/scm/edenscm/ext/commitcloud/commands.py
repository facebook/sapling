# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import errno
import time

from edenscm.mercurial import (
    bookmarks as bookmarksmod,
    cmdutil,
    edenapi_upload,
    error,
    hg,
    hintutil,
    lock as lockmod,
    node as nodemod,
    progress,
    pycompat,
    registrar,
    scmutil,
    templatefilters,
    util,
    visibility,
)
from edenscm.mercurial.i18n import _, _n

from . import (
    background,
    backup,
    backuplock,
    backupstate,
    dependencies,
    error as ccerror,
    interactivehistory,
    scmdaemon,
    service,
    subscription,
    sync,
    syncstate,
    token as tokenmod,
    upload,
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

createopts = [
    (
        "",
        "create",
        None,
        _(
            "create the workspace if it doesn't exist (applicable to all non default workspaces)"
        ),
    )
]

# The option could be useful if the current workspace is broken in some way
switchopt = [
    (
        "",
        "force",
        None,
        _(
            "discard local changes, do not sync the current workspace when switch to another one (ADVANCED)"
        ),
    )
]


@command("cloud", [], "SUBCOMMAND ...")
def cloud(ui, repo, **opts):
    """backup your commits and synchronise them via commit cloud

    Commit Cloud is the modern infrastructure for backing up your draft commits and bookmarks.

    Commit Cloud introduces a new abstraction: the commit cloud workspace.
    A workspace holds a set of draft commits and bookmarks.
    You can think of it as a backup of the contents of your smartlog in the cloud.
    You can have multiple workspaces (and so multiple smartlogs) and switch between them.

    Commit cloud lets you synchronize commits and bookmarks between
    different copies of the same repository if they are connected to the same commit cloud workspace.
    This may be useful, for example, to keep your laptop and desktop computers in sync.

    Use 'hg cloud join' to connect your repository to the default commit cloud workspace and get started.

    Use 'hg cloud sync' to trigger a new backup and synchronization. Backups and synchronizations
    also happen automatically in the background as you create and modify commits.

    Use 'hg cloud switch' to change which workspace you are connected to.
    Use 'hg cloud list' to see your workspaces.

    Use 'hg cloud leave' to stop using commit cloud workspaces.
    """
    raise error.Abort(
        "you need to specify a subcommand (run with --help to see a list of subcommands)"
    )


subcmd = cloud.subcommand(
    categories=[
        ("Connect to a cloud workspace", ["authenticate", "join", "switch", "leave"]),
        ("Synchronize with the connected cloud workspace", ["sync"]),
        (
            "Manage cloud workspaces",
            ["delete", "undelete", "list", "rename", "reclaim"],
        ),
        ("View the smartlog for a cloud workspace", ["sl", "ssl"]),
        (
            "Back up commits",
            ["backup", "check"],
        ),
        ("Manage automatic backup or sync", ["disable", "enable"]),
    ]
)


@subcmd(
    "join|connect",
    [
        ("", "switch", None, _("switch to another workspace")),
        ("", "merge", None, _("merge to another workspace")),
    ]
    + createopts
    + workspace.workspaceopts
    + pullopts
    + switchopt,
)
def cloudjoin(ui, repo, **opts):
    """connect the local repository to a commit cloud workspace ('default' workspace with no arguments)

    Local commits and bookmarks will be backed up to the commit cloud and
    synchronized between all repositories that have been connected
    to the same commit cloud workspace

    Use `hg cloud sync` to trigger a new backup and synchronization.
    """

    tokenlocator = tokenmod.TokenLocator(ui)
    checkauthenticated(ui, repo, tokenlocator)

    workspacename = workspace.parseworkspace(ui, opts)
    if workspacename is None:
        workspacename = workspace.defaultworkspace(ui)

    currentworkspace = workspace.currentworkspace(repo)

    switch = opts.get("switch")
    merge = opts.get("merge")
    create = opts.get("create")

    if switch and merge:
        ui.status(
            _(
                "'switch' and 'merge' options can not be provided together, please choose one over another\n"
            ),
            component="commitcloud",
        )
        return 1

    if currentworkspace == workspacename:
        ui.status(
            _(
                "this repository has been already connected to the '%s' workspace for the '%s' repo\n"
            )
            % (workspacename, ccutil.getreponame(repo)),
            component="commitcloud",
        )
        return cloudsync(ui, repo, **opts)

    # Check the current workspace and perform necessary clean up.
    # If the local repository is already connected to some workspace,
    # make sure that we perform correct merge or switch.
    # If the local repository is not connected yet to any workspace,
    # all local changes will be moved to the destination workspace (merge).
    if currentworkspace:
        if not switch and not merge:
            ui.status(
                _(
                    "this repository is already connected to the '%s' workspace, run `hg cloud join --help`\n"
                )
                % currentworkspace,
                component="commitcloud",
            )
            return 1

        serv = service.get(ui, tokenmod.TokenLocator(ui).token)
        reponame = ccutil.getreponame(repo)
        # check that the workspace exists if the destination workspace
        # doesn't equal to the default workspace for the current user
        if not create and workspace != workspace.defaultworkspace(ui):
            if not serv.getworkspaces(reponame, workspacename):
                raise error.Abort(
                    _(
                        "this repository can not be switched to the '%s' workspace\n"
                        "the workspace doesn't exist (please use --create option to create the workspace)"
                    )
                    % workspacename
                )

        if switch:
            # sync all the current commits and bookmarks before switching workspace
            if not opts.get("force"):
                try:
                    cloudsync(ui, repo, **opts)
                except ccerror.BadRequestError:
                    # the sync error can happen if the current workspace is missing on the server
                    # if it has been renamed or removed
                    if not serv.getworkspaces(reponame, currentworkspace):
                        raise error.Abort(
                            _(
                                "the current workspace '%s' has been renamed or removed, please use '--force' option to skip the sync step\n"
                                "note: using --force will discard the local view of commits but you can add commits back with `hg unhide`\n"
                            )
                            % currentworkspace
                        )
                    else:
                        raise
            ui.status(
                _(
                    "now this repository will be switched from the '%s' to the '%s' workspace\n"
                )
                % (currentworkspace, workspacename),
                component="commitcloud",
            )
            with backuplock.lock(repo), repo.wlock(), repo.lock(), repo.transaction(
                "commit cloud switch workspace clean up transaction"
            ) as tr:
                # check that the current location is a public commit
                if repo["."].mutable():
                    # * get the public root of the current commit
                    # * get the "main" bookmark that represents the main commit history
                    # * if the root is an ancestor of that bookmark, then update to it (the commit will be public in dst workspace)
                    currentnode = repo["."]
                    newnode = currentnode
                    while newnode.mutable():
                        newnode = newnode.p1()

                    publicroot = newnode
                    mainbookmark = bookmarksmod.mainbookmark(repo)
                    mainbookmarknode = repo[mainbookmark]
                    if repo.changelog.isancestor(
                        publicroot.node(), mainbookmarknode.node()
                    ):
                        # enforce the precondition that working directory must be clean
                        cmdutil.bailifchanged(repo)
                        hg.update(repo, newnode, False)
                        ui.status(
                            _("working directory now at %s\n")
                            % ui.label(str(publicroot), "node")
                        )
                    else:
                        raise error.Abort(
                            _(
                                "this repository can not be switched to the '%s' workspace\n"
                                "please update your location to a public commit first like `hg up %s`"
                            )
                            % (workspacename, mainbookmark)
                        )
                # remove heads and bookmarks before connecting to a new workspace
                visibility.setvisibleheads(repo, [])
                # remove all local bookmarks
                bmremove = []
                for key in sync._getbookmarks(repo).keys():
                    bmremove.append((key, None))
                repo._bookmarks.applychanges(repo, tr, bmremove)
                # remove all remote bookmarks (if sync of them enabled)
                bmremove = {
                    key: nodemod.nullhex
                    for key in sync._getremotebookmarks(repo).keys()
                }
                sync._updateremotebookmarks(repo, tr, bmremove)
                # erase state if the repo has been connected before to the destination workspace
                syncstate.SyncState.erasestate(repo, workspacename)
                # erase the state of the current workspace too
                syncstate.SyncState.erasestate(repo, currentworkspace)
                # clear subscription
                subscription.remove(repo)
                # clear workspace
                workspace.clearworkspace(repo)

        if merge:
            ui.status(
                _(
                    "this repository will be reconnected from the '%s' to the '%s' workspace\n"
                )
                % (currentworkspace, workspacename),
                component="commitcloud",
            )
            ui.status(
                _(
                    "all local commits and bookmarks will be merged into '%s' workspace\n"
                )
                % workspacename,
                component="commitcloud",
            )
            # TODO: suggest user to archive the old workspace if they want to
            # clear subscription
            subscription.remove(repo)
            # clear workspace
            workspace.clearworkspace(repo)
    else:
        if switch and not sync._iscleanrepo(repo):
            ui.status(
                _(
                    "this repository can not be switched to the '%s' workspace\n"
                    "the repository is not connected to any workspace yet and contains local commits or bookmarks\n"
                )
                % workspacename,
                component="commitcloud",
            )
            return 1

    # connect to a new workspace
    workspace.setworkspace(repo, workspacename)
    ui.status(
        _("this repository is now connected to the '%s' workspace for the '%s' repo\n")
        % (workspacename, ccutil.getreponame(repo)),
        component="commitcloud",
    )
    cloudsync(ui, repo, **opts)


@subcmd(
    "switch",
    [] + createopts + workspace.workspaceopts + pullopts + switchopt,
)
def switchworkspace(ui, repo, **opts):
    """switch the local repository to a different commit cloud workspace"""
    opts.update({"switch": True})
    cloudjoin(ui, repo, **opts)


@subcmd("rejoin|reconnect", [] + workspace.workspaceopts + pullopts)
def cloudrejoin(ui, repo, **opts):
    """reconnect the local repository to commit cloud

    If the local repository is not connected to commit cloud, attempt to connect
    it.  If the repository cannot be connected, then display a message
    describing how to connect to commit cloud.

    If connection is successful, then commits and bookmarks will be synchronized
    between all repositories that have been connected to the same commit cloud workspace.

    Use `hg cloud sync` to trigger a new synchronization.
    """
    if workspace.currentworkspace(repo):
        return

    active = []
    try:
        workspacename = workspace.parseworkspace(ui, opts)
        if workspacename is None:
            # If the workspace name is not given, figure out the sensible default.
            # The specific hostname workspace will be preferred over the default workspace.
            reponame = ccutil.getreponame(repo)
            hostnameworkspace = workspace.hostnameworkspace(ui)
            winfos = service.get(ui, tokenmod.TokenLocator(ui).token).getworkspaces(
                reponame, workspace.userworkspaceprefix(ui)
            )

            active = [winfo for winfo in winfos if not winfo.archived]

            if winfos and any([winfo.name == hostnameworkspace for winfo in active]):
                workspacename = hostnameworkspace
            else:
                workspacename = workspace.defaultworkspace(ui)

        ui.status(
            _("attempting to connect to the '%s' workspace for the '%s' repo\n")
            % (workspacename, ccutil.getreponame(repo)),
            component="commitcloud",
        )

        # update the raw_workspace option as workspacename has been already parsed
        for opt in workspace.workspaceopts:
            opts.pop(opt[1], None)
        opts.update({"raw_workspace": workspacename})
        cloudjoin(ui, repo, **opts)

    except ccerror.RegistrationError:
        ui.status(
            _("unable to connect: not authenticated with Commit Cloud on this host\n"),
            component="commitcloud",
        )
        educationpage = ui.config("commitcloud", "education_page")
        if educationpage:
            ui.status(_("learn more about Commit Cloud at %s\n") % educationpage)

    else:
        # provide a hint if several alternatives have been available
        if len(active) > 1:
            hintutil.trigger("commitcloud-switch", ui, active)


@subcmd("leave|disconnect")
def cloudleave(ui, repo, **opts):
    """disconnect the local repository from commit cloud

    Commits and bookmarks will no longer be synchronized with your Commit Cloud Workspace.
    """
    oldworkspacename = workspace.currentworkspace(repo)

    if not oldworkspacename:
        ui.status(
            _("this repository is not connected to any Commit Cloud Workspace\n"),
            component="commitcloud",
        )
        return

    confirmed = True

    if ui.interactive():
        ui.status(
            _(
                "you are about to leave Commit Cloud Sync, our infrastructure for backing up your draft commits and bookmarks\n"
                "this will make it harder to recover your work if you need to restore your commits on a new machine\n"
            ),
            component="commitcloud",
        )
        supportcontact = ui.config("ui", "supportcontact")
        if supportcontact:
            ui.status(
                _(
                    "help us to make your experience better by sharing your feedback with %s\n"
                )
                % supportcontact,
                component="commitcloud",
            )
        educationpage = ui.config("commitcloud", "education_page")
        if educationpage:
            ui.status(
                _(
                    "learn more about Commit Cloud Sync and Commit Cloud Workspaces at %s\n"
                )
                % educationpage,
                component="commitcloud",
            )
        prompt = _(
            "are you sure you want to disconnect the repo '%s' from the '%s' workspace [yn]:\n"
        ) % (ccutil.getreponame(repo), oldworkspacename)
        ui.write(ui.label(prompt, "ui.prompt"))
        confirmed = ui.prompt("", default="").strip().lower().startswith("y")

    if not confirmed:
        return

    subscription.remove(repo)
    workspace.clearworkspace(repo)
    ui.status(
        _("this repository is now disconnected from the '%s' workspace\n")
        % oldworkspacename,
        component="commitcloud",
    )


@subcmd("authenticate|auth", [("t", "token", "", _("set or update token"))])
def cloudauth(ui, repo, **opts):
    """authenticate this host with the commit cloud service and validate the authentication

    Token may not be required by the configuration but it is still possible to set it with -t option.
    Commit Cloud token may still be required for SCM Daemon to authenticate.
    """
    tokenlocator = tokenmod.TokenLocator(ui)

    token = opts.get("token")
    if token:
        if tokenlocator.tokenenforced and tokenlocator.token:
            ui.status(_("updating authentication token\n"))
        else:
            ui.status(_("setting authentication token\n"))

        if tokenlocator.tokenenforced:
            # check authentication
            service.get(ui, token).check()
            ui.status(_("token has been validated\n"))
            tokenlocator.settoken(token)
            ui.status(_("authentication successful\n"))
        else:
            ui.status(
                _("token will be set but not used in the current configuration\n")
            )
            tokenlocator.settoken(token)
            # check authentication
            service.get(ui, token).check()
            ui.status(_("authentication successful for the current configuration\n"))
    else:

        if not tokenlocator.tokenenforced:
            service.get(ui).check()
            ui.status(_("authentication successful for the current configuration\n"))
            return

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
            # Run through interactive authentication to obtain a token
            authenticate(ui, repo, tokenlocator)


cloudsmartlogopts = [
    (
        "d",
        "date",
        "",
        _(
            "show version of the smartlog on date specified (or on the first later date if there are no updates on the given date)"
        ),
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
    (
        "",
        "force-original-backend",
        None,
        "serve the smartlog from original mercurial backup infrastructure, rather than from the Mononoke backend regardless of the configuration (ADVANCED)",
    ),
]


@subcmd(
    "smartlog|sl",
    cloudsmartlogopts + workspace.workspaceopts,
)
def cloudsmartlog(ui, repo, template="sl_cloud", **opts):
    """get smartlog view for the default workspace of the given user

    If the requested template is not defined in the config
    the command provides a simple view as a list of draft commits.
    """

    reponame = ccutil.getreponame(repo)
    workspacename = workspace.parseworkspace(ui, opts)
    if workspacename is None:
        workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        workspacename = workspace.defaultworkspace(ui)

    if opts.get("history"):
        interactivehistory.showhistory(
            ui, repo, reponame, workspacename, template, **opts
        )
        return

    date = opts.get("date")
    version = opts.get("workspace_version")
    if date:
        parseddate = util.parsedate(date)
    else:
        parseddate = None

    if version and date:
        raise error.Abort(
            "'--workspace-version' and '--date' options can't be both provided"
        )

    ui.status(
        _("searching draft commits for the '%s' workspace for the '%s' repo\n")
        % (workspacename, reponame),
        component="commitcloud",
    )
    serv = service.get(ui, tokenmod.TokenLocator(ui).token)

    flags = []
    if ui.configbool("commitcloud", "sl_showremotebookmarks"):
        flags.append("ADD_REMOTE_BOOKMARKS")

    if ui.configbool("commitcloud", "sl_showallbookmarks"):
        flags.append("ADD_ALL_BOOKMARKS")

    if opts.get("force_original_backend"):
        flags.append("USE_ORIGINAL_BACKEND")

    if parseddate is None and not version:
        with progress.spinner(ui, _("fetching")):
            slinfo = serv.getsmartlog(reponame, workspacename, repo, 0, flags)
    else:
        with progress.spinner(ui, _("fetching")):
            slinfo = serv.getsmartlogbyversion(
                reponame, workspacename, repo, parseddate, version, 0, flags
            )
    if parseddate or version:
        formatteddate = time.strftime(
            "%Y-%m-%d %H:%M:%S", time.localtime(slinfo.timestamp)
        )
        ui.status(
            _("Smartlog version %d \nsynced at %s\n\n")
            % (slinfo.version, formatteddate)
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
    firstpublic, revdag = serv.makedagwalker(slinfo, repo)
    displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
    cmdutil.displaygraph(ui, repo, revdag, displayer, reserved=firstpublic)


@subcmd("supersmartlog|ssl", cloudsmartlogopts + workspace.workspaceopts)
def cloudsupersmartlog(ui, repo, **opts):
    """get super smartlog view for the given workspace"""
    cloudsmartlog(ui, repo, "ssl_cloud", **opts)


@subcmd(
    "hide",
    [
        ("r", "rev", [], _("revisions to hide (hash or prefix only)")),
        ("B", "bookmark", [], _("bookmarks to remove")),
        ("", "remotebookmark", [], _("remote bookmarks to remove")),
    ]
    + workspace.workspaceopts
    + cmdutil.dryrunopts,
)
def cloudhide(ui, repo, *revs, **opts):
    """remove commits or bookmarks from the cloud workspace"""
    reponame = ccutil.getreponame(repo)
    workspacename = workspace.parseworkspace(ui, opts)
    if workspacename is None:
        workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        workspacename = workspace.defaultworkspace(ui)

    with progress.spinner(ui, _("fetching commit cloud workspace")):
        serv = service.get(ui, tokenmod.TokenLocator(ui).token)
        slinfo = serv.getsmartlog(reponame, workspacename, repo, 0)
        firstpublic, revdag = serv.makedagwalker(slinfo, repo)
        cloudrefs = serv.getreferences(reponame, workspacename, 0)

    nodeinfos = slinfo.nodeinfos
    dag = slinfo.dag
    drafts = set(slinfo.draft)
    hexdrafts = set(nodemod.hex(d) for d in slinfo.draft)

    removenodes = set()

    for rev in list(revs) + opts.get("rev", []):
        if rev in hexdrafts:
            removenodes.add(nodemod.bin(rev))
        else:
            candidate = None
            for hexdraft in hexdrafts:
                if hexdraft.startswith(rev):
                    if candidate is None:
                        candidate = hexdraft
                    else:
                        raise error.Abort(_("ambiguous commit hash prefix: %s") % rev)
            if candidate is None:
                raise error.Abort(_("commit not in workspace: %s") % rev)
            removenodes.add(nodemod.bin(candidate))

    # Find the bookmarks we need to remove
    removebookmarks = set()
    for bookmark in opts.get("bookmark", []):
        kind, pattern, matcher = util.stringmatcher(bookmark)
        if kind == "literal":
            if pattern not in cloudrefs.bookmarks:
                raise error.Abort(_("bookmark not in workspace: %s") % pattern)
            removebookmarks.add(pattern)
        else:
            for bookmark in cloudrefs.bookmarks:
                if matcher(bookmark):
                    removebookmarks.add(bookmark)

    # Find the remote bookmarks we need to remove
    removeremotes = set()
    for remote in opts.get("remotebookmark", []):
        kind, pattern, matcher = util.stringmatcher(remote)
        if kind == "literal":
            if pattern not in cloudrefs.remotebookmarks:
                raise error.Abort(_("remote bookmark not in workspace: %s") % pattern)
            removeremotes.add(remote)
        else:
            for remote in cloudrefs.remotebookmarks:
                if matcher(remote):
                    removeremotes.add(remote)

    # Find the heads and bookmarks we need to remove
    allremovenodes = dag.descendants(removenodes)
    removeheads = set(allremovenodes & map(nodemod.bin, cloudrefs.heads))
    for node in allremovenodes:
        removebookmarks.update(nodeinfos[node].bookmarks)

    # Find the heads we need to remove because we are removing the last bookmark
    # to it.
    remainingheads = set(
        set(map(nodemod.bin, cloudrefs.heads)) & dag.all() - removeheads
    )
    for bookmark in removebookmarks:
        node = nodemod.bin(cloudrefs.bookmarks[bookmark])
        info = nodeinfos.get(node)
        if node in remainingheads and info:
            if removebookmarks.issuperset(set(info.bookmarks)):
                remainingheads.discard(node)
                removeheads.add(node)

    # Find the heads we need to add to keep other commits visible
    addheads = (
        dag.parents(removenodes) - allremovenodes - dag.ancestors(remainingheads)
    ) & drafts

    if removeheads:
        ui.status(_("removing heads:\n"))
        for head in sorted(removeheads):
            hexhead = nodemod.hex(head)
            ui.status(
                "    %s  %s\n"
                % (hexhead[:12], templatefilters.firstline(nodeinfos[head].message))
            )
    if addheads:
        ui.status(_("adding heads:\n"))
        for head in sorted(addheads):
            hexhead = nodemod.hex(head)
            ui.status(
                "    %s  %s\n"
                % (hexhead[:12], templatefilters.firstline(nodeinfos[head].message))
            )
    if removebookmarks:
        ui.status(_("removing bookmarks:\n"))
        for bookmark in sorted(removebookmarks):
            ui.status("    %s: %s\n" % (bookmark, cloudrefs.bookmarks[bookmark][:12]))
    if removeremotes:
        ui.status(_("removing remote bookmarks:\n"))
        for remote in sorted(removeremotes):
            ui.status("    %s: %s\n" % (remote, cloudrefs.remotebookmarks[remote][:12]))

    # Hexify all the head, as cloudrefs works with hex strings.
    removeheads = list(map(nodemod.hex, removeheads))
    addheads = list(map(nodemod.hex, addheads))

    if removeheads or addheads or removebookmarks or removeremotes:
        if opts.get("dry_run"):
            ui.status(_("not updating cloud workspace: --dry-run specified\n"))
            return 0
        with progress.spinner(ui, _("updating commit cloud workspace")):
            serv.updatereferences(
                reponame,
                workspacename,
                cloudrefs.version,
                oldheads=list(removeheads),
                newheads=list(addheads),
                oldbookmarks=list(removebookmarks),
                oldremotebookmarks=list(removeremotes),
            )
    else:
        ui.status(_("nothing to change\n"))


def authenticate(ui, repo, tokenlocator):
    """interactive authentication"""
    if not ui.interactive() or not tokenlocator.tokenenforced:
        msg = _("authentication with commit cloud required")
        raise ccerror.RegistrationError(ui, msg)

    authhelp = ui.config("commitcloud", "auth_help")
    if authhelp:
        ui.status(authhelp + "\n")

    # ui.prompt doesn't set up the prompt correctly, so pasting long lines
    # wraps incorrectly in the terminal.  Print the prompt on its own line
    # to avoid this.
    prompt = _(
        "paste your commit cloud authentication token below or run `hg cloud auth -t <token>` to set the token:\n"
    )
    ui.write(ui.label(prompt, "ui.prompt"))
    token = ui.prompt("", default="").strip()
    if token:
        ui.status(_("checking the token '%s'\n") % token)
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
        (
            "f",
            "force",
            None,
            "reset local state (reinitialise the local cache of backed up heads from the server)",
        ),
    ],
    _("[-r REV...]"),
)
def cloudbackup(ui, repo, *revs, **opts):
    """back up commits to commit cloud

    Commits that have already been backed up will be skipped.

    If no revision is specified, backs up all visible commits.
    """
    revs = revs + tuple(opts.get("rev", ()))
    if ui.configbool("commitcloud", "usehttpupload"):
        opts["rev"] = revs
        return cloudupload(ui, repo, **opts)

    repo.ignoreautobackup = True

    force = opts.get("force")
    inbackground = opts.get("background")
    if revs:
        if inbackground:
            raise error.Abort("'--background' cannot be used with specific revisions")
        revs = scmutil.revrange(repo, revs)
    else:
        revs = None

    if force and inbackground:
        raise error.Abort("'--background' cannot be used with '--force'")

    if inbackground:
        background.backgroundbackup(repo)
        return 0

    backedup, failed = backup.backup(
        repo,
        revs,
        connect_opts=opts,
        force=force,
    )

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
    "listworkspaces|list",
    [
        (
            "",
            "user",
            "",
            _(
                "username, defaults to current user (specified by username or email address)"
            ),
        ),
        ("a", "all", None, _("list all workspaces, including archived")),
    ],
)
def cloudlistworspaces(ui, repo, **opts):
    """list Commit Cloud workspaces that are available on the server for the user"""

    user = opts.get("user")
    workspacenameprefix = workspace.userworkspaceprefix(ui, user if user else None)
    currentworkspace = workspace.currentworkspace(repo)
    reponame = ccutil.getreponame(repo)
    activeonly = not opts.get("all")

    ui.status(
        _("searching workspaces for the '%s' repo\n") % reponame,
        component="commitcloud",
    )

    serv = service.get(ui, tokenmod.TokenLocator(ui).token)
    winfos = serv.getworkspaces(reponame, workspacenameprefix)
    if not winfos:
        ui.write(_("no workspaces found with the prefix %s\n") % workspacenameprefix)
        return

    active, archived = [], []
    for winfo in winfos:
        (active if not winfo.archived else archived).append(winfo)

    if not active and activeonly:
        ui.write(
            _("no active workspaces found with the prefix %s\n") % workspacenameprefix
        )
        return

    ui.write(
        ui.label(_("the following commitcloud workspaces are available:\n"), "bold")
    )

    anyconnected = False
    for winfo in active if activeonly else active + archived:
        fullname, shortname = (winfo.name, winfo.name[len(workspacenameprefix) :])
        isconnected = " (connected)" if fullname == currentworkspace else ""
        if isconnected:
            shortname = ui.label(shortname, "bold")
            anyconnected = True
        if not winfo.archived:
            ui.write(_("        %s%s\n") % (shortname, isconnected))
        else:
            ui.write(_("        %s%s (archived)\n") % (shortname, isconnected))

    currentmayberenamed = (
        not user
        and not anyconnected
        and currentworkspace
        and currentworkspace.startswith(workspacenameprefix)
    )
    # current workspace is missing on the server
    if currentmayberenamed:
        ui.write(
            _("        %s (connected) (renamed or removed)\n")
            % ui.label(currentworkspace[len(workspacenameprefix) :], "bold")
        )

    ui.status(_("run `hg cloud sl -w <workspace name>` to view the commits\n"))

    ui.status(
        _(
            "run `hg cloud switch -w <workspace name>` to switch to a different workspace\n"
        )
    )

    if activeonly and archived:
        ui.status(
            _("run `hg cloud list --all` to list all workspaces, including deleted\n")
        )


@subcmd("deleteworkspace|delete", [] + workspace.workspaceopts)
def clouddeleteworkspace(ui, repo, **opts):
    """Delete (archive) workspace from commit cloud"""

    workspacename = workspace.parseworkspace(ui, opts)
    if workspacename is None:
        raise error.Abort(_("workspace name should be provided\n"))

    confirmed = True
    if ui.interactive():
        prompt = (
            _("are you sure you want to delete the workspace %s [yn]:\n")
            % workspacename
        )
        ui.write(ui.label(prompt, "ui.prompt"))
        confirmed = ui.prompt("", default="").strip().lower().startswith("y")

    if not confirmed:
        return

    reponame = ccutil.getreponame(repo)
    service.get(ui, tokenmod.TokenLocator(ui).token).updateworkspacearchive(
        reponame, workspacename, True
    )
    ui.status(
        _("workspace %s has been deleted\n") % workspacename, component="commitcloud"
    )


@subcmd("undeleteworkspace|undelete", [] + workspace.workspaceopts)
def cloudundeleteworkspace(ui, repo, **opts):
    """Restore (unarchive) workspace in commit cloud"""

    workspacename = workspace.parseworkspace(ui, opts)
    if workspacename is None:
        raise error.Abort(_("workspace name should be provided\n"))

    reponame = ccutil.getreponame(repo)
    service.get(ui, tokenmod.TokenLocator(ui).token).updateworkspacearchive(
        reponame, workspacename, False
    )
    ui.status(
        _("workspace %s has been restored\n") % workspacename, component="commitcloud"
    )


@subcmd(
    "renameworkspace|rename",
    [
        (
            "s",
            "source",
            "",
            _("short name for the source workspace, defaults to the current workspace"),
        ),
        ("d", "destination", "", _("short name for the destination workspace")),
        ("", "rehost", None, _("rebind commit cloud workspace to the current host")),
        (
            "",
            "raw-source-workspace",
            "",
            _(
                "raw source workspace name (e.g. 'user/<username>/<workspace>'), "
                "permissions are checked on the server (ADVANCED)"
            ),
        ),
        (
            "",
            "raw-destination-workspace",
            "",
            _(
                "raw destination workspace name (e.g. 'user/<username>/<workspace>'), "
                "permissions are checked on the server (ADVANCED)"
            ),
        ),
    ],
)
def cloudrenameworkspace(ui, repo, skipconfirmation=False, **opts):
    """rename Commit Cloud workspace

    The command only supports renaming the workspaces of the current user.
    """

    source = opts.get("source")
    destination = opts.get("destination")
    rehost = opts.get("rehost")
    rawsource = opts.get("raw_source_workspace")
    rawdestination = opts.get("raw_destination_workspace")

    userworkspaceprefix = workspace.userworkspaceprefix(ui)
    (currentworkspace, locallyowned) = workspace.currentworkspacewithlocallyownedinfo(
        repo
    )
    reponame = ccutil.getreponame(repo)

    if destination and rehost:
        raise error.Abort(
            _("'rehost' option and 'destination' option are incompatible")
        )

    if not destination:
        destination = workspace.hostnameworkspace(ui) if rehost else rawdestination
        if not destination:
            raise error.Abort(_("please provide the destination workspace"))
    else:
        destination = userworkspaceprefix + destination

    if not source:
        if rawsource:
            source = rawsource
        # default to the current workspace
        elif currentworkspace:
            source = currentworkspace
            if not locallyowned:
                raise error.Abort(_("rename is only supported for personal workspaces"))
        else:
            raise error.Abort(
                _(
                    "the repo is not connected to any workspace, "
                    "please provide the source workspace"
                )
            )
    else:
        source = userworkspaceprefix + source

    if source == workspace.defaultworkspace(ui):
        raise error.Abort(_("rename of the default workspace is not allowed"))

    confirmed = True
    if ui.interactive() and not skipconfirmation:
        prompt = _(
            "are you sure you would like to rename the '%s' workspace to '%s' for the repo '%s'[yn]:\n"
        ) % (source, destination, reponame)
        ui.write(ui.label(prompt, "ui.prompt"))
        confirmed = ui.prompt("", default="").strip().lower().startswith("y")
    if not confirmed:
        return

    if source == currentworkspace:
        # sync all the current commits and bookmarks before rename
        cloudsync(ui, repo, **opts)

    ui.status(
        _("rename the '%s' workspace to '%s' for the repo '%s'\n")
        % (source, destination, reponame),
        component="commitcloud",
    )

    service.get(ui, tokenmod.TokenLocator(ui).token).renameworkspace(
        reponame, source, destination
    )

    if source == currentworkspace:
        with backuplock.lock(repo), repo.wlock(), repo.lock():
            # move the current state
            syncstate.SyncState.movestate(repo, source, destination)
            # move the subscription
            subscription.move(repo, source, destination)
            # update the current workspace name
            workspace.setworkspace(repo, destination)

    ui.status(_("rename successful\n"), component="commitcloud")


@subcmd(
    "reclaimworkspaces|reclaim",
    [
        (
            "",
            "user",
            "",
            _(
                "former username (can be specified by username or email address), "
                "defaults to the owner of the workspace the repo is connected to"
            ),
        )
    ],
)
def cloudreclaimworkspaces(ui, repo, **opts):
    """reclaim Commit Cloud workspaces to the current user

    The command is useful for username changes in configuration
    """
    reponame = ccutil.getreponame(repo)

    user = opts.get("user")
    if user:
        formeruserprefix = workspace.userworkspaceprefix(ui, user)
    else:
        (
            currentworkspace,
            migrationcheck,
        ) = workspace.currentworkspacewithusernamecheck(repo)

        if not migrationcheck:
            raise error.Abort(
                _(
                    "please, provide '--user' option, "
                    "can not identify the former username from the current workspace"
                )
            )
        formeruserprefix = currentworkspace.rpartition("/")[0] + "/"

    currentuserprefix = workspace.userworkspaceprefix(ui)

    if currentuserprefix == formeruserprefix:
        ui.status(
            _("nothing to reclaim: triggered for the same username\n"),
            component="commitcloud",
        )
        return 1

    formerworkspaces = list(
        service.get(ui, tokenmod.TokenLocator(ui).token).getworkspaces(
            reponame, formeruserprefix
        )
    )

    if not formerworkspaces:
        ui.status(_("nothing to reclaim\n"), component="commitcloud")

    active, archived = [], []
    for winfo in formerworkspaces:
        (active if not winfo.archived else archived).append(winfo)

    def getshortname(formerworkspacename):
        return formerworkspacename[len(formeruserprefix) :]

    def reclaimhelper(workspaces, archived=False):
        if not workspaces:
            return

        archivedlabel = (
            ui.label(_("archived"), "bold")
            if archived
            else ui.label(_("active"), "bold")
        )
        ui.status(
            _("the following %s workspaces are reclaim candidates:\n") % archivedlabel,
            component="commitcloud",
        )
        for winfo in workspaces:
            ui.write(_("    %s\n") % getshortname(winfo.name))

        confirmed = True
        if ui.interactive():
            prompt = _(
                "are you sure you would like to reclaim the workspaces above [yn]:\n"
            )
            ui.write(ui.label(prompt, "ui.prompt"))
            confirmed = ui.prompt("", default="").strip().lower().startswith("y")

        if not confirmed:
            return

        for winfo in workspaces:
            shortname = getshortname(winfo.name)
            renameopts = {
                "raw_source_workspace": winfo.name,
                "raw_destination_workspace": currentuserprefix + shortname,
            }
            configoverride = {("ui", "quiet"): True}
            try:
                with ui.configoverride(configoverride):
                    cloudrenameworkspace(ui, repo, skipconfirmation=True, **renameopts)
            except Exception as e:
                ui.status(
                    ui.label(_("skipping the workspace '%s'\n") % shortname, "bold"),
                    component="commitcloud",
                )
                ui.status(_("reason: %s\n") % e)

        ui.status(
            _("reclaim of %s workspaces completed\n") % archivedlabel,
            component="commitcloud",
        )

    reclaimhelper(active)
    reclaimhelper(archived, archived=True)


@subcmd(
    "sync",
    scmdaemon.scmdaemonsyncopts
    + pullopts
    + [
        (
            "",
            "reason",
            "",
            _(
                "reason why the sync has been triggered (used for logging purposes) (ADVANCED)"
            ),
        ),
        (
            "",
            "best-effort",
            False,
            _(
                "avoids taking the repo lock when possible, but may fail if "
                "other commands are running (ADVANCED)"
            ),
        ),
    ],
)
def cloudsync(ui, repo, cloudrefs=None, **opts):
    """backup and synchronize commits with the commit cloud service"""
    repo.ignoreautobackup = True
    full = opts.get("full")
    besteffort = opts.get("best_effort")

    if scmdaemon.checkmaybeskiprun(repo, opts):
        return 0

    maybeversion = scmdaemon.parsemaybeworkspaceversion(opts)
    maybeworkspace = scmdaemon.parsemaybeworkspacename(opts)
    maybebgssh = scmdaemon.parsemaybebgssh(ui, opts)

    if maybebgssh:
        ui.setconfig("ui", "ssh", maybebgssh)

    reason = opts.get("reason")
    if reason:
        ui.log("commitcloud_sync_reason", commitcloud_sync_reason=reason)
    elif ui.interactive():
        ui.log("commitcloud_sync_reason", commitcloud_sync_reason="manual run")

    ret = sync.sync(
        repo,
        cloudrefs,
        full,
        maybeversion,
        maybeworkspace,
        connect_opts=opts,
        besteffort=besteffort,
    )
    return ret


@subcmd("recover", [] + pullopts)
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
    ],
)
def cloudcheck(ui, repo, **opts):
    """check if commits have been backed up

    If no revision are specified then it checks working copy parent.
    """

    if ui.configbool("commitcloud", "usehttpupload"):
        # eden api based lookup
        revs = opts.get("rev")
        if not revs:
            revs = ["."]
        nodestocheck = [repo[r].node() for r in scmutil.revrange(repo, revs)]
        missingnodes = set(edenapi_upload._filtercommits(repo, nodestocheck))
        for n in nodestocheck:
            ui.write(nodemod.hex(n), " ")
            ui.write(_("uploaded") if not (n in missingnodes) else _("not uploaded"))
            ui.write(_("\n"))
            return

    revs = opts.get("rev")
    remote = opts.get("remote")
    if not revs:
        revs = ["."]

    unfi = repo
    revs = scmutil.revrange(repo, revs)
    nodestocheck = [repo[r].hex() for r in revs]

    if remote:
        # wireproto based lookup
        remotepath = ccutil.getremotepath(ui)
        getconnection = lambda: repo.connectionpool.get(remotepath, opts)
        isbackedup = {
            nodestocheck[i]: res
            for i, res in enumerate(
                dependencies.infinitepush.isbackedupnodes(getconnection, nodestocheck)
            )
        }
    else:
        # local backup state based lookup
        backeduprevs = unfi.revs("backedup()")
        isbackedup = {
            node: (unfi[node].rev() in backeduprevs) or not unfi[node].mutable()
            for node in nodestocheck
        }

    for n in nodestocheck:
        ui.write(n, " ")
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
                    "'@prog@ cloud disable' does not affect running backup processes\n"
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

    userworkspaceprefix = workspace.userworkspaceprefix(ui)
    if workspacename.startswith(userworkspaceprefix):
        # check it with the server
        if not service.get(ui, tokenmod.TokenLocator(ui).token).getworkspaces(
            ccutil.getreponame(repo), workspacename
        ):
            ui.write(
                _(
                    "Workspace: %s (renamed or removed) (run `hg cloud list` and switch to a different one)\n"
                )
                % ui.label(workspacename[len(userworkspaceprefix) :], "bold")
            )
        else:
            ui.write(
                _("Workspace: %s\n")
                % ui.label(workspacename[len(userworkspaceprefix) :], "bold")
            )

    ui.write(_("Raw Workspace Name: %s\n") % workspacename)

    backgroundnabled = background.autobackupenabled(repo)
    autosync = "ON" if backgroundnabled else "OFF"
    ui.write(_("Automatic Sync (on local changes): %s\n") % autosync)

    if backgroundnabled and subscription.testservicestatus(repo):
        ui.write(_("Automatic Sync via 'Scm Daemon' (on remote changes): ON\n"))
        logpath = util.expanduserpath(
            ui.config("commitcloud", "scm_daemon_log_path", "")
        )
        if logpath:
            ui.write(_("Scm Daemon Log Path: %s\n") % logpath)
    else:
        ui.write(_("Automatic Sync via 'Scm Daemon' (on remote changes): OFF\n"))

    logdir = ui.config("infinitepushbackup", "logdir", "")
    if logdir:
        user = util.getuser()
        if user:
            logpath = background.getlogfilename(logdir, user, ccutil.getreponame(repo))
            if logpath:
                ui.write(_("Background Backup Log Path (recent): %s\n") % logpath)

    state = syncstate.SyncState(repo, workspacename)

    ui.write(_("Last Sync Version: %s\n") % state.version)
    if state.maxage is not None:
        ui.write(_("Last Sync Maximum Commit Age: %s days\n") % state.maxage)
    ui.write(
        _("Last Sync Heads: %d (%d omitted)\n")
        % (len(state.heads), len(state.omittedheads))
    )
    ui.write(
        _("Last Sync Bookmarks: %d (%d omitted)\n")
        % (len(state.bookmarks), len(state.omittedbookmarks))
    )
    ui.write(_("Last Sync Remote Bookmarks: %d\n") % (len(state.remotebookmarks)))

    ui.write(_("Last Sync Time: %s\n") % time.ctime(state.lastupdatetime))

    if repo.svfs.isfile(sync._syncstatusfile):
        status = pycompat.decodeutf8(repo.svfs.read(sync._syncstatusfile))
    else:
        status = "Not logged"
    ui.write(_("Last Sync Status: %s\n") % status)


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
        with lockmod.lock(
            repo.sharedvfs, backuplock.lockfilename, timeout=timeout, ui=ui
        ):
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
    ],
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
    ],
)
def isbackedup(ui, repo, **opts):
    """check if commits have been backed up (DEPRECATED)

    If no revision are specified then it checks working copy parent.

    'hg isbackedup' is deprecated in favour of 'hg cloud check'.
    """
    return cloudcheck(ui, repo, **opts)


@subcmd(
    "getfrombackup",
    [
        ("r", "rev", [], _("revisions to lookup and pull (full hashes only)")),
    ],
)
def getfrombackup(ui, repo, **opts):
    """Downloading and applying mercurial bundles directly for list of given heads

    Backup store stores commits as mercurial bundles that can be fetched directly from the store and applied.

    The command could be useful when we migrate our server from one backend to another (Mononoke) and some commits can be missing in Mononoke.
    """
    revs = opts.get("rev")
    if not revs:
        raise error.Abort(
            _(
                "no revision specified, please run with `hg cloud getfrombackup -r <revs>`"
            )
        )

    service.get(ui, tokenmod.TokenLocator(ui).token).getheadsfrombackupbundlestore(
        repo, revs
    )


@subcmd(
    "upload",
    [
        ("r", "rev", [], _("revisions to upload to Commit Cloud")),
        (
            "f",
            "force",
            None,
            "reupload commits without checking what is present on the server",
        ),
    ],
)
def cloudupload(ui, repo, **opts):
    """Upload draft commits using EdenApi Uploads

    Commits that have already been uploaded will be skipped.
    If no revision is specified, uploads all visible commits.
    """

    repo.ignoreautobackup = True

    revs = opts.get("rev")
    if revs:
        revs = scmutil.revrange(repo, revs)
    else:
        revs = None

    uploaded, failed = upload.upload(repo, revs, force=opts.get("force"))
    if uploaded:
        with repo.lock():
            backupstate.BackupState(
                repo, ccutil.getremotepath(ui), usehttp=True
            ).update(uploaded)

    if failed:
        if len(failed) < 10:
            while failed:
                repo.ui.warn(
                    _("failed to upload %s\n") % nodemod.short(failed.pop()),
                    component="commitcloud",
                )
        else:
            repo.ui.warn(
                _("failed to upload %d commits\n") % len(failed),
                component="commitcloud",
            )
        return 2
