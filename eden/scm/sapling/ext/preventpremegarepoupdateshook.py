# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import registrar


configtable = {}
configitem = registrar.configitem(configtable)


def reposetup(ui, repo):
    ui.setconfig(
        "hooks", "preupdate.preventpremegarepoupdates", preventpremegarepoupdates
    )


def preventpremegarepoupdates(ui, repo, **kwargs):
    """
    Prevents checkouts of commits commits from before the unified megarepo history.
    Those commits might be confusing as they reflect only single subrepo.
    """

    if ui.plain():
        # When you've set `HGPLAIN`, we trust you to know what you're doing
        return False

    revset = "id('{}') and ({})".format(
        kwargs["parent1"], ui.config("preventpremegarepoupdates", "dangerrevset")
    )
    isbadrev = repo.anyrevs([revset])
    message = ui.config("preventpremegarepoupdates", "message")

    if isbadrev:
        return ui.promptchoice(message)

    # False means success
    return False
