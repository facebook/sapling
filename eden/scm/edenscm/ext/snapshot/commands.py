# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import error, registrar
from edenscm.i18n import _

from . import createremote, isworkingcopy, labels, latest, show, update

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
        ("Manage snapshots", ["create", "update", "add-labels", "remove-labels"]),
        ("Query snapshots", ["show"]),
    ]
)


@subcmd(
    "createremote|create",
    [
        (
            "L",
            "lifetime",
            "",
            _(
                "how long the snapshot should last for, seconds to days supported (e.g. 60s, 90d, 1h30m)"
            ),
            _("LIFETIME"),
        ),
        (
            "",
            "labels",
            "",
            _(
                "comma-separated list of named labels to be associated with the snapshot. Named snapshots will not expire"
            ),
            _("LABELS"),
        ),
        (
            "",
            "max-untracked-size",
            "1000",
            _("filter out any untracked files larger than this size, in megabytes"),
            _("MAX_SIZE"),
        ),
        (
            "",
            "max-file-count",
            1000,
            _("maximum allowed total number of files in a snapshot"),
            _("MAX_FILE_COUNT"),
        ),
        (
            "",
            "reuse-storage",
            None,
            _(
                "reuse same storage as latest snapshot, if possible; its lifetime won't be extended"
            ),
        ),
    ],
)
def createremotecmd(*args, **kwargs) -> None:
    """
    upload to the server a snapshot of the current uncommitted changes.

    exits with code 2 if the file count in the snapshot will exceed max-file-count.
    """
    createremote.createremote(*args, **kwargs)


@subcmd(
    "update|restore|checkout|co|up",
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
def updatecmd(*args, **kwargs) -> None:
    """download a previously created snapshot and update working copy to its state"""
    update.update(*args, **kwargs)


@subcmd(
    "show|info",
    [
        ("", "json", None, _("output in json format instead of human-readable")),
        ("", "stat", None, _("output diffstat-style summary of changes")),
    ],
    _("ID"),
)
def showcmd(*args, **kwargs) -> None:
    """gather information about the snapshot"""
    show.show(*args, **kwargs)


@subcmd(
    "isworkingcopy",
    [
        (
            "",
            "max-untracked-size",
            "",
            _("filter out any untracked files larger than this size, in megabytes"),
            _("MAX_SIZE"),
        ),
    ],
    _("ID"),
)
def isworkingcopycmd(*args, **kwargs) -> None:
    """test if a given snapshot is the working copy"""
    isworkingcopy.cmd(*args, **kwargs)


@subcmd(
    "latest",
    [
        (
            "",
            "is-working-copy",
            None,
            _("fails if there have been local changes since the latest snapshot"),
        ),
        (
            "",
            "max-untracked-size",
            "",
            _("filter out any untracked files larger than this size, in megabytes"),
            _("MAX_SIZE"),
        ),
    ],
)
def latestcmd(*args, **kwargs) -> None:
    """information regarding the latest created/restored snapshot"""
    latest.latest(*args, **kwargs)


@subcmd(
    "add-labels",
    [
        (
            "",
            "labels",
            "",
            _("comma-separated list of named labels to be added to the snapshot"),
        ),
    ],
    _("ID"),
)
def add_labels(*args, **kwargs) -> None:
    """Associate new labels with an existing snapshot"""
    labels.add_labels(*args, **kwargs)


@subcmd(
    "remove-labels",
    [
        (
            "",
            "labels",
            "",
            _(
                "comma-separated list of named labels to be removed from the snapshot. Cannot be used with --all"
            ),
        ),
        (
            "",
            "all",
            False,
            _(
                "flag representing if all the labels associated with the snapshot need to be removed. Cannot be used with --labels"
            ),
        ),
    ],
    _("ID"),
)
def remove_labels(*args, **kwargs) -> None:
    """Remove associated labels from an existing snapshot"""
    labels.remove_labels(*args, **kwargs)
