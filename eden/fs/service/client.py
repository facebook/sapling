# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals

from facebook.eden import EdenService
from thrift.protocol.THeaderProtocol import THeaderProtocol
from thrift.transport.THeaderTransport import THeaderTransport
from thrift.transport.TSocket import TSocket
from thrift.transport.TTransport import TTransportException

import os

SOCKET_PATH = 'socket'


class EdenNotRunningError(Exception):
    def __init__(self, eden_dir):
        msg = 'edenfs daemon does not appear to be running'
        super(EdenNotRunningError, self).__init__(msg)
        self.eden_dir = eden_dir


# Monkey-patch EdenService.EdenError's __str__() behavior to just return the
# error message.  By default it returns the same data as __repr__(), which is
# ugly to show to users.
def _eden_thrift_error_str(ex):
    return ex.message

EdenService.EdenError.__str__ = _eden_thrift_error_str


class EdenClient(EdenService.Client):
    '''
    EdenClient is a subclass of EdenService.Client that provides
    a few additional conveniences:

    - Smarter constructor
    - Implement the context manager __enter__ and __exit__ methods, so it can
      be used in with statements.
    '''
    def __init__(self, eden_dir):
        self._eden_dir = eden_dir
        sock_path = os.path.join(self._eden_dir, SOCKET_PATH)
        self._socket = TSocket(unix_socket=sock_path)
        self._socket.setTimeout(60000)  # in milliseconds
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
            self._transport.close()
            if ex.type == TTransportException.NOT_OPEN:
                raise EdenNotRunningError(self._eden_dir)

    def close(self):
        if self._transport is not None:
            self._transport.close()
            self._transport = None


def create_thrift_client(config_dir):
    '''Construct a thrift client to speak to the running eden server
    instance associated with the specified mount point.

    @return Returns a context manager for EdenService.Client.
    '''
    return EdenClient(config_dir)
