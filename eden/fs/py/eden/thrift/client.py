# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from typing import Optional

from eden.fs.service.eden.thrift_clients import EdenService
from thrift.python.client import ClientType, get_client

SOCKET_PATH = "socket"


def create_thrift_client(
    eden_dir: Optional[str] = None,
    socket_path: Optional[str] = None,
    timeout: float = 0,
) -> EdenService.Async:
    """
    Create a Thrift client for communicating with the Eden Thrift server.

    Args:
        eden_dir: Path to the Eden mount directory. Used to derive the socket path if socket_path is not provided.
        socket_path: Socket path to connect to the Eden server directly.
        timeout: Timeout in seconds for client operations.
    Returns:
        An async context manager for an EdenFS client connected to the specified Eden mount.
    Raises:
        TypeError: If neither eden_dir nor socket_path is provided.
    """

    if socket_path is None:
        if eden_dir is not None:
            socket_path = os.path.join(eden_dir, SOCKET_PATH)
        else:
            raise TypeError("one of eden_dir or socket_path is required")

    return get_client(
        EdenService,
        path=socket_path,
        timeout=timeout,
        client_type=ClientType.THRIFT_ROCKET_CLIENT_TYPE,
    )
