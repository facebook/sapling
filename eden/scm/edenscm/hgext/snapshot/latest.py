# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial.i18n import _

from .metalog import fetchlatestsnapshot


def latest(ui, repo, **opts):
    csid = fetchlatestsnapshot(repo.metalog())
    if csid is None:
        if not ui.plain():
            ui.status(_("no snapshot found\n"))
    else:
        csid = csid.hex()
        if ui.plain():
            ui.status(f"{csid}\n")
        else:
            ui.status(_("latest snapshot is {}\n").format(csid))
