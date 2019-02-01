# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""client for the Mononoke API server

Configs:
    ``mononoke-api.enabled`` specifies whether the Mononoke API should be used for this repo
    ``mononoke-api.host`` specifies the URI prefix of the Mononoke API server
    ``mononoke-api.creds`` specifies a PEM file containing TLS credentials for the API server
"""

from __future__ import absolute_import

from edenscm.mercurial import error, registrar, util
from edenscm.mercurial.i18n import _

from . import shallowutil


try:
    from edenscm.mercurial.rust.pymononokeapi import PyMononokeClient
except ImportError:
    pass

configtable = {}
configitem = registrar.configitem(configtable)

configitem("mononoke-api", "enabled", default=False)
configitem("mononoke-api", "host", default=None)
configitem("mononoke-api", "creds", default=None)


def setupclient(ui, repo):
    try:
        PyMononokeClient
    except NameError:
        raise error.Abort(_("pymononokeapi rust extension is not loaded"))

    if not ui.configbool("mononoke-api", "enabled"):
        raise error.Abort(_("Mononoke API is not enabled for this repository"))

    host = ui.config("mononoke-api", "host")
    if host is None:
        raise error.Abort(_("No Mononoke API server host configured"))

    creds = ui.config("mononoke-api", "creds")
    if creds is not None:
        creds = util.expandpath(creds)

    cachepath = shallowutil.getcachepath(ui)

    return PyMononokeClient(host, cachepath, repo.name, creds)


def healthcheck(ui, repo):
    host = ui.config("mononoke-api", "host")
    client = setupclient(ui, repo)

    try:
        client.health_check()
        ui.write(_("successfully connected to: %s\n") % host)
    except RuntimeError as e:
        raise error.Abort(e)


def getfile(ui, repo, node, path):
    client = setupclient(ui, repo)
    return client.get_file(node, path)
