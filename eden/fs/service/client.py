# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import, division, print_function, unicode_literals

import os
from typing import Any, cast

from facebook.eden import EdenService
from thrift.protocol.THeaderProtocol import THeaderProtocol
from thrift.transport.THeaderTransport import THeaderTransport
from thrift.transport.TSocket import TSocket
from thrift.transport.TTransport import TTransportException


SOCKET_PATH = "socket"


class EdenNotRunningError(Exception):

    def __init__(self, eden_dir):
        msg = "edenfs daemon does not appear to be running: tried %s" % eden_dir
        super(EdenNotRunningError, self).__init__(msg)
        self.eden_dir = eden_dir


# Monkey-patch EdenService.EdenError's __str__() behavior to just return the
# error message.  By default it returns the same data as __repr__(), which is
# ugly to show to users.
def _eden_thrift_error_str(ex):
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

    def __init__(self, eden_dir=None, socket_path=None):
        if socket_path is not None:
            self._socket_path = socket_path
        elif eden_dir is not None:
            self._socket_path = os.path.join(eden_dir, SOCKET_PATH)
        else:
            raise TypeError("one of eden_dir or socket_path is required")
        self._socket = TSocket(unix_socket=self._socket_path)
        # We used to set a timeout here, but picking the right duration is hard,
        # and safely retrying an arbitrary thrift call may not be safe.  So we
        # just leave the client with no timeout.
        # self._socket.setTimeout(60000)  # in milliseconds
        self._transport = THeaderTransport(self._socket)
        self._protocol = THeaderProtocol(self._transport)
        super(EdenClient, self).__init__(self._protocol)

    def __enter__(self):
        self.open()
        return self

    def __exit__(self, exc_type, exc_value, exc_traceback):
        self.close()

    def open(self):
        try:
            self._transport.open()
        except TTransportException as ex:
            self.close()
            if ex.type == TTransportException.NOT_OPEN:
                raise EdenNotRunningError(self._socket_path)
            raise

    def close(self):
        if self._transport is not None:
            self._transport.close()
            self._transport = None


def create_thrift_client(eden_dir=None, socket_path=None):
    """Construct a thrift client to speak to the running eden server
    instance associated with the specified mount point.

    @return Returns a context manager for EdenService.Client.
    """
    return EdenClient(eden_dir=eden_dir, socket_path=socket_path)
