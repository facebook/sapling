# sendunbundlereplay.py - send unbundlereplay wireproto command
#
# Copyright 2019-present Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

from edenscm.mercurial import hg, replay, util
from edenscm.mercurial.commands import command
from edenscm.mercurial.i18n import _


@command(
    "^sendunbundlereplay",
    [
        ("", "file", "", _("file to read bundle from"), ""),
        ("", "path", "", _("hg server remotepath (ssh)"), ""),
        ("r", "rebasedhead", "", _("expected rebased head hash"), ""),
        ("b", "ontobook", "", _("expected onto bookmark for pushrebase"), ""),
    ],
    _("[OPTION]..."),
    norepo=True,
)
def sendunbundlereplay(ui, **opts):
    """Send unbundlereplay wireproto command to a given server

    Takes `rebasedhook` and `ontobook` arguments on the commmand
    line, and commit dates in stdin. The commit date format is:
    <commithash>=<hg-parseable-date>

    ``sendunbundlereplay.respondlightly`` config option instructs the server
    to avoid sending large bundle2 parts back.
    """
    fname = opts["file"]
    path = opts["path"]
    rebasedhead = opts["rebasedhead"]
    ontobook = opts["ontobook"]
    commitdates = dict(map(lambda s: s.split("="), ui.fin))
    with open(fname, "rb") as f:
        stream = util.chunkbuffer([f.read()])

    remote = hg.peer(ui, {}, path)
    reply = remote.unbundlereplay(
        stream,
        ["force"],
        remote.url(),
        replay.ReplayData(commitdates, rebasedhead, ontobook),
        ui.configbool("sendunbundlereplay", "respondlightly", True),
    )

    for part in reply.iterparts():
        part.read()
