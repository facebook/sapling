# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import registrar, scmutil


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
    "preventpremegarepoupdates", "statefile", default=".megarepo/remapping_state"
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

    if kwargs["parent1"] == "000000000000":
        # scmutil.revsingle(repo, "000000000000") complains that such changeset
        # doesn't exist
        return False

    ctx = scmutil.revsingle(repo, kwargs["parent1"])
    statusfile = ui.config("preventpremegarepoupdates", "statefile")
    message = ui.config("preventpremegarepoupdates", "message")

    if statusfile not in ctx.manifest():
        return ui.promptchoice(message)

    # False means success
    return False
