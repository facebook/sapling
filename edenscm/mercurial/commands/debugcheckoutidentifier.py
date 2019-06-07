# Copyright 2013 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .cmdtable import command


@command("debugcheckoutidentifier", [])
def checkoutidentifier(ui, repo, **opts):
    """display the current checkout unique identifier

    This is a random string that was generated when the commit was checked out.
    It can be logged during commands that operate on a checkout to correlate
    them with other commands that operate on the same checkout in metrics and
    telemetry, as well as any commit that is eventually created from that
    checkout.
    """
    ui.write("%s\n" % repo.dirstate.checkoutidentifier)
    return 0
