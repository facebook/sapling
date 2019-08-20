# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import subprocess

from .. import encoding, util
from ..i18n import _
from .cmdtable import command


@command("debugrunshell", [("", "cmd", "", _("command to run"))], norepo=True)
def debugrunshell(ui, *args, **opts):
    """run a shell command"""
    # XXX: ui.fin/fout/ferr are not used for this subprocess.
    # It's very hard. Things to consider:
    # - For stdin: No way from this side to know when the subprocess
    #   wants data. We cannot use `ui.fin.read(1)` if the other side
    #   does not want data.
    # - For stdout and stderr: "istty" property might get lost.
    # - Windows: Many things (file handler, TTY, etc.) are different.

    cmd = opts["cmd"]
    env = encoding.environ.copy()
    env["HG"] = util.hgexecutable()

    return subprocess.call(cmd, shell=True, env=env)
