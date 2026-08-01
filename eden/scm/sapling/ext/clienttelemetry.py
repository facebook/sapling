# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# clienttelemetry: provide information about the client in server telemetry
"""provide information about the client in server telemetry"""

import socket
from typing import Any, Dict

from sapling import extensions, wireproto


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


def uisetup(ui) -> None:
    wireproto.wireprotocommand("clienttelemetry", "*")(_clienttelemetry)
    extensions.wrapfunction(wireproto, "_capabilities", _capabilities)
