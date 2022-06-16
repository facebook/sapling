#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
from typing import Optional

from eden.fs.service.streamingeden.clients import StreamingEdenService
from thrift.py3 import get_client
from thrift.py3.client import ClientType
from thrift.py3.exceptions import TransportError, TransportErrorType

SOCKET_PATH = "socket"


class EdenNotRunningError(Exception):
    def __init__(self, socket_path: str):
        super(EdenNotRunningError, self).__init__(
            f"edenfs daemon does not appear to be running: tried {socket_path}"
        )
        self.socket_path = socket_path


class EdenClient(StreamingEdenService):
    """
    EdenClient is a subclass of EdenService that provides
    some conveniences and helps deal with evolving Thrift APIs.
    """

    socket_path: Optional[str] = None

    async def __aenter__(self):
        try:
            return await super().__aenter__()
        except TransportError as ex:
            if ex.type == TransportErrorType.NOT_OPEN:
                raise EdenNotRunningError(self.socket_path)
            raise


def create_thrift_client(
    eden_dir: Optional[str] = None,
    socket_path: Optional[str] = None,
    timeout: Optional[float] = None,
) -> EdenClient:
    """
    Construct a thrift client to speak to the running eden server
    instance associated with the specified mount point.

    @return Returns an EdenService.Client.
    """

    if socket_path is not None:
        pass
    elif eden_dir is not None:
        socket_path = os.path.join(eden_dir, SOCKET_PATH)
    else:
        raise TypeError("one of eden_dir or socket_path is required")

    if timeout is None:
        # We used to set a default timeout here, but picking the right duration is hard,
        # and safely retrying an arbitrary thrift call may not be safe.  So we
        # just leave the client with no timeout, unless one is given.
        timeout = 0

    client = get_client(
        EdenClient,
        path=socket_path,
        timeout=timeout,
        client_type=ClientType.THRIFT_ROCKET_CLIENT_TYPE,
    )
    client.socket_path = socket_path
    return client
