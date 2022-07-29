# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""launcher for Interactive Smartlog GUI (EXPERIMENTAL)
"""

import os
import os.path
import subprocess

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
        ("", "open", True, _("open ISL in a local browser")),
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
    return launch_server(ui, cwd=repo.root, port=port, open_isl=open_isl)


def launch_server(ui, *, cwd, port=DEFAULT_PORT, open_isl=True):
    if iswindows:
        # TODO(T125822314): Fix packaging issue so isl works on Windows.
        raise error.Abort(_("isl is not currently supported on Windows"))

    isl_bin = os.path.join(os.path.dirname(__file__), "isl")

    ui.status(_("launching web server for Interactive Smartlog...\n"))
    args = ["dotslash", isl_bin, "--port", str(port)]
    if not open_isl:
        args.append("--no-open")
    subprocess.call(args, cwd=cwd)
