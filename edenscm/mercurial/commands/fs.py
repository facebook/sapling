# commands/fs.py - commands for controlling the edenfs daemon
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import subprocess

from .. import cmdutil, error, registrar
from ..i18n import _


command = registrar.command()


@command("fs", [], subonly=True, norepo=True)
def fs(ui, **opts):
    """control the edenfs daemon"""


table = {}
subcmd = fs.subcommand(
    table,
    categories=[
        ("Start and stop the edenfs daemon", ["start", "stop", "restart"]),
        ("Maintenance for the edenfs daemon", ["check", "doctor", "gc"]),
    ],
)


def _calledenfsctl(ui, command, args=None, opts=None):
    cmd = ["edenfsctl"] + command.split()
    if opts:
        commandopts = cmdutil.findsubcmd(command.split(), table)[3][1]
        for commandopt in commandopts:
            name = commandopt[1]
            key = name.replace("-", "_")
            if key in opts:
                value = opts[key]
                default = commandopt[2]
                if value:
                    if default in (None, True, False):
                        cmd.append("--%s" % name)
                    else:
                        cmd.append("--%s=%s" % (name, value))
    if args:
        cmd.extend(args)
    ui.debug("calling '%s'...\n" % (" ".join(cmd)))
    try:
        return subprocess.call(cmd)
    except OSError as e:
        if e.errno == errno.ENOENT:
            raise error.Abort(
                _("'edenfsctl' not found"),
                hint=_(
                    "ensure edenfs is installed and its tools are available in the system path"
                ),
            )
        else:
            raise


@subcmd(
    "restart", [("", "graceful", None, _("perform a graceful restart"))], norepo=True
)
def restart(ui, **opts):
    """restart the edenfs daemon

    Run "@prog@ fs restart --graceful" to perform a graceful restart.  The
    new edenfs daemon will take over the existing edenfs mount points with
    minimal disruption to clients.  Open file handles will continue to work
    across the restart.
    """
    return _calledenfsctl(ui, "restart", opts=opts)


@subcmd("start", norepo=True)
def start(ui, **opts):
    """start the edenfs daemon"""
    return _calledenfsctl(ui, "start")


@subcmd("stop", norepo=True)
def stop(ui, **opts):
    """stop the edenfs daemon"""
    return _calledenfsctl(ui, "stop")
