# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import error, registrar
from edenscm.mercurial.i18n import _

from . import createremote, restore, info

cmdtable = {}
command = registrar.command(cmdtable)


@command("snapshot", [], "SUBCOMMAND ...")
def snapshot(ui, repo, **opts):
    """create and share snapshots with uncommitted changes"""

    raise error.Abort(
        "you need to specify a subcommand (run with --help to see a list of subcommands)"
    )


subcmd = snapshot.subcommand(
    categories=[
        ("Manage snapshots", ["createremote", "restore"]),
        ("Query snapshots", ["info"]),
    ]
)


@subcmd("createremote|create", [])
def createremotecmd(*args, **kwargs):
    """upload to the server a snapshot of the current uncommitted changes"""
    createremote.createremote(*args, **kwargs)


@subcmd(
    "restore",
    [
        (
            "C",
            "clean",
            None,
            _("discard uncommitted changes and untracked files (no backup)"),
        )
    ],
    _("ID"),
)
def restorecmd(*args, **kwargs):
    """download a previously created snapshot and update working copy to its state"""
    restore.restore(*args, **kwargs)


@subcmd("info", [], _("ID"))
def infocmd(*args, **kwargs):
    """gather information about the snapshot"""
    info.info(*args, **kwargs)
