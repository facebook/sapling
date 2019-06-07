# commands/fs.py - commands for controlling the edenfs daemon
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import subprocess

from .. import cmdutil, error
from ..i18n import _
from .cmdtable import command


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
    "check|fsck",
    [
        (
            "",
            "force",
            None,
            _("force a check even on checkouts that appear to be mounted"),
        )
    ]
    + cmdutil.dryrunopts,
)
def check(ui, repo, **opts):
    """check the filesystem of a virtual checkout"""
    args = [repo.root]
    if ui.verbose:
        args.append("--verbose")
    if opts["dry_run"]:
        # edenfsctl uses a different name for --dry-run
        args.append("--check-only")
        opts["dry_run"] = False
    return _calledenfsctl(ui, "fsck", args, opts=opts)


@subcmd("chown", [], "UID GID")
def chown(ui, repo, uid, gid):
    """change the ownership of a virtual checkout

    Reassigns ownership of a virtual checkout to the specified user and group.
    """
    return _calledenfsctl(ui, "chown", [repo.root, uid, gid])


@subcmd("config", [], norepo=True)
def config(ui):
    """show the edenfs daemon configuration"""
    return _calledenfsctl(ui, "config")


@subcmd(
    "doctor",
    [("n", "dry-run", None, _("do not try to fix any issues, only report them"))],
    norepo=True,
)
def doctor(ui, **opts):
    """debug and fix issues with the edenfs daemon"""
    return _calledenfsctl(ui, "doctor", opts=opts)


@subcmd("gc", [], norepo=True)
def gc(ui, **opts):
    """minimize disk and memory usage by freeing caches"""
    return _calledenfsctl(ui, "gc")


@subcmd("info", [])
def info(ui, repo, **opts):
    """show details about the virtual checkout"""
    return _calledenfsctl(ui, "info", [repo.root])


@subcmd("list", [("", "json", None, _("list checkouts in JSON format"))], norepo=True)
def list_(ui, **opts):
    """list available virtual checkouts"""
    return _calledenfsctl(ui, "list", opts=opts)


@subcmd(
    "prefetch",
    [
        (
            "",
            "pattern-file",
            "",
            _("specify a file that lists patterns or files to match, one per line"),
            _("FILE"),
        ),
        ("", "silent", None, _("do not print the names of the matching files")),
        ("", "no-prefetch", None, _("do not prefetch, just match the names")),
    ],
    _("[PATTERN]..."),
)
def prefetch(ui, repo, *patterns, **opts):
    """prefetch content for files"""
    return _calledenfsctl(
        ui, "prefetch", ["--repo", repo.root] + list(patterns), opts=opts
    )


@subcmd(
    "remove|rm",
    [
        (
            "y",
            "yes",
            None,
            _("do not prompt for confirmation before removing the checkout"),
        )
    ],
)
def remove(ui, repo, **opts):
    """remove a virtual checkout"""
    return _calledenfsctl(ui, "remove", [repo.root], opts=opts)


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


@subcmd("stats", subonly=True, norepo=True)
def stats(ui, **opts):
    """print statistics for the edenfs daemon"""


statscmd = stats.subcommand()


@statscmd(
    "io", [("A", "all", None, "show status for all the system calls")], norepo=True
)
def statsio(ui, **opts):
    """show information about the number of I/O calls"""
    return _calledenfsctl(ui, "stats io", opts=opts)


@statscmd(
    "latency", [("A", "all", None, "show status for all the system calls")], norepo=True
)
def statslatency(ui, **opts):
    """show information about the latency of I/O calls"""
    return _calledenfsctl(ui, "stats latency", opts=opts)


@statscmd("memory", [], norepo=True)
def statsmemory(ui, **opts):
    """show memory statistics for the edenfs daemon"""
    return _calledenfsctl(ui, "stats memory")


@statscmd("thrift", [], norepo=True)
def statsthrift(ui, **opts):
    """show the number of received thrift calls"""
    return _calledenfsctl(ui, "stats thrift")


@statscmd("thriftlatency|thrift-latency", [], norepo=True)
def statsthriftlatency(ui, **opts):
    """show the latency of received thrift calls"""
    return _calledenfsctl(ui, "stats thrift-latency")


@subcmd("status", norepo=True)
def status(ui, **opts):
    """check the health of the edenfs daemon"""
    return _calledenfsctl(ui, "status")


@subcmd("stop", norepo=True)
def stop(ui, **opts):
    """stop the edenfs daemon"""
    return _calledenfsctl(ui, "stop")


@subcmd("top", norepo=True)
def top(ui, **opts):
    """monitor virtual checkout accesses by process"""
    return _calledenfsctl(ui, "top")


@subcmd("version", norepo=True)
def version(ui, **opts):
    """show version information for the edenfs daemon"""
    return _calledenfsctl(ui, "version")
