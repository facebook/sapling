# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import error, httpconnection
from edenscm.mercurial.i18n import _
from edenscm.mercurial.rust.bindings import edenapi

from . import shallowutil


def enabled(ui):
    """Check whether HTTP data fetching is enabled."""
    return ui.configbool("edenapi", "enabled")


def bailifdisabled(ui):
    """Abort if HTTP data fetching is disabled."""
    if not enabled(ui):
        raise error.Abort(_("HTTP data fetching is disabled"))


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
    url = getbaseurl(ui)
    creds = getcreds(ui, url)
    cachepath = shallowutil.getcachepath(ui)
    backend = ui.config("edenapi", "backend")
    return edenapi.client(url, cachepath, repo.name, backend, creds)
