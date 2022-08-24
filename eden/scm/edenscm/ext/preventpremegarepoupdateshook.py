# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import registrar


configtable = {}
configitem = registrar.configitem(configtable)

configitem(
    "preventpremegarepoupdates",
    "message",
    default=(
        "Checking out commits from before megarepo merge is discouraged. "
        "The resulting checkout will contain just the contents of one git subrepo. "
        "Many tools might not work as expected. "
        "Do you want to continue (Yn)?  $$ &Yes $$ &No"
    ),
)
configitem(
    "preventpremegarepoupdates",
    "dangerrevset",
    default="not(contains('.megarepo/remapping_state'))",
)


def reposetup(ui, repo):
    ui.setconfig(
        "hooks", "preupdate.preventpremegarepoupdates", preventpremegarepoupdates
    )


def preventpremegarepoupdates(ui, repo, **kwargs):
    """
    Prevents checkouts of commits comits from before the unified megarepo history.
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
