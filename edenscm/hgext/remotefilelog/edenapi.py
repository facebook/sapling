# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

""""client for the Eden HTTP API

Configs:
    ``edenapi.enabled`` specifies whether HTTP data fetching should be used
    ``edenapi.url`` specifies the base URL of the API server

To set up TLS mutual authentication, add an entry to the [auth] section
matching the configured base URL:
    ``auth.edenapi.prefix``: base URL to match (without scheme)
    ``auth.edenapi.schemes``: URL scheme to match; should usually be "https".
    ``auth.edenapi.cert``: client certificate for TLS mutual authenticaton
    ``auth.edenapi.key``: client key for TLS mutual authentication

"""

from __future__ import absolute_import

from edenscm.mercurial import error, httpconnection, registrar
from edenscm.mercurial.i18n import _
from edenscm.mercurial.rust.bindings import edenapi

from . import shallowutil


configtable = {}
configitem = registrar.configitem(configtable)

configitem("edenapi", "enabled", default=False)
configitem("edenapi", "url", default=None)


def getbaseurl(ui):
    """Get the base URL of the API server."""
    url = ui.config("edenapi", "url")
    if url is None:
        raise error.Abort(_("No Eden API base URL configured"))
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
    """Initialize a new Eden API client using the user's config."""
    if not ui.configbool("edenapi", "enabled"):
        raise error.Abort(_("HTTP data fetching is not enabled for this repository"))

    url = getbaseurl(ui)
    creds = getcreds(ui, url)
    cachepath = shallowutil.getcachepath(ui)

    return edenapi.client(url, cachepath, repo.name, creds)


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
    req = edenapi.getfilesrequest()
    for (node, path) in keys:
        req.push(node, path)
    return client.get_files(req)
