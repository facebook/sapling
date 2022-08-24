#!/usr/bin/env python2
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

try:
    from edenscm.mercurial import hg, registrar
    from edenscm.mercurial.i18n import _
except ImportError:
    from edenscm import hg, registrar
    from edenscm.i18n import _


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
