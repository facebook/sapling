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
    return launch_server(
        ui,
        cwd=repo.root,
        port=port,
        open_isl=open_isl,
        json_output=json_output,
        foreground=foreground,
    )


def launch_server(
    ui, *, cwd, port=DEFAULT_PORT, open_isl=True, json_output=False, foreground=False
):
    isl_args = get_isl_args()
    ui.status_err(_("launching web server for Interactive Smartlog...\n"))
    args = ["--port", str(port)]
    if not open_isl:
        args.append("--no-open")
    if json_output:
        args.append("--json")
    if foreground:
        args.append("--foreground")
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
    # @fb-only: isl_bin = "C:/Tools/hg/isl/isl" 
    # @fb-only: if os.path.isfile(isl_bin): 
        # @fb-only: return ["dotslash", isl_bin] 
    # TODO(T125822314): Fix packaging issue so isl works on Windows.
    raise error.Abort(_("isl is not currently supported on Windows"))
