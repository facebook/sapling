# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""client for the Mononoke API server

Configs:
    ``mononoke-api.enabled`` specifies whether HTTP data fetching should be used
    ``mononoke-api.url`` specifies the base URL of the API server

To set up TLS mutual authentication, add an entry to the [auth] section
matching the configured base URL:
    ``auth.mononoke-api.prefix``: base URL to match (without scheme)
    ``auth.mononoke-api.schemes``: URL scheme to match; should usually be "https".
    ``auth.mononoke-api.cert``: client certificate for TLS mutual authenticaton
    ``auth.mononoke-api.key``: client key for TLS mutual authentication

"""

from __future__ import absolute_import

from edenscm.mercurial import error, httpconnection, registrar
from edenscm.mercurial.i18n import _

from . import shallowutil


try:
    from edenscm.mercurial.rust.pymononokeapi import PyMononokeClient, GetFilesRequest
except ImportError:
    pass

configtable = {}
configitem = registrar.configitem(configtable)

configitem("mononoke-api", "enabled", default=False)


def getbaseurl(ui):
    """Get the base URL of the API server."""
    url = ui.config("mononoke-api", "url")
    if url is None:
        raise error.Abort(_("No Mononoke API server URL configured"))
    return url


def getcreds(ui, url):
    """Get the TLS mutual authentication credentials for the given URL."""
    res = httpconnection.readauthforuri(ui, url, None)
    if res is None:
        return None
    group, auth = res
    if "cert" not in auth or "key" not in auth:
        return None
    return (auth["cert"], auth["key"])


def initclient(ui, repo):
    """Initialize a new PyMononokeClient using the user's config."""
    try:
        PyMononokeClient
    except NameError:
        raise error.Abort(_("pymononokeapi rust extension is not loaded"))

    if not ui.configbool("mononoke-api", "enabled"):
        raise error.Abort(_("HTTP data fetching is not enabled for this repository"))

    url = getbaseurl(ui)
    creds = getcreds(ui, url)
    cachepath = shallowutil.getcachepath(ui)

    return PyMononokeClient(url, cachepath, repo.name, creds)


def healthcheck(ui, repo):
    """Perform a health check of the API server."""
    client = initclient(ui, repo)
    url = getbaseurl(ui)

    try:
        client.health_check()
        ui.write(_("successfully connected to: %s\n") % url)
    except RuntimeError as e:
        raise error.Abort(e)


def getfiles(ui, repo, keys):
    """Fetch files from the server and write them to a datapack."""
    client = initclient(ui, repo)
    req = GetFilesRequest()
    for (node, path) in keys:
        req.push(node, path)
    return client.get_files(req)
