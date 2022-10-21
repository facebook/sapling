# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sshpeer.py - ssh repository proxy class for mercurial
#
# Copyright 2005, 2006 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import abc
from typing import Any, Optional

from . import error, perftrace, pycompat, util, wireproto
from .i18n import _
from .pycompat import decodeutf8, encodeutf8


def _writestderror(ui: "Any", s: bytes) -> None:
    if s and not ui.quiet:
        for l in s.splitlines():
            if l.startswith(b"ssh:"):
                prefix = ""
            else:
                prefix = _("remote: ")
            ui.write_err(prefix, decodeutf8(l, errors="replace"), "\n")


class stdiopeer(wireproto.wirepeer):
    __metaclass__ = abc.ABCMeta

    def __init__(self, ui, path, create=False, initial_config: Optional[str] = None):
        self._url = path
        self._ui = ui
        self._pipeo = self._pipei = None
        self._caps = set()

    # Begin of _basepeer interface.

    @util.propertycache
    def ui(self):
        return self._ui

    def url(self):
        return self._url

    def local(self):
        return None

    def peer(self):
        return self

    def canpush(self):
        return True

    def close(self):
        self._cleanup()

    # End of _basepeer interface.

    # Begin of _basewirecommands interface.

    def capabilities(self):
        return self._caps

    # End of _basewirecommands interface.

    def _abort(self, exception):
        self._cleanup()
        raise exception

    @abc.abstractmethod
    def _cleanup(self):
        return

    def _submitbatch(self, req):
        rsp = self._callstream("batch", cmds=wireproto.encodebatchcmds(req))
        available = self._getamount()
        # TODO this response parsing is probably suboptimal for large
        # batches with large responses.
        toread = min(available, 1024)
        work = rsp.read(toread)
        available -= toread
        chunk = work
        while chunk:
            while b";" in work:
                one, work = work.split(b";", 1)
                yield wireproto.unescapebytearg(one)
            toread = min(available, 1024)
            chunk = rsp.read(toread)
            available -= toread
            work += chunk
        yield wireproto.unescapebytearg(work)

    def _callstream(self, cmd, **args):
        args = args
        self.ui.debug("sending %s command\n" % cmd)
        self._pipeo.write(encodeutf8("%s\n" % cmd))
        _func, names = wireproto.commands[cmd]
        keys = names.split()
        wireargs = {}
        for k in keys:
            if k == "*":
                wireargs["*"] = args
                break
            else:
                wireargs[k] = args[k]
                del args[k]
        for k, v in sorted(pycompat.iteritems(wireargs)):
            k = encodeutf8(k)
            if isinstance(v, str):
                v = encodeutf8(v)
            self._pipeo.write(b"%s %d\n" % (k, len(v)))
            if isinstance(v, dict):
                for dk, dv in pycompat.iteritems(v):
                    if isinstance(dk, str):
                        dk = encodeutf8(dk)
                    if isinstance(dv, str):
                        dv = encodeutf8(dv)
                    self._pipeo.write(b"%s %d\n" % (dk, len(dv)))
                    self._pipeo.write(dv)
            else:
                self._pipeo.write(v)
        self._pipeo.flush()

        return self._pipei

    def _callcompressable(self, cmd, **args):
        return self._callstream(cmd, **args)

    def _call(self, cmd, **args):
        self._callstream(cmd, **args)
        return self._recv()

    def _callpush(self, cmd, fp, **args):
        r = self._call(cmd, **args)
        if r:
            return b"", r
        for d in iter(lambda: fp.read(4096), b""):
            self._send(d)
        self._send(b"", flush=True)
        r = self._recv()
        if r:
            return b"", r
        return self._recv(), b""

    def _calltwowaystream(self, cmd, fp, **args):
        r = self._call(cmd, **args)
        if r:
            # XXX needs to be made better
            raise error.Abort(_("unexpected remote reply: %s") % r)
        payloadsize = 0
        for d in iter(lambda: fp.read(4096), b""):
            payloadsize += len(d)
            self._send(d)
        self._send(b"", flush=True)
        perftrace.tracebytes("two-way stream payload size", payloadsize)
        return self._pipei

    def _getamount(self):
        l = self._pipei.readline()
        if l == "\n":
            msg = _("check previous remote output")
            self._abort(error.OutOfBandError(hint=msg))
        try:
            return int(l)
        except ValueError:
            self._abort(error.ResponseError(_("unexpected response:"), l))

    def _recv(self):
        return self._pipei.read(self._getamount())

    def _send(self, data, flush=False):
        self._pipeo.write(b"%d\n" % len(data))
        if data:
            self._pipeo.write(data)
        if flush:
            self._pipeo.flush()


instance = stdiopeer
