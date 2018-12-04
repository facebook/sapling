# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import error, registrar, util
from mercurial.i18n import _

from ..extlib.pymononokeapi import PyMononokeClient


configtable = {}
configitem = registrar.configitem(configtable)

configitem("mononoke-api", "enabled", default=False)
configitem("mononoke-api", "host", default=None)
configitem("mononoke-api", "creds", default=None)


def get_client(ui):
    if not ui.configbool("mononoke-api", "enabled"):
        raise error.Abort(_("Mononoke API is not enabled for this repository"))

    host = ui.config("mononoke-api", "host")
    if host is None:
        raise error.Abort(_("No Mononoke API server host configured"))

    creds = ui.config("mononoke-api", "creds")
    if creds is not None:
        creds = util.expandpath(creds)
    if creds is None:
        raise error.Abort(_("No Mononoke API server TLS credentials configured"))

    return PyMononokeClient(host, creds)


def health_check(ui):
    client = get_client(ui)
    try:
        client.health_check()
        ui.write("success\n")
    except RuntimeError as e:
        raise error.Abort(e)
