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
    return launch_server(ui, cwd=repo.root, port=port)


def launch_server(ui, *, cwd, port=DEFAULT_PORT):
    if iswindows:
        # TODO: Fix packaging issue so isl works on Windows.
        raise error.Abort(_("isl is not currently supported on Windows"))

    isl_bin = os.path.join(os.path.dirname(__file__), "isl")

    ui.status(_("launching web server for Interactive Smartlog...\n"))
    subprocess.call(["dotslash", isl_bin, "--port", str(port)], cwd=cwd)
