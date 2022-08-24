# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import error
from edenscm.i18n import _

from . import background


scmdaemonsyncopts = [
    (
        "",
        "raw-workspace-name",
        "",
        _(
            "target raw workspace name to sync"
            "(skip `cloud sync` if the target workspace is different than the current one) (EXPERIMENTAL)"
        ),
    ),
    (
        "",
        "workspace-version",
        "",
        _(
            "target workspace version to sync"
            "(skip `cloud sync` if the target version is less or equal than the current one) (EXPERIMENTAL)"
        ),
    ),
    (
        "",
        "check-autosync-enabled",
        None,
        _(
            "check settings for background commit cloud operations"
            "(skip `cloud sync` if background operations are currently disabled) (EXPERIMENTAL)"
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


def parsemaybeworkspaceversion(opts):
    versionstr = opts.get("workspace_version")
    if versionstr:
        try:
            return int(versionstr)
        except ValueError:
            raise error.Abort(
                _("error: argument 'workspace-version' should be a number")
            )
    return None


def parsemaybeworkspacename(opts):
    return opts.get("raw_workspace_name")


def parsemaybebgssh(ui, opts):
    if opts.get("use_bgssh"):
        return ui.config("infinitepush", "bgssh")
    return None


def checkmaybeskiprun(repo, opts):
    if opts.get("check_autosync_enabled") and not background.autobackupenabled(repo):
        repo.ui.status(
            _("background operations are currently disabled\n"), component="commitcloud"
        )
        return True
    return False
