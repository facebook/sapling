# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import config, error, util
from edenscm.mercurial.i18n import _


workspaceopts = [
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
    domain = ui.config("commitcloud", "email_domain")
    user = util.emaildomainuser(user, domain)
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


def defaultworkspace(ui, user=None):
    """Returns the default workspace for the given or current user"""
    if user is None:
        domain = ui.config("commitcloud", "email_domain")
        user = util.emaildomainuser(ui.username(), domain)
    return "user/%s/default" % user


filename = "commitcloudrc"


def _get(repo, name):
    """Read commitcloudrc file to get a value"""
    if repo.svfs.exists(filename):
        with repo.svfs.open(filename, r"rb") as f:
            cloudconfig = config.config()
            cloudconfig.read(filename, f)
            return cloudconfig.get("commitcloud", name)
    else:
        return None


def currentworkspace(repo):
    """
    Returns the currently connected workspace, or None if the repo is not
    connected to a workspace.
    """
    return _get(repo, "current_workspace")


def disconnected(repo):
    """
    Returns True if the user has manually disconnected from a workspace.
    """
    disconnected = _get(repo, "disconnected")
    if disconnected is None or isinstance(disconnected, bool):
        return disconnected
    return util.parsebool(disconnected)


def setworkspace(repo, workspace):
    """Sets the currently connected workspace."""
    with repo.wlock(), repo.lock(), repo.svfs.open(filename, "w", atomictemp=True) as f:
        f.write("[commitcloud]\ncurrent_workspace=%s\n" % workspace)


def clearworkspace(repo):
    """Clears the currently connected workspace."""
    with repo.wlock(), repo.lock(), repo.svfs.open(filename, "w", atomictemp=True) as f:
        f.write("[commitcloud]\ndisconnected=true\n")
