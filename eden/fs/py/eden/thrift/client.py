# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from contextlib import contextmanager
from typing import Generator, Optional

from eden.fs.service.eden.thrift_clients import EdenService
from thrift.python.client import ClientType, get_client, get_sync_client
from thrift.python.exceptions import TransportError

SOCKET_PATH = "socket"


class EdenNotRunningError(Exception):
    """
    Exception raised when the EdenFS daemon is not running.

    This is the modern thrift-python equivalent of legacy.EdenNotRunningError.
    It wraps TransportError exceptions that occur when attempting to connect
    to the EdenFS daemon's Unix socket.
    """

    def __init__(self, eden_dir: str, cause: Optional[TransportError] = None) -> None:
        msg = f"edenfs daemon does not appear to be running: tried {eden_dir}"
        super().__init__(msg)
        self.eden_dir = eden_dir
        self.__cause__ = cause


@contextmanager
def create_thrift_client(
    eden_dir: Optional[str] = None,
    socket_path: Optional[str] = None,
    timeout: float = 0,
) -> Generator[EdenService.Sync, None, None]:
    """
    Create a synchronous Thrift client for communicating with the Eden Thrift server.

    Args:
        eden_dir: Path to the Eden mount directory. Used to derive the socket path if socket_path is not provided.
        socket_path: Socket path to connect to the Eden server directly.
        timeout: Timeout in seconds for client operations.

    Yields:
        A sync EdenFS client connected to the specified Eden mount.

    Raises:
        TypeError: If neither eden_dir nor socket_path is provided.
        EdenNotRunningError: If unable to connect to the EdenFS daemon.
    """
    if socket_path is None:
        if eden_dir is not None:
            socket_path = os.path.join(eden_dir, SOCKET_PATH)
        else:
            raise TypeError("one of eden_dir or socket_path is required")

    try:
        with get_sync_client(
            EdenService,
            path=socket_path,
            timeout=timeout,
            client_type=ClientType.THRIFT_ROCKET_CLIENT_TYPE,
        ) as client:
            yield client
    except TransportError as e:
        raise EdenNotRunningError(socket_path, e) from e


def create_async_thrift_client(
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
