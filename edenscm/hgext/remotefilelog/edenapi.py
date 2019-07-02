# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import traceback

from edenscm.mercurial import error, httpconnection
from edenscm.mercurial.i18n import _
from edenscmnative.bindings import edenapi


try:
    xrange(0)
except NameError:
    xrange = range

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


def logexception(ui, exc):
    """Log an exception to Mercurial's telemetry and print a user warning."""
    exctype = type(exc).__name__
    excmsg = str(exc)

    if debug(ui):
        ui.warn("%s: %s\n" % (exctype, excmsg))

    ui.log(
        "edenapi_error",
        exception_msg=excmsg,
        exception_type=exctype,
        traceback=traceback.format_exc(),
    )


def _getbaseurl(ui):
    """Get the base URL of the API server."""
    url = ui.config("edenapi", "url")
    if url is None:
        raise error.Abort(_("No Eden API base URL configured"))
    return url


def _getcreds(ui, url):
    """Get the TLS mutual authentication credentials for the given URL."""
    res = httpconnection.readauthforuri(ui, url, None)
    if res is None:
        return None
    group, auth = res
    if "cert" not in auth or "key" not in auth:
        return None
    return (auth["cert"], auth["key"])


def _logconfig(ui):
    """Log various HTTPS fetching config values for debugging."""
    ui.log(
        "edenapi",
        "",
        http_data_batch_size=ui.configint("edenapi", "databatchsize"),
        http_history_batch_size=ui.configint("edenapi", "historybatchsize"),
        http_enabled=ui.configbool("edenapi", "enabled"),
    )


def _initclient(ui, repo):
    """Initialize a new Eden API client using the user's config."""
    _logconfig(ui)
    url = _getbaseurl(ui)
    kwargs = {
        "url": url,
        "repo": repo.name,
        "creds": _getcreds(ui, url),
        "databatchsize": ui.configint("edenapi", "databatchsize"),
        "historybatchsize": ui.configint("edenapi", "historybatchsize"),
        "validate": ui.configbool("edenapi", "validate"),
        "streamdata": ui.configbool("edenapi", "streamdata"),
        "streamhistory": ui.configbool("edenapi", "streamhistory"),
        "streamtrees": ui.configbool("edenapi", "streamtrees"),
    }
    return edenapi.client(**kwargs)


def _badcertwarning(ui):
    """Show the user a configurable message when their TLS certificate
       is missing, expired, or otherwise invalid.
    """
    msg = ui.config("edenapi", "authhelp")
    if msg is not None:
        ui.warn(msg + "\n")


def _tlswarning(ui):
    """Show the user a configurable message when a TLS error occurs
       during data fetching.
    """
    msg = ui.config("edenapi", "tlshelp")
    if msg is not None:
        ui.warn(msg + "\n")


def _warnexceptions(ui):
    """Decorator that catches certain exceptions defined by the Rust bindings
       and emits a user-friendly message before re-raising the exception.

       Although function is designed as a decorator, in practice it needs
       to be called manually rather than using decorator syntax, since it
       requires a ui object as an argument, which is typically not available
       outside of a function/method body.
    """

    def decorator(func):
        def wrapped(*args, **kwargs):
            try:
                return func(*args, **kwargs)
            except edenapi.CertificateError as e:
                _badcertwarning(ui)
                raise e
            except edenapi.TlsError as e:
                _tlswarning(ui)
                raise e

        return wrapped

    return decorator


def _retryonerror(ui, exctypes, maxtries=3):
    """Decorator retries the wrapped function if one of the
       listed exception types is raised.
    """

    def decorator(func):
        def wrapped(*args, **kwargs):
            for i in xrange(maxtries):
                try:
                    return func(*args, **kwargs)
                except Exception as e:
                    if type(e) not in exctypes or i == maxtries - 1:
                        raise e
                    if debug(ui):
                        ui.warn(_("retrying due to exception: "))
                    logexception(ui, e)

        return wrapped

    return decorator


class pyclient(object):
    def __init__(self, ui, repo):
        self._ui = ui
        self._rustclient = _warnexceptions(ui)(_initclient)(ui, repo)

    def __getattr__(self, name):
        method = getattr(self._rustclient, name)
        method = _retryonerror(self._ui, [edenapi.ProxyError])(method)
        method = _warnexceptions(self._ui)(method)
        return method
