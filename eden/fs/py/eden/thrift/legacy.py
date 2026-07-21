# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


from __future__ import absolute_import, division, print_function, unicode_literals

import os
from typing import Any, Optional

from eden.thrift.client import (
    create_thrift_client as _create_modern_client,
    EdenNotRunningError,
)


__all__ = ["EdenClient", "EdenNotRunningError", "SOCKET_PATH", "create_thrift_client"]

SOCKET_PATH = "socket"


class EdenClient:
    """
    Backwards-compatible wrapper around the modern thrift-python client.

    Delegates all thrift method calls to the underlying EdenService.Sync
    client obtained from eden.thrift.client.create_thrift_client().
    """

    def __init__(
        self,
        eden_dir: Optional[str],
        socket_path: Optional[str],
        timeout: Optional[float],
    ) -> None:
        self._eden_dir = eden_dir
        self._socket_path = socket_path
        self._timeout = timeout
        # pyre-fixme[4]: Attribute must be annotated.
        self._client = None
        # pyre-fixme[4]: Attribute must be annotated.
        self._ctx = None

    def __enter__(self) -> "EdenClient":
        if self._client is not None:
            raise RuntimeError("EdenClient is already connected")
        self._ctx = _create_modern_client(
            eden_dir=self._eden_dir,
            socket_path=self._socket_path,
            timeout=self._timeout if self._timeout is not None else 0,
        )
        try:
            self._client = self._ctx.__enter__()
        except BaseException:
            self._ctx = None
            raise
        return self

    def __exit__(
        self,
        # pyre-fixme[2]: Parameter annotation cannot be `Any`.
        exc_type: "Any",
        # pyre-fixme[2]: Parameter annotation cannot be `Any`.
        exc_value: "Any",
        # pyre-fixme[2]: Parameter annotation cannot be `Any`.
        exc_traceback: "Any",
    ) -> "Optional[bool]":
        if self._ctx is not None:
            try:
                self._ctx.__exit__(exc_type, exc_value, exc_traceback)
            finally:
                self._client = None
                self._ctx = None
        return False

    # pyre-fixme[3]: Return type must be annotated.
    def __getattr__(self, name: str) -> Any:
        """Delegate all thrift method calls to the underlying modern client."""
        if self._client is None:
            raise RuntimeError("EdenClient is not connected; use as a context manager")
        return getattr(self._client, name)

    def getPid(self) -> int:
        return self.getDaemonInfo().pid


def create_thrift_client(
    eden_dir: "Optional[str]" = None,
    socket_path: "Optional[str]" = None,
    timeout: "Optional[float]" = None,
) -> "EdenClient":
    """
    Construct a thrift client to speak to the running eden server
    instance associated with the specified mount point.

    @return Returns a context manager for EdenClient.
    """
    if socket_path is None and eden_dir is not None:
        socket_path = os.path.join(eden_dir, SOCKET_PATH)
    elif socket_path is None:
        raise TypeError("one of eden_dir or socket_path is required")

    return EdenClient(eden_dir=eden_dir, socket_path=socket_path, timeout=timeout)
