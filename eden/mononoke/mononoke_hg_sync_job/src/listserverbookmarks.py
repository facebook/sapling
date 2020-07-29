#!/usr/bin/env python2
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import hg, registrar
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "^listserverbookmarks",
    [("", "path", "", _("hg server remotepath (ssh)"), "")],
    _("[OPTION]..."),
    norepo=True,
)
def listserverbookmarks(ui, **opts):
    """List the bookmarks for a remote server"""
    path = opts["path"]
    remote = hg.peer(ui, {}, path)
    bookmarks = remote.listkeys("bookmarks")

    for pair in bookmarks.items():
        ui.write("%s\1%s\0" % pair)
    ui.flush()
