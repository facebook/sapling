# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
    "web|isl",
    [
        (
            "p",
            "port",
            DEFAULT_PORT,
            _("port for Sapling Web"),
        ),
        ("", "json", False, _("output machine-readable JSON")),
        ("", "open", True, _("open Sapling Web in a local browser")),
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
        (
            "",
            "platform",
            "",
            _(
                "which environment ISL is being embedded in, used to support IDE integrations (ADVANCED)"
            ),
        ),
    ],
)
def isl_cmd(ui, repo, *args, **opts):
    """launch Sapling Web GUI on localhost

    Sapling Web is a collection of web-based tools including Interactive Smartlog,
    which is a GUI that facilitates source control operations such as creating,
    reordering, or rebasing commits.
    Running this command launches a web server that makes Sapling Web and
    Interactive Smartlog available in a local web browser.

    Examples:

    Launch Sapling Web locally on port 8081::

        $ @prog@ web --port 8081
        Listening on http://localhost:8081/?token=bbe168b7b4af1614dd5b9ddc48e7d30e&cwd=%2Fhome%2Falice%2Fsapling
        Server logs will be written to /dev/shm/tmp/isl-server-logrkrmxp/isl-server.log

    Using the ``--json`` option to get the current status of Sapling Web::

        $ @prog@ web --port 8081 --json | jq
        {
            "url": "http://localhost:8081/?token=bbe168b7b4af1614dd5b9ddc48e7d30e&cwd=%2Fhome%2Falice%2Fsapling",
            "port": 8081,
            "token": "bbe168b7b4af1614dd5b9ddc48e7d30e",
            "pid": 1521158,
            "wasServerReused": true,
            "logFileLocation": "/dev/shm/tmp/isl-server-logrkrmxp/isl-server.log",
            "cwd": "/home/alice/sapling"
        }

    Using the ``--kill`` option to shut down the server::

        $ @prog@ web --port 8081 --kill
        killed ISL server process 1521158
    """
    port = opts.get("port")
    open_isl = opts.get("open")
    json_output = opts.get("json")
    foreground = opts.get("foreground")
    kill = opts.get("kill")
    force = opts.get("force")
    platform = opts.get("platform")
    return launch_server(
        ui,
        cwd=repo.root,
        port=port,
        open_isl=open_isl,
        json_output=json_output,
        foreground=foreground,
        force=force,
        kill=kill,
        platform=platform,
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
    platform=None,
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
    if platform:
        args.append("--platform")
        args.append(str(platform))
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

    # Assuming __file__ looks like C:\some\place\sl\python39.zip\edenscm\commands\isl.pyc,
    # edenscm-isl should be located in "sl" alongside the zip.
    return [
        os.path.join(
            os.path.dirname(__file__), "..", "..", "..", "edenscm-isl", "run-isl.bat"
        )
    ]
