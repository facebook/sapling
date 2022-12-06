# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import socket
from typing import List, Optional, Tuple

from edenscm import config, error, pycompat, util
from edenscm.i18n import _


workspaceopts: List[Tuple[str, ...]] = [
    (
        "u",
        "user",
        "",
        _(
            "use the workspaces of a different user (specified by username or "
            "email address) instead of the current user"
        ),
    ),
    ("w", "workspace", "", _("name of workspace to use (default: 'default')")),
    (
        "",
        "raw-workspace",
        "",
        _("raw workspace name (e.g. 'user/<username>/<workspace>') (ADVANCED)"),
    ),
]


def parseworkspace(ui, opts):
    """Parse command line options to get a workspace name.

    Returns None if the user specifies no workspace command line arguments.

    Workspace naming convention:
    section/section_name/workspace_name
        where section is one of ('user', 'group', 'team', 'project')
    Examples:
        team/source_control/shared
        user/<username>/default
        project/commit_cloud/default
    """
    rawworkspace = opts.get("raw_workspace")
    if rawworkspace:
        for opt in "user", "workspace":
            if opts.get(opt):
                raise error.Abort(_("--raw-workspace and --%s are incompatible") % opt)
        return rawworkspace

    user = opts.get("user")
    workspace = opts.get("workspace")
    if not any([user, workspace]):
        # User has not specified any workspace options.
        return None

    # Currently only "user" workspaces are implemented
    if not user:
        user = ui.username()
    domains = ui.configlist("commitcloud", "email_domains")
    user = util.emaildomainuser(user, domains)
    prefix = "user/%s/" % user

    if not workspace:
        workspace = "default"

    # Workaround for users specifying the full workspace name with "-w"
    if workspace.startswith("user/") and not opts.get("user"):
        msg = (
            "specifying full workspace names with '-w' is deprecated\n"
            "(use '-u' to select another user's workspaces)\n"
        )
        ui.warn(msg)
        return workspace

    return prefix + workspace


def defaultworkspace(ui, user: Optional[str] = None) -> str:
    """Returns the default workspace for the given or current user"""
    if user is None:
        domains = ui.configlist("commitcloud", "email_domains")
        user = util.emaildomainuser(ui.username(), domains)
    return "user/%s/default" % user


def userworkspaceprefix(ui, user: Optional[str] = None) -> str:
    """Returns the workspace prefix for the given user or current user"""
    if user is None:
        domains = ui.configlist("commitcloud", "email_domains")
        user = util.emaildomainuser(ui.username(), domains)
    return "user/%s/" % user


def hostnameworkspace(ui, user: Optional[str] = None) -> str:
    """Returns the host workspace for the given or current user for the current host"""
    if user is None:
        domains = ui.configlist("commitcloud", "email_domains")
        user = util.emaildomainuser(ui.username(), domains)
    return "user/%s/%s" % (
        user,
        ui.config("commitcloud", "hostname", socket.gethostname()),
    )


def parseworkspaceordefault(ui, repo, opts):
    """Parse command line options to get a workspace name

    If not provided, use the current workspace name.

    If the repo is not connected to any workspace, assume the workspace 'default'.
    """
    workspacename = parseworkspace(ui, opts)
    if workspacename is None:
        workspacename = currentworkspace(repo)
    if workspacename is None:
        ui.warn(
            _(
                "the repository is not connected to any workspace, assuming the 'default' workspace\n"
            )
        )
        workspacename = defaultworkspace(ui)
    return workspacename


filename = "commitcloudrc"


def _get(repo, *names):
    """Read commitcloudrc file to get a value"""
    if repo.svfs.exists(filename):
        with repo.svfs.open(filename, r"rb") as f:
            cloudconfig = config.config()
            cloudconfig.read(filename, f)
            return (
                cloudconfig.get("commitcloud", names[0])
                if len(names) == 1
                else tuple(cloudconfig.get("commitcloud", name) for name in names)
            )
    else:
        return None if len(names) == 1 else tuple(None for name in names)


def currentworkspace(repo):
    """
    Returns the currently connected workspace, or None if the repo is not
    connected to a workspace.
    """
    return _get(repo, "current_workspace")


def currentworkspacewithlocallyownedinfo(repo):
    """
    Returns the currently connected workspace, or None if the repo is not
    connected to a workspace and the flag that it's locally owned.
    """
    (current_workspace, locally_owned) = _get(
        repo, "current_workspace", "locally_owned"
    )
    return (current_workspace, eval(locally_owned) if locally_owned else False)


def currentworkspacewithusernamecheck(repo):
    """
    Returns the currently connected workspace, or None if the repo is not
    connected to a workspace and the flag if it requires username migration
    because the workspace name doesn't match the current username anymore.
    """

    (current_workspace, locally_owned) = currentworkspacewithlocallyownedinfo(repo)
    migrationrequired = locally_owned and not current_workspace.startswith(
        userworkspaceprefix(repo.ui)
    )
    return (current_workspace, migrationrequired)


def disconnected(repo):
    """
    Returns True if the user has manually disconnected from a workspace.
    """
    disconnected = _get(repo, "is_disconnected")
    if disconnected is None or isinstance(disconnected, bool):
        return disconnected
    return util.parsebool(disconnected)


def setworkspace(repo, workspace) -> None:
    """Sets the currently connected workspace."""
    with repo.wlock(), repo.lock(), repo.svfs.open(
        filename, "wb", atomictemp=True
    ) as f:
        locallyowned = workspace.startswith(userworkspaceprefix(repo.ui))
        f.write(
            b"[commitcloud]\ncurrent_workspace=%s\nlocally_owned=%s\n"
            % (pycompat.encodeutf8(workspace), pycompat.encodeutf8(str(locallyowned)))
        )


def clearworkspace(repo) -> None:
    """Clears the currently connected workspace."""
    with repo.wlock(), repo.lock(), repo.svfs.open(
        filename, "wb", atomictemp=True
    ) as f:
        f.write(b"[commitcloud]\nis_disconnected=true\n")
