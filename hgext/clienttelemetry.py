# Copyright 2018-present Facebook. All Rights Reserved.
#
# clienttelemetry: provide information about the client in server telemetry
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""provide information about the client in server telemetry

  [clienttelemetry]
  # whether or not to announce the remote hostname when connecting
  announceremotehostname = False
"""

from __future__ import absolute_import
import random
import socket
import string
import sys

from mercurial.i18n import _
from mercurial import (
    dispatch,
    extensions,
    hg,
    wireproto,
)

# Client telemetry functions generate client telemetry data at connection time.
_clienttelemetryfuncs = {}

def clienttelemetryfunc(f):
    """Decorator for registering client telemetry functions."""
    _clienttelemetryfuncs[f.__name__] = f
    return f

@clienttelemetryfunc
def fullcommand(ui):
    return ' '.join(sys.argv[1:])

@clienttelemetryfunc
def hostname(ui):
    return socket.gethostname()

@clienttelemetryfunc
def correlator(ui):
    """
    The correlator is a random string that is logged on both the client and
    server.  This can be used to correlate the client logging to the server
    logging.
    """
    alphabet = string.ascii_letters + string.digits
    corr = ''.join(random.choice(alphabet) for _x in range(16))
    ui.log('clienttelemetry', '', client_correlator=corr)
    return corr

# Client telemetry data is generated before connection and stored here.
_clienttelemetrydata = {}

def _clienttelemetry(repo, proto, args):
    """Handle received client telemetry"""
    logargs = {'client_%s' % key: value for key, value in args.items()}
    repo.ui.log('clienttelemetry', '', **logargs)
    return socket.gethostname()

def _capabilities(orig, repo, proto):
    result = orig(repo, proto)
    result.append('clienttelemetry')
    return result

def _runcommand(orig, ui, options, cmd, cmdfunc):
    # Record the command that is running in the client telemetry data.
    _clienttelemetrydata['command'] = cmd
    return orig(ui, options, cmd, cmdfunc)

def _peersetup(ui, peer):
    if peer.capable('clienttelemetry'):
        logargs = {name: f(ui) for name, f in _clienttelemetryfuncs.items()}
        logargs.update(_clienttelemetrydata)
        peername = peer._call('clienttelemetry', **logargs)
        ui.log('clienttelemetry', '', server_realhostname=peername)
        if ui.configbool('clienttelemetry', 'announceremotehostname', True):
            ui.status(_('connected to %s\n') % peername)

def uisetup(ui):
    wireproto.wireprotocommand('clienttelemetry', '*')(_clienttelemetry)
    extensions.wrapfunction(wireproto, '_capabilities', _capabilities)
    hg.wirepeersetupfuncs.append(_peersetup)
    extensions.wrapfunction(dispatch, '_runcommand', _runcommand)
