# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import threading

from bindings import edenapi

from . import error
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
        method = _spawnthread(method)
        return method


def _spawnthread(func):
    """Decorator that spawns a new Python thread to run the wrapped function.

    This is useful for FFI calls to allow the Python interpreter to handle
    signals during the FFI call. For example, without this it would not be
    possible to interrupt the process with Ctrl-C during a long-running FFI
    call.
    """

    def wrapped(*args, **kwargs):
        result = ["err", error.Abort(_("thread aborted unexpectedly"))]

        def target(*args, **kwargs):
            try:
                result[:] = ["ok", func(*args, **kwargs)]
            except Exception as e:
                result[:] = ["err", e]

        thread = threading.Thread(target=target, args=args, kwargs=kwargs)
        thread.start()

        # XXX: Need to repeatedly poll the thread because blocking
        # indefinitely on join() would prevent the interpreter from
        # handling signals.
        while thread.is_alive():
            thread.join(1)

        variant, value = result
        if variant == "err":
            raise value

        return value

    return wrapped


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
