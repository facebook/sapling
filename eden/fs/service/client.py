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


def create_thrift_client(config_dir):
    '''Construct a thrift client to speak to the running eden server
    instance associated with the specified mount point.

    @return EdenService.Client
    '''
    sock_path = os.path.join(config_dir, SOCKET_PATH)
    sock = TSocket(unix_socket=sock_path)
    sock.setTimeout(60000)  # in milliseconds
    transport = THeaderTransport(sock)
    protocol = THeaderProtocol(transport)
    client = EdenService.Client(protocol)

    try:
        transport.open()
    except TTransportException as ex:
        if ex.type == TTransportException.NOT_OPEN:
            raise EdenNotRunningError(config_dir)

    return client
