# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# clienttelemetry: provide information about the client in server telemetry
"""provide information about the client in server telemetry

[clienttelemetry]
# whether or not to announce the remote hostname when connecting
announceremotehostname = False
"""

import socket
from typing import Any, Dict

from sapling import blackbox, dispatch, extensions, hg, perftrace, util, wireproto
from sapling.i18n import _

# Client telemetry functions generate client telemetry data at connection time.
_clienttelemetryfuncs = {}


def clienttelemetryfunc(f):
    """Decorator for registering client telemetry functions."""
    _clienttelemetryfuncs[f.__name__] = f
    return f


@clienttelemetryfunc
def hostname(ui) -> str:
    return socket.gethostname()


@clienttelemetryfunc
def wantslfspointers(ui):
    """
    Tells the server whether this clients wants LFS pointers to be sent in
    getpackv2. Only applies when the repository is being migrated to sending
    LFS pointers and doesn't apply on repositories already converted.

    Oh, if you haven't realized already, this is a hack. Hopefully when the
    Mercurial servers are gone we'll be able to have a real capability exchange
    system when establishing a connection.
    """

    return str(ui.configbool("lfs", "wantslfspointers"))


@clienttelemetryfunc
def wantsunhydratedcommits(ui):
    """
    Tells the server whether this clients wants unhydrated draft commits
    """

    return str(ui.configbool("infinitepush", "wantsunhydratedcommits"))


# Client telemetry data is generated before connection and stored here.
_clienttelemetrydata = {}


def _clienttelemetry(repo, proto, args) -> str:
    """Handle received client telemetry"""
    logargs = {"client_%s" % key: value for key, value in args.items()}
    repo.ui.log("clienttelemetry", **logargs)
    # Make them available to other extensions
    repo.clienttelemetry = logargs
    return socket.gethostname()


def getclienttelemetry(repo) -> Dict[str, Any]:
    kwargs = {}
    if hasattr(repo, "clienttelemetry"):
        clienttelemetry = repo.clienttelemetry
        fields = ["client_fullcommand", "client_hostname"]
        for f in fields:
            if f in clienttelemetry:
                kwargs[f] = clienttelemetry[f]
    return kwargs


def _capabilities(orig, repo, proto):
    result = orig(repo, proto)
    result.append("clienttelemetry")
    return result


def clienttelemetryvaluesfromconfig(ui):
    result = {}
    for name, value in ui.configitems("clienttelemetryvalues"):
        result[name] = value
    return result


def _runcommand(orig, lui, repo, cmd, fullargs, *args):
    # Record the command that is running in the client telemetry data.
    _clienttelemetrydata["command"] = cmd

    fullcommand = dispatch._formatargs(fullargs)
    # Long invocations can occupy a lot of space in the logs.
    if len(fullcommand) > 256:
        fullcommand = fullcommand[:256] + " (truncated)"

    _clienttelemetrydata["fullcommand"] = fullcommand
    return orig(lui, repo, cmd, fullargs, *args)


def _peersetup(ui, peer) -> None:
    if peer.capable("clienttelemetry"):
        logargs = clienttelemetryvaluesfromconfig(ui)
        logargs.update({name: f(ui) for name, f in _clienttelemetryfuncs.items()})
        logargs.update(_clienttelemetrydata)
        response = peer._call("clienttelemetry", **logargs).decode()
        responseitems = response.split()
        peername = responseitems[0] if responseitems else ""
        peer._realhostname = peername
        peerinfo = {}
        for index in range(1, len(responseitems) - 1, 2):
            peerinfo[responseitems[index]] = responseitems[index + 1]
        peer._peerinfo = peerinfo
        blackbox.log({"clienttelemetry": {"peername": peername, "peerinfo": peerinfo}})
        util.info("client-telemetry", peername=peername, **peerinfo)
        ann = ui.configbool("clienttelemetry", "announceremotehostname", False)
        if ann is None:
            ann = not ui.plain() and ui._isatty(ui.ferr)
        if ann and not ui.quiet:
            ui.write_err(_("connected to %s\n") % response)
            perftrace.tracevalue("Server", peername)
            for item, value in peerinfo.items():
                perftrace.tracevalue(f"Server {item}", value)


def uisetup(ui) -> None:
    wireproto.wireprotocommand("clienttelemetry", "*")(_clienttelemetry)
    extensions.wrapfunction(wireproto, "_capabilities", _capabilities)
    hg.wirepeersetupfuncs.append(_peersetup)
    extensions.wrapfunction(dispatch, "runcommand", _runcommand)
