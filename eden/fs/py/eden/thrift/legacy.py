# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from __future__ import absolute_import, division, print_function, unicode_literals

import os
import sys
from typing import Any, cast, Optional  # noqa: F401

from facebook.eden import EdenService
from facebook.eden.ttypes import DaemonInfo
from thrift.protocol.THeaderProtocol import THeaderProtocol
from thrift.Thrift import TApplicationException
from thrift.transport.THeaderTransport import THeaderTransport
from thrift.transport.TTransport import TTransportException

if sys.platform == "win32":
    from eden.thrift.windows_thrift import WindowsSocketException, WinTSocket  # @manual
else:
    from thrift.transport.TSocket import TSocket

    class WindowsSocketException(Exception):
        pass


SOCKET_PATH = "socket"


class EdenNotRunningError(Exception):
    def __init__(self, eden_dir: str) -> None:
        msg = "edenfs daemon does not appear to be running: tried %s" % eden_dir
        super(EdenNotRunningError, self).__init__(msg)
        self.eden_dir = eden_dir


# Monkey-patch EdenService.EdenError's __str__() behavior to just return the
# error message.  By default it returns the same data as __repr__(), which is
# ugly to show to users.
def _eden_thrift_error_str(ex: "EdenService.EdenError") -> str:
    return ex.message


# TODO: https://github.com/python/mypy/issues/2427
cast(Any, EdenService.EdenError).__str__ = _eden_thrift_error_str


class EdenClient(EdenService.Client):
    """
    EdenClient is a subclass of EdenService.Client that provides
    a few additional conveniences:

    - Smarter constructor
    - Implement the context manager __enter__ and __exit__ methods, so it can
      be used in with statements.
    """

    def __init__(
        self,
        socket_path: str,
        transport: "THeaderTransport",
        protocol: "THeaderProtocol",
    ) -> None:
        self._socket_path = socket_path
        self._transport: "Optional[THeaderTransport]" = transport

        super(EdenClient, self).__init__(protocol)

    def __enter__(self) -> "EdenClient":
        self.open()
        return self

    def __exit__(
        self, exc_type: "Any", exc_value: "Any", exc_traceback: "Any"
    ) -> "Optional[bool]":
        self.close()
        return False

    def open(self) -> None:
        transport = self._transport
        assert transport is not None
        try:
            transport.open()
        except TTransportException as ex:
            self.close()
            if ex.type == TTransportException.NOT_OPEN:
                raise EdenNotRunningError(self._socket_path)
            raise
        except WindowsSocketException:
            self.close()
            raise EdenNotRunningError(self._socket_path)

    def close(self) -> None:
        if self._transport is not None:
            self._transport.close()
            self._transport = None

    def getDaemonInfo(self) -> "DaemonInfo":
        try:
            info = super(EdenClient, self).getDaemonInfo()
        except TApplicationException as ex:
            if ex.type != TApplicationException.UNKNOWN_METHOD:
                raise
            # Older versions of EdenFS did not have a getDaemonInfo() method
            pid = super(EdenClient, self).getPid()
            info = DaemonInfo(pid=pid, status=None)

        # Older versions of EdenFS did not return status information in the
        # getDaemonInfo() response.
        if info.status is None:
            info.status = super(EdenClient, self).getStatus()
        return info

    def getPid(self) -> int:
        try:
            return self.getDaemonInfo().pid
        except TApplicationException as ex:
            if ex.type == TApplicationException.UNKNOWN_METHOD:
                # Running on an older server build, fall back to the
                # old getPid() method.
                return super(EdenClient, self).getPid()
            else:
                raise


def create_thrift_client(
    eden_dir: "Optional[str]" = None,
    socket_path: "Optional[str]" = None,
    timeout: "Optional[float]" = None,
) -> "EdenClient":
    """
    Construct a thrift client to speak to the running eden server
    instance associated with the specified mount point.

    @return Returns a context manager for EdenService.Client.
    """

    if socket_path is not None:
        pass
    elif eden_dir is not None:
        socket_path = os.path.join(eden_dir, SOCKET_PATH)
    else:
        raise TypeError("one of eden_dir or socket_path is required")
    if sys.platform == "win32":
        socket = WinTSocket(unix_socket=socket_path)
    else:
        socket = TSocket(unix_socket=socket_path)

    # We used to set a default timeout here, but picking the right duration is hard,
    # and safely retrying an arbitrary thrift call may not be safe.  So we
    # just leave the client with no timeout, unless one is given.
    if timeout is None:
        timeout_ms = None
    else:
        timeout_ms = timeout * 1000
    socket.setTimeout(timeout_ms)

    transport = THeaderTransport(socket)
    protocol = THeaderProtocol(transport)
    return EdenClient(socket_path, transport, protocol)
