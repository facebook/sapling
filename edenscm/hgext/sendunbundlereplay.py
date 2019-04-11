# sendunbundlereplay.py - send unbundlereplay wireproto command
#
# Copyright 2019-present Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import datetime

from edenscm.mercurial import error, hg, replay, util
from edenscm.mercurial.commands import command
from edenscm.mercurial.i18n import _


@command(
    "^sendunbundlereplay",
    [
        ("", "file", "", _("file to read bundle from"), ""),
        ("", "path", "", _("hg server remotepath (ssh)"), ""),
        ("r", "rebasedhead", "", _("expected rebased head hash"), ""),
        (
            "",
            "deleted",
            False,
            _("bookmark was deleted, can't be used with `--rebasedhead`"),
        ),
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
    deleted = opts["deleted"]
    ontobook = opts["ontobook"]
    if rebasedhead and deleted:
        raise error.Abort("can't use `--rebasedhead` and `--deleted`")

    if not (rebasedhead or deleted):
        raise error.Abort("either `--rebasedhead` or `--deleted` should be used")

    commitdates = dict(map(lambda s: s.split("="), ui.fin))
    with open(fname, "rb") as f:
        stream = util.chunkbuffer([f.read()])

    before = datetime.datetime.utcnow()
    remote = hg.peer(ui, {}, path)
    elapsed = (datetime.datetime.utcnow() - before).total_seconds()
    ui.note(_("creating a peer took: %r\n") % elapsed)

    before = datetime.datetime.utcnow()
    reply = remote.unbundlereplay(
        stream,
        ["force"],
        remote.url(),
        replay.ReplayData(commitdates, rebasedhead, ontobook),
        ui.configbool("sendunbundlereplay", "respondlightly", True),
    )
    elapsed = (datetime.datetime.utcnow() - before).total_seconds()
    ui.note(_("single wireproto command took: %r\n") % elapsed)

    returncode = 0
    for part in reply.iterparts():
        part.read()
        if part.type.startswith("error:"):
            returncode = 1
            ui.warn("%s\n" % part.type)
            if "message" in part.params:
                ui.warn("%s\n" % (part.params["message"]))

    return returncode
