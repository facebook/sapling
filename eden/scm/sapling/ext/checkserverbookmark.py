# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# checkserverbookmark.py - check whether the bookmark is where we expect
# it to be on a server


from sapling import error, hg
from sapling.commands import command
from sapling.i18n import _
from sapling.node import hex


def getremote(ui, path):
    remote = hg.peer(ui, {}, path)
    return remote


def runlookup(ui, remote, name):
    return remote.lookup(name)


def runlistkeys(ui, remote):
    return remote.listkeys("bookmarks")


def verifyexisting(ui, remote, name, hash) -> int:
    location = hex(runlookup(ui, remote, name))
    if location.strip() != hash.strip():
        ui.warn(
            _(
                "@prog@ server does not have an expected bookmark location. "
                + "book: %s, server: %s; expected %s\n"
            )
            % (name, location, hash)
        )
        return 1
    ui.warn(
        _("@prog@ server has expected bookmark location. book: %s, hash: %s\n")
        % (name, hash)
    )
    return 0


def verifydeleted(ui, remote, name) -> int:
    serverkeys = runlistkeys(ui, remote)
    if name in serverkeys:
        ui.warn(
            _(
                "@prog@ server has bookmark, which is expected to have been deleted: %s\n"
            )
            % (name,)
        )
        return 1
    ui.warn(_("@prog@ server expectedly does not have a bookmark: %s\n") % (name,))
    return 0


@command(
    "checkserverbookmark",
    [
        ("", "path", "", _("@prog@ server remotepath (ssh)"), ""),
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
def checkserverbookmark(ui, **opts) -> int:
    """Verify whether the bookmark on @prog@ server points to a given hash"""
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
