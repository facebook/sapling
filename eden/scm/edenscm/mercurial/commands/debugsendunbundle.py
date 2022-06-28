# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sendunbundle.py - send unbundle wireproto command
from __future__ import absolute_import

from .. import error, hg, pycompat, util
from ..i18n import _
from .cmdtable import command


def getunbundlecontents():
    return util.chunkbuffer([pycompat.stdin.read()])


def rununbundle(ui, remote, stream):
    returncode = 0
    try:
        reply = remote.unbundle(stream, [b"force"], remote.url())
    except Exception as e:
        raise error.Abort("unbundle exception: %s" % (e,))

    for part in reply.iterparts():
        part.read()
        if part.type.startswith("error:"):
            returncode = 1
            ui.warn(_("unbundle failed, error part type: %s\n") % part.type)
            if "message" in part.params:
                ui.warn(_("part message: %s\n") % (part.params["message"]))
            if "params" in part.params:
                ui.warn(
                    _("part params: %s\n")
                    % (", ".join(part.params["params"].split("\0")))
                )
    return returncode


@command("debugsendunbundle", [], _("[OPTION]... [REMOTE]"), norepo=True)
def debugsendunbundle(ui, remote, **opts):
    """Send unbundle wireproto command to a given server"""
    stream = getunbundlecontents()
    remote = hg.peer(ui, {}, remote)
    return rununbundle(ui, remote, stream)
