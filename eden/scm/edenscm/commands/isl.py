# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""launcher for Interactive Smartlog GUI (EXPERIMENTAL)
"""

import os
import os.path
import subprocess
from typing import List

from .. import error
from ..i18n import _
from ..pycompat import iswindows

from . import util
from .cmdtable import command

DEFAULT_PORT = 3011


@command(
    "isl",
    [
        (
            "p",
            "port",
            DEFAULT_PORT,
            _("port for ISL web server"),
        ),
        ("", "json", False, _("output machine-readable JSON")),
        ("", "open", True, _("open ISL in a local browser")),
        ("f", "foreground", False, _("keep the server process in the foreground")),
        (
            "",
            "kill",
            False,
            _("kill any running server process, but do not start a new server"),
        ),
        (
            "",
            "force",
            False,
            _("kill any running server process, then start a new server"),
        ),
    ],
)
def isl_cmd(ui, repo, *args, **opts):
    """launch Interactive Smartlog web server on localhost

    Interactive Smartlog (ISL) is a GUI that facilitates source control
    operations, such as creating, reordering, or rebasing commits.
    Running this command launches a web server that makes ISL available via a
    web interface.
    """
    port = opts.get("port")
    open_isl = opts.get("open")
    json_output = opts.get("json")
    foreground = opts.get("foreground")
    kill = opts.get("kill")
    force = opts.get("force")
    return launch_server(
        ui,
        cwd=repo.root,
        port=port,
        open_isl=open_isl,
        json_output=json_output,
        foreground=foreground,
        force=force,
        kill=kill,
    )


def launch_server(
    ui,
    *,
    cwd,
    port=DEFAULT_PORT,
    open_isl=True,
    json_output=False,
    foreground=False,
    kill=False,
    force=False,
):
    isl_args = get_isl_args()
    if isl_args[0] == "dotslash":
        ui.status_err(_("launching web server for Interactive Smartlog...\n"))
        if not foreground:
            ui.status_err(
                _("re-run with --foreground and check VPN if slow to start.\n")
            )
    args = ["--port", str(port)]
    args.append("--command")
    args.append(util.hgcmd()[0])
    if not open_isl:
        args.append("--no-open")
    if json_output:
        args.append("--json")
    if foreground:
        args.append("--foreground")
    if force:
        args.append("--force")
    if kill:
        args.append("--kill")
    subprocess.call(isl_args + args, cwd=cwd)


def get_isl_args() -> List[str]:
    if iswindows:
        return get_isl_args_on_windows()

    this_dir = os.path.dirname(__file__)
    isl_bin = os.path.join(this_dir, "isl")
    if os.path.isfile(isl_bin):
        # This is the path to ISL in the Buck-built release.
        return ["dotslash", isl_bin]
    else:
        # This is the path to ISL in the Make-built release.
        script = "run-isl.bat" if iswindows else "run-isl"
        return [os.path.join(this_dir, "..", "..", "edenscm-isl", script)]


def get_isl_args_on_windows() -> List[str]:
    # @fb-only
    # @fb-only
        # @fb-only
    # TODO(T125822314): Fix packaging issue so isl works on Windows.
    raise error.Abort(_("isl is not currently supported on Windows"))
