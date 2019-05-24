# checkserverbookmark.py - check whether the bookmark is where we expect
# it to be on a server
#
# Copyright 2019-present Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import datetime

from edenscm.mercurial import error, hg
from edenscm.mercurial.commands import command
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex


def getremote(ui, path):
    before = datetime.datetime.utcnow()
    remote = hg.peer(ui, {}, path)
    elapsed = (datetime.datetime.utcnow() - before).total_seconds()
    ui.warn(_("creating a peer took: %r\n") % elapsed)
    return remote


def runlookup(ui, remote, name):
    before = datetime.datetime.utcnow()
    result = ""
    try:
        result = remote.lookup(name)
    finally:
        elapsed = (datetime.datetime.utcnow() - before).total_seconds()
        ui.warn(_("running lookup took: %r\n") % elapsed)
    return result


def runlistkeys(ui, remote):
    before = datetime.datetime.utcnow()
    result = {}
    try:
        result = remote.listkeys("bookmarks")
    finally:
        elapsed = (datetime.datetime.utcnow() - before).total_seconds()
        ui.warn(_("running listkeys took: %r\n") % elapsed)
    return result


def verifyexisting(ui, remote, name, hash):
    location = hex(runlookup(ui, remote, name))
    if location.strip() != hash.strip():
        ui.warn(
            _(
                "hg server does not have an expected bookmark location. "
                + "book: %s, server: %s; expected %s\n"
            )
            % (name, location, hash)
        )
        return 1
    ui.warn(
        _("hg server has expected bookmark location. book: %s, hash: %s\n")
        % (name, hash)
    )
    return 0


def verifydeleted(ui, remote, name):
    serverkeys = runlistkeys(ui, remote)
    if name in serverkeys:
        ui.warn(
            _("hg server has bookmark, which is expected to have been deleted: %s\n")
            % (name,)
        )
        return 1
    ui.warn(_("hg server expectedly does not have a bookmark: %s\n") % (name,))
    return 0


@command(
    "^checkserverbookmark",
    [
        ("", "path", "", _("hg server remotepath (ssh)"), ""),
        ("", "name", "", _("bookmark name to check"), ""),
        ("", "hash", "", _("hash to verify against the bookmark"), ""),
        (
            "",
            "deleted",
            False,
            _("bookmark is expected to not exist, cannot be used with `--hash`"),
        ),
    ],
    _("[OPTION]..."),
    norepo=True,
)
def checkserverbookmark(ui, **opts):
    """Verify whether the bookmark on hg server points to a given hash"""
    name = opts["name"]
    path = opts["path"]
    hash = opts["hash"]
    deleted = opts["deleted"]
    if hash and deleted:
        raise error.Abort("can't use `--hash` and `--deleted`")

    if not (hash or deleted):
        raise error.Abort("either `--hash` or `--deleted` should be used")

    remote = getremote(ui, path)
    if deleted:
        return verifydeleted(ui, remote, name)
    else:
        return verifyexisting(ui, remote, name, hash)
