# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# clienttelemetry: provide information about the client in server telemetry
"""provide information about the client in server telemetry

  [clienttelemetry]
  # whether or not to announce the remote hostname when connecting
  announceremotehostname = False
"""

from __future__ import absolute_import

import socket
import string

from edenscm.mercurial import (
    blackbox,
    dispatch,
    extensions,
    hg,
    perftrace,
    util,
    wireproto,
)
from edenscm.mercurial.i18n import _


# Client telemetry functions generate client telemetry data at connection time.
_clienttelemetryfuncs = {}


def clienttelemetryfunc(f):
    """Decorator for registering client telemetry functions."""
    _clienttelemetryfuncs[f.__name__] = f
    return f


@clienttelemetryfunc
def hostname(ui):
    return socket.gethostname()


_correlator = None


@clienttelemetryfunc
def correlator(ui):
    """
    The correlator is a random string that is logged on both the client and
    server.  This can be used to correlate the client logging to the server
    logging.
    """
    global _correlator
    if _correlator is None:
        _correlator = util.makerandomidentifier()
        ui.log("clienttelemetry", client_correlator=_correlator)
    return _correlator


# Client telemetry data is generated before connection and stored here.
_clienttelemetrydata = {}


def _clienttelemetry(repo, proto, args):
    """Handle received client telemetry"""
    logargs = {"client_%s" % key: value for key, value in args.items()}
    repo.ui.log("clienttelemetry", **logargs)
    # Make them available to other extensions
    repo.clienttelemetry = logargs
    return socket.gethostname()


def getclienttelemetry(repo):
    kwargs = {}
    if util.safehasattr(repo, "clienttelemetry"):
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


def _runcommand(orig, lui, repo, cmd, fullargs, ui, options, d, cmdpats, cmdoptions):
    # Record the command that is running in the client telemetry data.
    _clienttelemetrydata["command"] = cmd
    _clienttelemetrydata["fullcommand"] = dispatch._formatargs(fullargs)
    return orig(lui, repo, cmd, fullargs, ui, options, d, cmdpats, cmdoptions)


def _peersetup(ui, peer):
    if peer.capable("clienttelemetry"):
        logargs = {name: f(ui) for name, f in _clienttelemetryfuncs.items()}
        logargs.update(_clienttelemetrydata)
        peername = peer._call("clienttelemetry", **logargs)
        ui.log("clienttelemetry", server_realhostname=peername)
        blackbox.log({"clienttelemetry": {"peername": peername}})
        ann = ui.configbool("clienttelemetry", "announceremotehostname", None)
        if ann is None:
            ann = not ui.plain() and ui._isatty(ui.ferr)
        if ann and not ui.quiet:
            ui.warn(_("connected to %s\n") % peername)
            perftrace.tracevalue("Server", peername)


def uisetup(ui):
    wireproto.wireprotocommand("clienttelemetry", "*")(_clienttelemetry)
    extensions.wrapfunction(wireproto, "_capabilities", _capabilities)
    hg.wirepeersetupfuncs.append(_peersetup)
    extensions.wrapfunction(dispatch, "runcommand", _runcommand)
