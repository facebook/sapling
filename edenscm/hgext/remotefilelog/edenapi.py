# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import error, httpconnection
from edenscm.mercurial.i18n import _
from edenscm.mercurial.rust.bindings import edenapi

from . import shallowutil


# Set to True to manually disable HTTPS fetching.
_disabled = False


def enabled(ui):
    """Check whether HTTPS data fetching is enabled."""
    return not _disabled and ui.configbool("edenapi", "enabled")


def debug(ui):
    """Check whether HTTPS data fetching is in debug mode."""
    return ui.configbool("edenapi", "debug")


def bailifdisabled(ui):
    """Abort if HTTPS data fetching is disabled."""
    if not enabled(ui):
        raise error.Abort(_("HTTPS data fetching is disabled"))


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


def logconfig(ui):
    """Log various HTTPS fetching config values for debugging."""
    ui.log(
        "edenapi",
        "",
        http_data_batch_size=ui.configint("edenapi", "databatchsize"),
        http_history_batch_size=ui.configint("edenapi", "historybatchsize"),
        http_enabled=ui.configbool("edenapi", "enabled"),
    )


def initclient(ui, repo):
    """Initialize a new Eden API client using the user's config."""
    logconfig(ui)
    url = getbaseurl(ui)
    kwargs = {
        "url": url,
        "cachepath": shallowutil.getcachepath(ui),
        "repo": repo.name,
        "creds": getcreds(ui, url),
        "databatchsize": ui.configint("edenapi", "databatchsize"),
        "historybatchsize": ui.configint("edenapi", "historybatchsize"),
        "validate": ui.configbool("edenapi", "validate"),
    }
    return edenapi.client(**kwargs)
