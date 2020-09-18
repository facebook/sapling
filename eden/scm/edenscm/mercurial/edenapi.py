# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from bindings import edenapi

from . import util
from .i18n import _


class pyclient(object):
    """Wrapper around an EdenAPI client from Mercurial's Rust bindings.

    The primary purpose of this class is user-friendliness. It provides correct
    handling of SIGINT and prints out configurable user-friendly error messages.
    """

    def __init__(self, ui):
        self._ui = ui
        self._rustclient = _warnexceptions(ui)(edenapi.client)(ui._rcfg._rcfg)

    def __getattr__(self, name):
        method = getattr(self._rustclient, name)
        method = _warnexceptions(self._ui)(method)
        method = util.threaded(method)
        return method


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
                _printhelp(ui, "tlsauthhelp")
                raise e
            except edenapi.TlsError as e:
                _printhelp(ui, "tlshelp")
                raise e

        return wrapped

    return decorator


def _printhelp(ui, msgname):
    """Print a help message defined in the [help] config section."""
    msg = ui.config("help", msgname)
    if msg is not None:
        ui.warn(msg + "\n")
